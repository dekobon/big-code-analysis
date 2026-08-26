// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::bytes::Regex;
use termcolor::{Color, ColorSpec, WriteColor};

use crate::langs::*;

/// Reads a file, normalising all CR-only and CRLF line endings to LF.
///
/// **Note for downstream consumers**: the returned buffer never contains `\r`
/// bytes. Callers that previously observed raw `\r\n` sequences will see plain
/// `\n` after this call. This is intentional — the metric engine requires LF-
/// only input — but it is a behavioural difference from a plain `fs::read`.
///
/// # Errors
///
/// Returns any [`std::io::Error`] surfaced by [`File::open`] (the
/// path is missing, lacks read permission, is a directory, …) or by
/// [`File::read_to_end`] while reading the file contents.
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

/// Bytes from the start of the file probed to decide whether the contents
/// look like UTF-8 before the whole file is read. A small fixed window keeps
/// the rejection of obviously-binary files cheap; the last character of the
/// window may be a multibyte sequence split by this boundary, which the
/// classifier tolerates only when more file follows (see `read_file_with_eol`).
const UTF8_PROBE_BYTES: usize = 64;

/// Decides whether a file's probe prefix is decodable UTF-8 and where its
/// real content starts. Returns the post-BOM content slice when the probe
/// is acceptable, or the [`SkipReason`] that rejects the file.
///
/// A UTF-16 BE/LE BOM marks a file whose body is interleaved-NUL UTF-16,
/// which the metric engine cannot parse: stripping the BOM and continuing
/// would let the ASCII-dominant body pass the UTF-8 probe (each NUL is a
/// valid single-byte UTF-8 scalar) and reach the parser as garbage (issue
/// #803). Skip such files, mirroring `is_generated`'s documented stance
/// that UTF-16 source is unsupported. A UTF-8 BOM, by contrast, prefixes
/// genuine UTF-8 and is stripped so the body parses normally.
/// `starts_with` is bounds-safe for a probe shorter than the BOM.
///
/// Validation is at the byte level rather than via a lossy string
/// round-trip. The probe is only the first `UTF8_PROBE_BYTES`, so a file
/// longer than the probe may legitimately have its last multibyte
/// character split across the window boundary. `String::from_utf8_lossy`
/// could not distinguish that benign truncation (issue #746) from a real
/// encoding error, and its replacement character `U+FFFD` collided with
/// the same scalar appearing legitimately in the source (issue #758).
/// `probe_truncated` is true only when the file continues past the probe;
/// when the probe is the whole file there is no more data to complete a
/// trailing partial sequence, so such a sequence is genuine corruption.
fn probe_decodable_prefix(
    start: &[u8],
    file_size: usize,
    probe_len: usize,
) -> Result<&[u8], SkipReason> {
    let start = if start.starts_with(b"\xFE\xFF") || start.starts_with(b"\xFF\xFE") {
        return Err(SkipReason::Utf16Bom);
    } else if let Some(rest) = start.strip_prefix(b"\xEF\xBB\xBF") {
        rest
    } else {
        start
    };

    let probe_truncated = file_size > probe_len;
    match std::str::from_utf8(start) {
        Ok(_) => {}
        // Only a trailing incomplete multibyte sequence: the bytes before
        // `valid_up_to()` are valid UTF-8 and the truncated tail is
        // completed by data later in the file.
        Err(e) if e.error_len().is_none() && probe_truncated => {}
        Err(_) => return Err(SkipReason::NotUtf8),
    }
    Ok(start)
}

