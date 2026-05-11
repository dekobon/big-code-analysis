use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::bytes::Regex;
use termcolor::{Color, ColorSpec, StandardStreamLock, WriteColor};

use crate::langs::fake;
use crate::langs::*;

/// Reads a file, normalising all CR-only and CRLF line endings to LF.
///
/// **Note for downstream consumers**: the returned buffer never contains `\r`
/// bytes. Callers that previously observed raw `\r\n` sequences will see plain
/// `\n` after this call. This is intentional — the metric engine requires LF-
/// only input — but it is a behavioural difference from a plain `fs::read`.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use big_code_analysis::read_file;
///
/// let path = Path::new("Cargo.toml");
/// read_file(&path).unwrap();
/// ```
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    normalize_line_endings(&mut data);

    Ok(data)
}

/// Reads a file, normalising all CR-only and CRLF line endings to LF, and ensures
/// the buffer ends with exactly one `\n`. Returns `None` for files ≤ 3 bytes or
/// files that appear to be non-UTF-8.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use big_code_analysis::read_file_with_eol;
///
/// let path = Path::new("Cargo.toml");
/// read_file_with_eol(&path).unwrap();
/// ```
pub fn read_file_with_eol(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    let file_size = fs::metadata(path).map_or(1024 * 1024, |m| m.len() as usize);
    if file_size <= 3 {
        // this file is very likely almost empty... so nothing to do on it
        return Ok(None);
    }

    let mut file = File::open(path)?;

    let mut start = vec![0; 64.min(file_size)];
    let start = if file.read_exact(&mut start).is_ok() {
        // Skip the bom if one
        if start[..2] == [b'\xFE', b'\xFF'] || start[..2] == [b'\xFF', b'\xFE'] {
            &start[2..]
        } else if start[..3] == [b'\xEF', b'\xBB', b'\xBF'] {
            &start[3..]
        } else {
            &start
        }
    } else {
        return Ok(None);
    };

    // so start contains more or less 64 chars
    let mut head = String::from_utf8_lossy(start).into_owned();
    // The last char could be wrong because we were in the middle of an utf-8 sequence
    head.pop();
    // now check if there is an invalid char
    if head.contains('\u{FFFD}') {
        return Ok(None);
    }

    let mut data = Vec::with_capacity(file_size + 2);
    data.extend_from_slice(start);

    file.read_to_end(&mut data)?;

    normalize_line_endings(&mut data);

    Ok(Some(data))
}

/// Writes data to a file.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use big_code_analysis::write_file;
///
/// let path = Path::new("foo.txt");
/// let data: [u8; 4] = [0; 4];
/// write_file(&path, &data).unwrap();
/// ```
pub fn write_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(data)?;

    Ok(())
}

/// Detects the language of a code using
/// the extension of a file.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use big_code_analysis::get_language_for_file;
///
/// let path = Path::new("build.rs");
/// get_language_for_file(&path).unwrap();
/// ```
pub fn get_language_for_file(path: &Path) -> Option<LANG> {
    if let Some(ext) = path.extension() {
        let ext = ext.to_str()?.to_lowercase();
        get_from_ext(&ext)
    } else {
        None
    }
}

fn mode_to_str(mode: &[u8]) -> Option<String> {
    std::str::from_utf8(mode).ok().map(|m| m.to_lowercase())
}

// comment containing coding info are useful
static RE1_EMACS: OnceLock<Regex> = OnceLock::new();
static RE2_EMACS: OnceLock<Regex> = OnceLock::new();
static RE1_VIM: OnceLock<Regex> = OnceLock::new();
static RE_GENERATED: OnceLock<Regex> = OnceLock::new();

// Regular expressions
const FIRST_EMACS_EXPRESSION: &str = r"(?i)-\*-.*[^-\w]mode\s*:\s*([^:;\s]+)";
const SECOND_EMACS_EXPRESSION: &str = r"-\*-\s*([^:;\s]+)\s*-\*-";
const VIM_EXPRESSION: &str = r"(?i)vim\s*:.*[^\w]ft\s*=\s*([^:\s]+)";