/// Why [`read_file_with_eol_classified`] declined to hand a file's bytes to
/// the metric engine.
///
/// [`read_file_with_eol`] collapses every one of these into `Ok(None)`, which
/// left front-ends unable to name the real cause — `bca` reported a
/// multi-kilobyte binary and a one-byte source alike as "empty" (#1287). This
/// enum is the classified form of that same decision; the variants map 1:1 onto
/// the skip branches of [`read_file_with_eol_classified`].
///
/// Marked `#[non_exhaustive]`: a future gate (a size ceiling, a further
/// encoding) adds a variant additively, so match on it with a wildcard arm or
/// render it through [`Display`](std::fmt::Display).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// The file is zero bytes long.
    Empty,
    /// The file holds 1–3 bytes — too little to be worth parsing — or it
    /// shrank below the UTF-8 probe window between the `stat` and the read.
    TooSmall,
    /// The file opens with a UTF-16 BE or LE byte-order mark. Its
    /// interleaved-NUL body would pass a byte-level UTF-8 check and reach the
    /// parser as garbage, so it is skipped rather than decoded (#803).
    Utf16Bom,
    /// The file's leading window is not valid UTF-8 — the shape a binary file
    /// takes here.
    NotUtf8,
}

impl SkipReason {
    /// Which small-file reason applies to a file of `size` bytes: zero bytes
    /// is [`Empty`](Self::Empty), anything else under the gate is
    /// [`TooSmall`](Self::TooSmall).
    fn for_size(size: usize) -> Self {
        if size == 0 {
            Self::Empty
        } else {
            Self::TooSmall
        }
    }
}

impl std::fmt::Display for SkipReason {
    /// Renders the reason as a noun phrase naming the skipped file, so a
    /// caller can splice it into a diagnostic: `skipping {reason}: {path}`.
    ///
    /// Per the project diagnostic convention the phrase carries no severity
    /// word — the presenting layer owns that prefix.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let phrase = match self {
            Self::Empty => "empty file",
            Self::TooSmall => "file too small to analyze (3 bytes or fewer)",
            Self::Utf16Bom => "UTF-16 file (unsupported encoding)",
            Self::NotUtf8 => "file with non-UTF-8 contents",
        };
        f.write_str(phrase)
    }
}

/// Reads a file, normalising all CR-only and CRLF line endings to LF, and ensures
/// the buffer ends with exactly one `\n`. Returns `None` for readable files
/// ≤ 3 bytes or files that appear to be non-UTF-8.
///
/// `None` names no reason. Use [`read_file_with_eol_classified`] — of which
/// this function is a thin projection — when the caller reports the skip to a
/// user, so an empty file, a one-byte source, a binary blob and a UTF-16 file
/// are not all announced as "empty" (#1287).
///
/// # Errors
///
/// Returns any [`std::io::Error`] surfaced by [`File::open`] (the
/// path is missing, lacks read permission, is a directory, …) or by
/// the subsequent reads from the open file handle. A clean short read
/// during the probe (`UnexpectedEof`) yields `Ok(None)`; any other
/// `read_exact` error kind propagates as `Err`. A non-UTF-8 head, a
/// too-small file, or a UTF-16 BE/LE BOM is reported via `Ok(None)`,
/// not an error — but "too small" is decided only for a file that can
/// be opened, so an unreadable one errors instead (#1060).
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
    read_file_with_eol_classified(path).map(Result::ok)
}

/// Reads a file exactly as [`read_file_with_eol`] does, but names the reason
/// when the file is skipped instead of collapsing every skip into `None`.
///
/// The outer `Result` is the I/O outcome; the inner one is the analysis
/// outcome — `Ok(bytes)` for a file the metric engine can parse, or
/// `Err(`[`SkipReason`]`)` for one it declines. [`read_file_with_eol`] is
/// `read_file_with_eol_classified(path).map(Result::ok)`, so the two agree
/// branch for branch.
///
/// # Errors
///
/// Identical to [`read_file_with_eol`]: any [`std::io::Error`] from
/// [`File::open`] or the subsequent reads propagates. Skips are *not* errors —
/// they arrive as `Ok(Err(reason))`.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use big_code_analysis::{SkipReason, read_file_with_eol_classified};
///
/// let dir = tempfile::tempdir().unwrap();
/// let path = dir.path().join("tiny.rs");
/// std::fs::write(&path, b"x").unwrap();
///
/// assert_eq!(
///     read_file_with_eol_classified(&path).unwrap(),
///     Err(SkipReason::TooSmall)
/// );
/// ```
pub fn read_file_with_eol_classified(path: &Path) -> std::io::Result<Result<Vec<u8>, SkipReason>> {
    match read_gated(path) {
        Ok(data) => Ok(Ok(data)),
        Err(ReadStop::Skip(reason)) => Ok(Err(reason)),
        Err(ReadStop::Io(err)) => Err(err),
    }
}

/// The two ways [`read_gated`] stops short of returning bytes. Collapsing
/// them into one `Err` is what lets that function spell every gate as `?`
/// instead of hand-wrapping each early return in the public
/// `Ok(Err(reason))` / `Err(err)` shape; the split back out happens once, in
/// [`read_file_with_eol_classified`].
enum ReadStop {
    /// A genuine I/O failure, propagated to the caller as `Err`.
    Io(std::io::Error),
    /// The file was read far enough to decide the metric engine should not
    /// see it. Not an error (issue #804).
    Skip(SkipReason),
}

impl From<std::io::Error> for ReadStop {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<SkipReason> for ReadStop {
    fn from(reason: SkipReason) -> Self {
        Self::Skip(reason)
    }
}

/// Applies the pre-parse gates in order — size, readability, UTF-8 probe —
/// and returns the EOL-normalised contents of a file that clears them all.
fn read_gated(path: &Path) -> Result<Vec<u8>, ReadStop> {
    let meta = fs::metadata(path);
    let file_size = meta.as_ref().map_or(1024 * 1024, |m| m.len() as usize);
    if file_size <= 3 {
        // Nothing worth parsing this small — but `stat` alone must not
        // decide it: `stat` succeeds on a file the process cannot open,
        // so an unreadable tiny file read as empty and `bca check`
        // exited 0 on a tree it never read (#1060). `meta` is
        // necessarily `Ok` here; the open is a discarded readability
        // probe, skipped for non-regular files because a FIFO blocks.
        if meta.is_ok_and(|m| m.is_file()) {
            File::open(path)?;
        }
        return Err(SkipReason::for_size(file_size).into());
    }

    let mut file = File::open(path)?;

    let probe_len = UTF8_PROBE_BYTES.min(file_size);
    let mut start = vec![0; probe_len];
    // A clean short read (the file shrank below the probe between the
    // `metadata` call and here) is reported as a skip, matching the
    // too-small-file case. Any other `read_exact` failure — a real I/O
    // fault such as a permission or hardware error — must propagate as
    // `Err` per the documented contract (issue #804); collapsing every
    // error to a skip would silently swallow genuine read failures.
    match file.read_exact(&mut start) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(SkipReason::TooSmall.into());
        }
        Err(e) => return Err(e.into()),
    }

    // Sniff the probe: reject UTF-16 / corrupt files, strip a UTF-8 BOM,
    // and anchor the buffer at the post-BOM content. An `Err` names why
    // the file is skipped (see `probe_decodable_prefix`).
    let start = probe_decodable_prefix(&start, file_size, probe_len)?;

    let mut data = Vec::with_capacity(file_size + 2);
    data.extend_from_slice(start);

    file.read_to_end(&mut data)?;

    normalize_line_endings(&mut data);

    Ok(data)
}