// Generated-code marker patterns. Matched against the leading window of the
// file (see `is_generated`) so a marker phrase deep in the body does not
// trigger a skip. Each alternative covers a widely-used convention:
//
// - `@generated`      — Facebook / Meta convention, also used by buck2,
//                       rustfmt, prettier, and many code generators.
// - `DO NOT EDIT`     — Go's `Code generated ... DO NOT EDIT.` line is
//                       canonical, but the bare phrase appears in Bazel,
//                       protoc, OpenAPI clients, etc. — match either.
// - `GENERATED CODE`  — Lizard's marker; preserved for compatibility with
//                       projects that already tag generated files this way.
const GENERATED_EXPRESSION: &str = r"(?i)@generated\b|DO NOT EDIT|GENERATED CODE";

/// Bytes from the start of the file scanned for a generated-code marker.
/// 5 KiB is enough to cover any reasonable file header (license + autogen
/// preamble) without paying a meaningful read cost.
const GENERATED_SCAN_BYTES: usize = 5 * 1024;
/// Maximum lines scanned for a generated-code marker. Caps the work on a
/// pathological "all-on-one-line" file.
const GENERATED_SCAN_LINES: usize = 50;

/// Returns `true` when `buf` looks like generated code: its leading window
/// (first ~50 lines or first 5 KiB, whichever is smaller) contains a known
/// marker phrase. Matching is case-insensitive for the marker and never
/// allocates on the negative path.
///
/// Recognized markers:
///
/// - `@generated` — Facebook / Meta convention, also used by buck2,
///   rustfmt, and prettier.
/// - `DO NOT EDIT` — Go's `Code generated by ... DO NOT EDIT.` is the
///   canonical form; the bare phrase is also widely copied.
/// - `GENERATED CODE` — Lizard's marker, preserved for compatibility.
///
/// Detection runs against raw bytes before parsing, so callers can discard
/// generated files without paying tree-sitter parse cost. Non-UTF-8 input
/// will not panic — `regex::bytes::Regex` operates on the raw byte slice.
///
/// # Examples
///
/// ```
/// use big_code_analysis::is_generated;
///
/// assert!(is_generated(b"// @generated\nfn x() {}\n"));
/// assert!(is_generated(
///     b"// Code generated by protoc. DO NOT EDIT.\npackage x\n",
/// ));
/// assert!(!is_generated(b"fn main() { /* not generated */ }\n"));
/// ```
pub fn is_generated(buf: &[u8]) -> bool {
    // Strip a leading UTF-8 BOM so a marker on the first line of a
    // BOM-prefixed file still matches against the line start. UTF-16 BOMs
    // are not handled: the byte-pattern regex cannot match the
    // interleaved-zero encoding (`@\x00g\x00...`) that follows a UTF-16
    // BOM, so a strip would not enable detection — it would only obscure
    // the fact that UTF-16 source files are unsupported here.
    let buf = buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(buf);

    // Bound the search window: at most GENERATED_SCAN_BYTES bytes, and
    // among those, stop after GENERATED_SCAN_LINES newlines. Scanning fewer
    // lines avoids matching a marker phrase deep in the file body (the
    // negative case in the issue's acceptance criteria).
    let cap = buf.len().min(GENERATED_SCAN_BYTES);
    let end = buf[..cap]
        .iter()
        .enumerate()
        .filter_map(|(i, &b)| (b == b'\n').then_some(i + 1))
        .nth(GENERATED_SCAN_LINES - 1)
        .unwrap_or(cap);
    let window = &buf[..end];

    RE_GENERATED
        .get_or_init(|| {
            Regex::new(GENERATED_EXPRESSION).expect("GENERATED_EXPRESSION is a constant regex")
        })
        .is_match(window)
}

#[inline]
fn get_regex<'a>(
    once_lock: &OnceLock<Regex>,
    line: &'a [u8],
    regex: &'a str,
) -> Option<regex::bytes::Captures<'a>> {
    once_lock
        .get_or_init(|| Regex::new(regex).unwrap())
        .captures_iter(line)
        .next()
}

/// Resolves a language from a script's shebang line.
///
/// Returns `None` unless `buf` starts with `#!`. Reads up to the first `\n`,
/// strips an optional trailing `\r`, splits on whitespace, and takes the
/// basename of either the first token or — when that basename is `env` — the
/// next non-flag token. Trailing version digits and dots (`python3`,
/// `lua5.1`, `perl5.36`) are stripped before lookup. Non-UTF-8 bytes on the
/// shebang line yield `None` (no panic).
fn get_shebang_lang(buf: &[u8]) -> Option<LANG> {
    // Early-out for the common case (any non-shebang buffer): no allocation,
    // no UTF-8 decoding.
    let rest = buf.strip_prefix(b"#!")?;
    let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
    let line = &rest[..line_end];
    // Trim a trailing CR even though normalize_line_endings should have removed
    // it — guess_language is on the public API and may be called with raw input.
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = std::str::from_utf8(line).ok()?;

    let mut tokens = line.split_ascii_whitespace();
    let first_base = basename(tokens.next()?);

    let interpreter = if first_base == "env" {
        skip_env_args(&mut tokens)?
    } else {
        first_base
    };

    get_from_interpreter(strip_version_suffix(interpreter))
}

// Walk past leading `env` arguments (`-FLAG`, `-u VAR`, `NAME=value`) and
// return the basename of the actual interpreter token. Per `env(1)`, only
// `-u` consumes a following argument; other short flags (`-i`, `-S`, …)
// stand alone or carry their argument inline (e.g. `-S "node --foo"`).
fn skip_env_args<'a>(tokens: &mut std::str::SplitAsciiWhitespace<'a>) -> Option<&'a str> {
    loop {
        let tok = tokens.next()?;
        if let Some(flag) = tok.strip_prefix('-') {
            if flag == "u" {
                tokens.next()?;
            }
            continue;
        }
        if tok.contains('=') {
            continue;
        }
        return Some(basename(tok));
    }
}

fn basename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// Strips a trailing run of digits and dots used to encode an interpreter
/// version (`python3` → `python`, `lua5.1` → `lua`, `perl5.36` → `perl`).
fn strip_version_suffix(name: &str) -> &str {
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if trimmed.is_empty() { name } else { trimmed }
}

fn get_from_interpreter(name: &str) -> Option<LANG> {
    match name {
        "sh" | "bash" | "dash" | "ksh" | "zsh" => Some(LANG::Bash),
        "python" => Some(LANG::Python),
        "perl" => Some(LANG::Perl),
        "lua" | "luajit" => Some(LANG::Lua),
        "php" | "php-cgi" => Some(LANG::Php),
        "node" | "nodejs" => Some(LANG::Javascript),
        "tclsh" | "wish" => Some(LANG::Tcl),
        _ => None,
    }
}

fn get_emacs_mode(buf: &[u8]) -> Option<String> {
    // we just try to use the emacs info (if there)
    for (i, line) in buf.splitn(5, |c| *c == b'\n').enumerate() {
        if let Some(cap) = get_regex(&RE1_EMACS, line, FIRST_EMACS_EXPRESSION) {
            return mode_to_str(&cap[1]);
        } else if let Some(cap) = get_regex(&RE2_EMACS, line, SECOND_EMACS_EXPRESSION) {
            return mode_to_str(&cap[1]);
        } else if let Some(cap) = get_regex(&RE1_VIM, line, VIM_EXPRESSION) {
            return mode_to_str(&cap[1]);
        }
        if i == 3 {
            break;
        }
    }

    for (i, line) in buf.rsplitn(5, |c| *c == b'\n').enumerate() {
        if let Some(cap) = get_regex(&RE1_VIM, line, VIM_EXPRESSION) {
            return mode_to_str(&cap[1]);
        }
        if i == 3 {
            break;
        }
    }

    None
}