/// Normalises an in-memory source buffer to match the [`read_file_with_eol`]
/// on-disk path: all CR-only and CRLF line endings become LF, and the buffer is
/// guaranteed to end with exactly one `\n`.
///
/// In-memory entry points (the web server's JSON/octet-stream payloads, the
/// Python `analyze_source` bindings) feed caller-supplied bytes straight to the
/// parser, whereas the CLI reads files through [`read_file_with_eol`]. Without
/// this step, identical content yields different metrics across surfaces — an
/// editor buffer with no trailing newline reports `sloc: 0` over the wire but
/// `sloc: 1` from the CLI on the same bytes (issue #640). Run the buffer through
/// this helper before parsing so every surface computes the canonical numbers.
///
/// Returns a fresh owned buffer; the input is consumed so the common case
/// (already-owned request body) reuses its allocation.
///
/// # Examples
///
/// ```
/// use big_code_analysis::normalize_eol;
///
/// // CRLF endings collapse to LF and a missing final newline is added.
/// assert_eq!(normalize_eol(b"a\r\nb".to_vec()), b"a\nb\n");
/// ```
#[must_use]
pub fn normalize_eol(mut data: Vec<u8>) -> Vec<u8> {
    normalize_line_endings(&mut data);
    data
}

/// Writes data to a file.
///
/// # Errors
///
/// Returns any [`std::io::Error`] surfaced by [`File::create`]
/// (parent directory missing, lacks write permission, target is a
/// directory, …) or by [`File::write_all`] while writing the buffer.
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
#[must_use]
pub fn get_language_for_file(path: &Path) -> Option<LANG> {
    let ext = path.extension()?.to_str()?;
    get_from_ext(&lowercase_ext(ext))
}

/// Borrows `ext` when it is already the ASCII-lowercase spelling
/// [`get_from_ext`]'s table is keyed on, allocating only for the
/// mixed-case minority (`foo.C`, `foo.PY`).
///
/// The `is_ascii` guard is load-bearing: [`str::to_lowercase`] folds
/// `U+212A KELVIN SIGN` onto ASCII `k`, so a non-ASCII extension must
/// keep that Unicode-aware path to leave the lookup key as it was before
/// #1111. Past the guard the input is ASCII, hence the cheaper fold.
fn lowercase_ext(ext: &str) -> Cow<'_, str> {
    if !ext.is_ascii() {
        Cow::Owned(ext.to_lowercase())
    } else if ext.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(ext.to_ascii_lowercase())
    } else {
        Cow::Borrowed(ext)
    }
}

fn mode_to_str(mode: &[u8]) -> Option<String> {
    std::str::from_utf8(mode).ok().map(str::to_lowercase)
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
///
/// # Panics
///
/// Panics if the embedded marker regex set fails to build; the marker
/// list is a static literal so this represents a compile-time bug, not
/// a runtime input that can be handled.
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
        .get_or_init(|| Regex::new(regex).expect("constant regex pattern must compile"))
        .captures(line)
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
        "ruby" => Some(LANG::Ruby),
        "elixir" | "iex" => Some(LANG::Elixir),
        _ => None,
    }
}

// Editors place mode/file-local-variable lines near the very top or
// very bottom of a file. Emacs honours the first non-shebang line and a
// trailing "Local Variables:" block; Vim honours modelines in the first
// or last few lines (`modelines` defaults to 5). Scanning this many real
// lines at each end covers both conventions without trawling the body.
const MODE_LINE_SCAN_WINDOW: usize = 5;

// Entries into `get_emacs_mode` — the only thing able to observe #1111's
// laziness: every assertion on what `guess_language` returns holds just as
// well when the scan runs eagerly and throws the result away. Read by the
// test that pins it, which carries why.
crate::observation::counter!(modeline_scans);

fn get_emacs_mode(buf: &[u8]) -> Option<String> {
    modeline_scans::record();
    // Forward scan: the first `MODE_LINE_SCAN_WINDOW` real lines may carry
    // an emacs `-*- … -*-` header or a Vim modeline. `split` yields one
    // element per line (no unbounded remainder), and `take` bounds the
    // window precisely — the former `splitn(5)` + `i == 3` break inspected
    // only 4 lines yet split off a 5th unbounded remainder (issue #709).
    for line in buf.split(|c| *c == b'\n').take(MODE_LINE_SCAN_WINDOW) {
        if let Some(cap) = get_regex(&RE1_EMACS, line, FIRST_EMACS_EXPRESSION) {
            return mode_to_str(&cap[1]);
        } else if let Some(cap) = get_regex(&RE2_EMACS, line, SECOND_EMACS_EXPRESSION) {
            return mode_to_str(&cap[1]);
        } else if let Some(cap) = get_regex(&RE1_VIM, line, VIM_EXPRESSION) {
            return mode_to_str(&cap[1]);
        }
    }

    // Backward scan for a trailing Vim modeline. Skip empty pieces so a
    // trailing newline (the common case after `read_file_with_eol`) and
    // any trailing blank lines do not consume the window before a real
    // modeline is reached — the former `rsplitn(5)` spent its first slot
    // on that empty piece, covering fewer than the intended real lines.
    for line in buf
        .rsplit(|c| *c == b'\n')
        .filter(|line| !line.is_empty())
        .take(MODE_LINE_SCAN_WINDOW)
    {
        if let Some(cap) = get_regex(&RE1_VIM, line, VIM_EXPRESSION) {
            return mode_to_str(&cap[1]);
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
pub fn guess_language<P: AsRef<Path>>(buf: &[u8], path: P) -> (Option<LANG>, &'static str) {
    // Precedence: extension, then emacs/vim modeline, then shebang. The
    // fallbacks are lazy, so a recognised extension never pays for the
    // modeline scan; the previous form ran it for every file and
    // discarded it, every arm of its extension branch having returned the
    // extension's language — the "modeline agrees" arm included (#1111).
    let lang = get_language_for_file(path.as_ref())
        .or_else(|| get_emacs_mode(buf).and_then(|mode| get_from_emacs_mode(&mode)))
        .or_else(|| get_shebang_lang(buf));

    lang.map_or((None, ""), |lang| (Some(lang), lang.name()))
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
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().copied() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            // A `Prefix` (Windows drive / UNC) component only ever
            // appears first; the leading peek+next above already
            // consumed it, so it cannot recur in this loop.
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
            // `ancestor` is yielded by `path1.ancestors()`, so it is
            // a prefix of `path1` by construction; `path2` was just
            // verified by `starts_with` above. Both `strip_prefix`
            // calls are therefore infallible.
            let path1 = path1
                .strip_prefix(ancestor)
                .expect("ancestor is by construction a prefix of path1");
            let path2 = path2
                .strip_prefix(ancestor)
                .expect("ancestor verified by starts_with above");
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
    let include_path = include_path
        .strip_prefix("mozilla/")
        .unwrap_or(include_path);

    // Resolve the include relative to the including file's parent
    // before normalizing. This preserves leading `..` traversal so
    // `#include "../foo.h"` from `src/lib/file.c` targets
    // `src/foo.h`, not the lexically-popped `foo.h` (issue #297).
    // Lexical-only normalization is required because `current_path`
    // and the entries in `all_files` are typically not canonicalized
    // and the included header need not exist on disk yet.
    let resolved_path = current_path
        .parent()
        .map(|parent| normalize_path(parent.join(include_path)));

    let include_path = normalize_path(include_path);
    let Some(file_name) = include_path.file_name().and_then(|n| n.to_str()) else {
        return vec![];
    };
    let Some(possibilities) = all_files.get(file_name) else {
        return vec![];
    };
    if possibilities.len() == 1 {
        return possibilities.clone();
    }

    // Strategy chain: each step looks for a UNIQUE candidate that
    // matches a progressively weaker signal (full resolved target →
    // suffix on the normalized include → siblings of the including
    // file). When no step yields a unique match, fall back to the
    // closest by path distance, which may return zero or many.
    resolve_against_resolved(possibilities, current_path, resolved_path.as_deref())
        .or_else(|| unique_filter(possibilities, current_path, |p| p.ends_with(&include_path)))
        .or_else(|| resolve_against_parent(possibilities, current_path))
        .unwrap_or_else(|| min_distance_candidates(possibilities, current_path))
}

/// Filter `possibilities` to those satisfying `pred` and distinct
/// from `current_path`, returning `Some(matched)` only when exactly
/// one survives. The cascading caller treats `None` as "this strategy
/// did not yield a unique resolution — try the next one."
fn unique_filter<F>(possibilities: &[PathBuf], current_path: &Path, pred: F) -> Option<Vec<PathBuf>>
where
    F: Fn(&PathBuf) -> bool,
{
    let matched: Vec<PathBuf> = possibilities
        .iter()
        .filter(|p| current_path != p.as_path() && pred(p))
        .cloned()
        .collect();
    (matched.len() == 1).then_some(matched)
}

/// Strongest signal: a candidate matches the fully resolved relative
/// target. Prefer exact equality, then suffix match (so absolute
/// `all_files` entries still match a relative resolved target like
/// `src/foo.h`).
fn resolve_against_resolved(
    possibilities: &[PathBuf],
    current_path: &Path,
    resolved: Option<&Path>,
) -> Option<Vec<PathBuf>> {
    let resolved = resolved?;
    unique_filter(possibilities, current_path, |p| p == resolved)
        .or_else(|| unique_filter(possibilities, current_path, |p| p.ends_with(resolved)))
}

/// Candidate-in-same-directory heuristic: keep entries whose path
/// starts with the including file's parent directory.
fn resolve_against_parent(possibilities: &[PathBuf], current_path: &Path) -> Option<Vec<PathBuf>> {
    let parent = current_path.parent()?;
    unique_filter(possibilities, current_path, |p| p.starts_with(parent))
}

/// Last-chance fallback in the `guess_file` strategy chain: returns
/// every candidate whose `get_paths_dist` from `current_path` ties
/// the minimum, or an empty `Vec` when no candidate has a defined
/// distance. Unlike the unique-match strategies, this may
/// legitimately return zero or many entries — its result is the
/// function's final answer, not a "try the next strategy" signal.
fn min_distance_candidates(possibilities: &[PathBuf], current_path: &Path) -> Vec<PathBuf> {
    // Hold survivors as borrows during the walk: `Less` arms clear the
    // prior set without dropping owned `PathBuf`s, and the trailing
    // `cloned()` runs exactly once per final survivor — never on
    // entries that were tentatively kept and later evicted.
    let mut dist_min = usize::MAX;
    let mut path_min: Vec<&PathBuf> = Vec::new();
    for p in possibilities {
        if current_path == p {
            continue;
        }
        let Some(dist) = get_paths_dist(current_path, p) else {
            continue;
        };
        match dist.cmp(&dist_min) {
            Ordering::Less => {
                dist_min = dist;
                path_min.clear();
                path_min.push(p);
            }
            Ordering::Equal => path_min.push(p),
            Ordering::Greater => {}
        }
    }
    path_min.into_iter().cloned().collect()
}

// Accept `&mut dyn WriteColor` rather than `&mut StandardStreamLock` so
// tests (e.g. `function::dump_spans`) can substitute `termcolor::NoColor`
// over a `Vec<u8>` to capture the rendered bytes. Production callers
// continue to pass `&mut StandardStreamLock`, which unsized-coerces to
// the trait object at the call site.
#[inline]
pub(crate) fn color(stdout: &mut dyn WriteColor, color: Color) -> std::io::Result<()> {
    stdout.set_color(ColorSpec::new().set_fg(Some(color)))
}

#[inline]
pub(crate) fn intense_color(stdout: &mut dyn WriteColor, color: Color) -> std::io::Result<()> {
    stdout.set_color(ColorSpec::new().set_fg(Some(color)).set_intense(true))
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