/// Guesses the language of a code.
///
/// Returns a tuple containing a [`LANG`] as first argument
/// and the language name as a second one.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use big_code_analysis::guess_language;
///
/// let source_code = "int a = 42;";
///
/// // The path to a dummy file used to contain the source code
/// let path = PathBuf::from("foo.c");
/// let source_slice = source_code.as_bytes();
///
/// // Guess the language of a code
/// guess_language(&source_slice, &path);
/// ```
///
/// [`LANG`]: enum.LANG.html
pub fn guess_language<'a, P: AsRef<Path>>(buf: &[u8], path: P) -> (Option<LANG>, &'a str) {
    let ext = path
        .as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let from_ext = get_from_ext(&ext);

    let mode = get_emacs_mode(buf).unwrap_or_default();

    let from_mode = get_from_emacs_mode(&mode);

    if let Some(lang_ext) = from_ext {
        if let Some(lang_mode) = from_mode {
            if lang_ext == lang_mode {
                (
                    Some(lang_mode),
                    fake::get_true(&ext, &mode).unwrap_or_else(|| lang_mode.get_name()),
                )
            } else {
                // we should probably rely on extension here
                (Some(lang_ext), lang_ext.get_name())
            }
        } else {
            (
                Some(lang_ext),
                fake::get_true(&ext, &mode).unwrap_or_else(|| lang_ext.get_name()),
            )
        }
    } else if let Some(lang_mode) = from_mode {
        (
            Some(lang_mode),
            fake::get_true(&ext, &mode).unwrap_or_else(|| lang_mode.get_name()),
        )
    } else if let Some(lang_shebang) = get_shebang_lang(buf) {
        (
            Some(lang_shebang),
            fake::get_true(&ext, &mode).unwrap_or_else(|| lang_shebang.get_name()),
        )
    } else {
        (None, fake::get_true(&ext, &mode).unwrap_or_default())
    }
}

/// Normalises all CR-only and CRLF line endings to LF throughout the buffer,
/// then ensures the buffer ends with exactly one `\n`.
pub(crate) fn normalize_line_endings(data: &mut Vec<u8>) {
    // In-place compaction: write pointer stays ≤ read pointer, so no extra allocation.
    let mut w = 0;
    let mut r = 0;
    while r < data.len() {
        if data[r] == b'\r' {
            data[w] = b'\n';
            w += 1;
            r += if data.get(r + 1).copied() == Some(b'\n') {
                2
            } else {
                1
            };
        } else {
            data[w] = data[r];
            w += 1;
            r += 1;
        }
    }
    data.truncate(w);
    let trailing = data.iter().rev().take_while(|&&c| c == b'\n').count();
    data.truncate(data.len() - trailing);
    data.push(b'\n');
}

pub(crate) fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
    // Copied from Cargo sources: https://github.com/rust-lang/cargo/blob/master/src/cargo/util/paths.rs#L65
    let mut components = path.as_ref().components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

pub(crate) fn get_paths_dist(path1: &Path, path2: &Path) -> Option<usize> {
    for ancestor in path1.ancestors() {
        if path2.starts_with(ancestor) && !ancestor.as_os_str().is_empty() {
            let path1 = path1.strip_prefix(ancestor).unwrap();
            let path2 = path2.strip_prefix(ancestor).unwrap();
            return Some(path1.components().count() + path2.components().count());
        }
    }
    None
}

pub(crate) fn guess_file<S: ::std::hash::BuildHasher>(
    current_path: &Path,
    include_path: &str,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) -> Vec<PathBuf> {
    let include_path = if let Some(end) = include_path.strip_prefix("mozilla/") {
        end
    } else {
        include_path
    };
    let include_path = normalize_path(include_path);
    let Some(file_name) = include_path.file_name() else {
        return vec![];
    };
    let Some(file_name) = file_name.to_str() else {
        return vec![];
    };
    if let Some(possibilities) = all_files.get(file_name) {
        if possibilities.len() == 1 {
            // Only one file with this name
            return possibilities.clone();
        }

        let mut new_possibilities = Vec::new();
        for p in possibilities.iter() {
            if p.ends_with(&include_path) && current_path != p {
                new_possibilities.push(p.clone());
            }
        }
        if new_possibilities.len() == 1 {
            // Only one path is finishing with "foo/Bar.h"
            return new_possibilities;
        }
        new_possibilities.clear();

        if let Some(parent) = current_path.parent() {
            for p in possibilities.iter() {
                if p.starts_with(parent) && current_path != p {
                    new_possibilities.push(p.clone());
                }
            }
            if new_possibilities.len() == 1 {
                // Only one path in the current working directory (current_path)
                return new_possibilities;
            }
            new_possibilities.clear();
        }

        let mut dist_min = usize::MAX;
        let mut path_min = Vec::new();
        for p in possibilities.iter() {
            if current_path == p {
                continue;
            }
            if let Some(dist) = get_paths_dist(current_path, p) {
                match dist.cmp(&dist_min) {
                    Ordering::Less => {
                        dist_min = dist;
                        path_min.clear();
                        path_min.push(p);
                    }
                    Ordering::Equal => {
                        path_min.push(p);
                    }
                    Ordering::Greater => {}
                }
            }
        }

        let path_min: Vec<_> = path_min.drain(..).cloned().collect();
        return path_min;
    }

    vec![]
}

#[inline]
pub(crate) fn color(stdout: &mut StandardStreamLock, color: Color) -> std::io::Result<()> {
    stdout.set_color(ColorSpec::new().set_fg(Some(color)))
}

#[inline]
pub(crate) fn intense_color(stdout: &mut StandardStreamLock, color: Color) -> std::io::Result<()> {
    stdout.set_color(ColorSpec::new().set_fg(Some(color)).set_intense(true))
}

#[cfg(test)]
pub(crate) fn check_func_space<T: crate::ParserTrait, F: Fn(crate::FuncSpace)>(
    source: &str,
    filename: &str,
    check: F,
) {
    let path = std::path::PathBuf::from(filename);
    // Mirror the CRLF/CR normalisation that read_file_with_eol applies via normalize_line_endings
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut trimmed_bytes = normalized.trim_end().trim_matches('\n').as_bytes().to_vec();
    trimmed_bytes.push(b'\n');
    let parser = T::new(trimmed_bytes, &path, None);
    let func_space = crate::metrics(&parser, &path).unwrap();

    check(func_space);
}

#[cfg(test)]
pub(crate) fn check_metrics<T: crate::ParserTrait>(
    source: &str,
    filename: &str,
    check: fn(crate::CodeMetrics) -> (),
) {
    check_func_space::<T, _>(source, filename, |func_space| check(func_space.metrics));
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_read() {
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join("test_read");
        let data = vec![
            (b"\xFF\xFEabc".to_vec(), Some(b"abc\n".to_vec())),
            (b"\xFE\xFFabc".to_vec(), Some(b"abc\n".to_vec())),
            (b"\xEF\xBB\xBFabc".to_vec(), Some(b"abc\n".to_vec())),
            (b"\xEF\xBB\xBFabc\n".to_vec(), Some(b"abc\n".to_vec())),
            (b"\xEF\xBBabc\n".to_vec(), None),
            (b"abcdef\n".to_vec(), Some(b"abcdef\n".to_vec())),
            (b"abcdef".to_vec(), Some(b"abcdef\n".to_vec())),
            // CRLF throughout should be normalised to LF
            (b"abc\r\ndef\r\n".to_vec(), Some(b"abc\ndef\n".to_vec())),
            // UTF-8 BOM + CRLF
            (
                b"\xEF\xBB\xBFabc\r\ndef\r\n".to_vec(),
                Some(b"abc\ndef\n".to_vec()),
            ),
        ];
        for (d, expected) in data {
            write_file(&tmp_path, &d).unwrap();
            let res = read_file_with_eol(&tmp_path).unwrap();
            assert_eq!(res, expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_get_language_for_file_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"foo.\xff"));
        assert_eq!(get_language_for_file(path), None);
    }

    #[cfg(unix)]
    #[test]
    fn test_guess_language_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::PathBuf;

        let path = PathBuf::from(OsStr::from_bytes(b"foo.\xff"));
        let (lang, _name) = guess_language(b"int a = 42;", &path);
        assert_eq!(lang, None);
    }

    #[test]
    fn test_guess_file_no_file_name() {
        let all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let current = Path::new("/some/file.c");
        let result = guess_file(current, "..", &all_files);
        assert!(result.is_empty());
    }

    #[test]
    fn test_guess_language() {
        let buf = b"// -*- foo: bar; mode: c++; hello: world\n";
        assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "c/c++"));

        let buf = b"// -*- c++ -*-\n";
        assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "c/c++"));

        let buf = b"// -*- foo: bar; bar-mode: c++; hello: world\n";
        assert_eq!(
            guess_language(buf, "foo.py"),
            (Some(LANG::Python), "python")
        );

        let buf = b"/* hello world */\n";
        assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "c/c++"));

        let buf = b"\n\n\n\n\n\n\n\n\n// vim: set ts=4 ft=c++\n\n\n";
        assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::Cpp), "c/c++"));

        let buf = b"\n\n\n\n\n\n\n\n\n\n\n\n";
        assert_eq!(guess_language(buf, "foo.txt"), (None, ""));

        let buf = b"// -*- foo: bar; mode: Objective-C++; hello: world\n";
        assert_eq!(
            guess_language(buf, "foo.mm"),
            (Some(LANG::Cpp), "obj-c/c++")
        );
    }

    #[test]
    fn shebang_bare_bash() {
        assert_eq!(get_shebang_lang(b"#!/bin/bash\n"), Some(LANG::Bash));
    }

    #[test]
    fn shebang_env_python3() {
        assert_eq!(
            get_shebang_lang(b"#!/usr/bin/env python3\n"),
            Some(LANG::Python),
        );
    }

    #[test]
    fn shebang_versioned_perl_with_flag() {
        assert_eq!(
            get_shebang_lang(b"#!/usr/bin/perl5.36 -w\n"),
            Some(LANG::Perl),
        );
    }

    #[test]
    fn shebang_env_dash_s_node() {
        assert_eq!(
            get_shebang_lang(b"#!/usr/bin/env -S node --experimental\n"),
            Some(LANG::Javascript),
        );
    }

    #[test]
    fn shebang_env_with_var_assignment() {
        // `env FOO=bar python3` — skip the assignment, find the interpreter.
        assert_eq!(
            get_shebang_lang(b"#!/usr/bin/env FOO=bar python3\n"),
            Some(LANG::Python),
        );
    }

    #[test]
    fn shebang_env_dash_u_consumes_next_token() {
        // `env -u VAR python3` — `-u` is the only `env` short flag that
        // consumes a following argument (the variable name to unset). Without
        // the special case, `VAR` would be misidentified as the interpreter.
        assert_eq!(
            get_shebang_lang(b"#!/usr/bin/env -u VAR python3\n"),
            Some(LANG::Python),
        );
    }

    #[test]
    fn shebang_versioned_lua() {
        assert_eq!(get_shebang_lang(b"#!/usr/bin/lua5.1\n"), Some(LANG::Lua));
    }

    #[test]
    fn shebang_node() {
        assert_eq!(
            get_shebang_lang(b"#!/usr/local/bin/node\n"),
            Some(LANG::Javascript),
        );
    }

    #[test]
    fn shebang_tclsh() {
        assert_eq!(get_shebang_lang(b"#!/usr/bin/tclsh\n"), Some(LANG::Tcl));
    }

    #[test]
    fn shebang_no_trailing_newline() {
        assert_eq!(get_shebang_lang(b"#!/bin/sh"), Some(LANG::Bash));
    }

    #[test]
    fn shebang_crlf_line_ending() {
        // guess_language usually receives LF-normalised input, but be defensive.
        assert_eq!(get_shebang_lang(b"#!/bin/bash\r\n"), Some(LANG::Bash));
    }

    #[test]
    fn shebang_empty_buffer() {
        assert_eq!(get_shebang_lang(b""), None);
    }

    #[test]
    fn shebang_single_byte() {
        assert_eq!(get_shebang_lang(b"#"), None);
    }

    #[test]
    fn shebang_no_shebang_prefix() {
        assert_eq!(get_shebang_lang(b"// not a shebang\n"), None);
    }

    #[test]
    fn shebang_unknown_interpreter() {
        assert_eq!(get_shebang_lang(b"#!/usr/bin/ruby\n"), None);
    }

    #[test]
    fn shebang_env_only_no_interpreter() {
        assert_eq!(get_shebang_lang(b"#!/usr/bin/env\n"), None);
    }

    #[test]
    fn shebang_non_utf8_returns_none() {
        // Invalid UTF-8 on the shebang line must not panic.
        assert_eq!(get_shebang_lang(b"#!/usr/bin/\xff\xfe\n"), None);
    }

    #[test]
    fn guess_language_extension_wins_over_shebang() {
        // The .py extension must outrank a `#!/bin/sh` shebang.
        let buf = b"#!/bin/sh\nprint('hi')\n";
        assert_eq!(
            guess_language(buf, "foo.py"),
            (Some(LANG::Python), "python")
        );
    }

    #[test]
    fn guess_language_shebang_falls_through_when_no_extension() {
        let buf = b"#!/usr/bin/env python3\nprint('hi')\n";
        assert_eq!(guess_language(buf, "run"), (Some(LANG::Python), "python"));
    }

    #[test]
    fn guess_language_shebang_loses_to_mode_line() {
        // Mode line outranks the shebang.
        let buf = b"#!/usr/bin/env node\n# -*- mode: python -*-\n";
        assert_eq!(guess_language(buf, "run"), (Some(LANG::Python), "python"));
    }

    #[test]
    fn normalize_line_endings_normalizes_crlf() {
        let mut d = b"code\r\n# comment\r\n".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"code\n# comment\n");
    }

    #[test]
    fn normalize_line_endings_normalizes_lone_cr() {
        let mut d = b"code\r# comment\r".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"code\n# comment\n");
    }

    #[test]
    fn normalize_line_endings_normalizes_cr_before_crlf() {
        // lone CR followed immediately by CRLF → two separate line breaks
        let mut d = b"a\r\r\nb".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"a\n\nb\n");
    }

    #[test]
    fn normalize_line_endings_normalizes_crlf_blank_line() {
        let mut d = b"a\r\n\r\nb\r\n".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"a\n\nb\n");
    }

    #[test]
    fn normalize_line_endings_empty_buffer() {
        let mut d = b"".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"\n");
    }

    #[test]
    fn is_generated_at_generated_top() {
        assert!(is_generated(b"// @generated\nfn x() {}\n"));
    }

    #[test]
    fn is_generated_go_do_not_edit() {
        assert!(is_generated(
            b"// Code generated by protoc. DO NOT EDIT.\npackage x\n",
        ));
    }

    #[test]
    fn is_generated_lizard_marker() {
        assert!(is_generated(b"# GENERATED CODE\nprint('x')\n"));
    }

    #[test]
    fn is_generated_python_do_not_edit() {
        assert!(is_generated(b"# DO NOT EDIT\nprint('x')\n"));
    }

    #[test]
    fn is_generated_case_insensitive_marker() {
        assert!(is_generated(b"// @GENERATED\nfn x() {}\n"));
    }

    #[test]
    fn is_generated_marker_only_in_body_is_false() {
        // Marker phrase appearing well past the scan window must not trigger.
        let mut buf = Vec::with_capacity(8 * 1024);
        for i in 0..200 {
            buf.extend_from_slice(format!("// line {i}\n").as_bytes());
        }
        buf.extend_from_slice(b"// @generated  -- but this is line 200+\n");
        assert!(!is_generated(&buf));
    }

    #[test]
    fn is_generated_empty_file_is_false() {
        assert!(!is_generated(b""));
    }

    #[test]
    fn is_generated_non_utf8_does_not_panic() {
        // Non-UTF-8 garbage with no ASCII-marker substring: every byte is
        // 0x80..=0xFF (continuation / invalid in UTF-8 lead positions), so
        // it cannot contain `@generated`, `DO NOT EDIT`, or `GENERATED CODE`
        // as a byte sequence. Verifies both no-panic and the negative
        // result.
        let buf: Vec<u8> = (0x80u8..=0xFFu8).cycle().take(2048).collect();
        assert!(!is_generated(&buf));
    }

    #[test]
    fn is_generated_short_file_with_marker() {
        // File smaller than the scan window with a marker on the first line.
        assert!(is_generated(b"# @generated"));
    }

    #[test]
    fn is_generated_utf8_bom_then_marker() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\xEF\xBB\xBF");
        buf.extend_from_slice(b"// @generated\nfn x() {}\n");
        assert!(is_generated(&buf));
    }

    #[test]
    fn is_generated_no_marker_returns_false() {
        assert!(!is_generated(
            b"// Hand-written file.\nfn main() { println!(\"hi\"); }\n"
        ));
    }

    #[test]
    fn normalize_line_endings_mixed_endings() {
        // LF + lone-CR + CRLF in one buffer — each is converted independently.
        let mut d = b"a\nb\rc\r\nd".to_vec();
        normalize_line_endings(&mut d);
        assert_eq!(d, b"a\nb\nc\nd\n");
    }
}
