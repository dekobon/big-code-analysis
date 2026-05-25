// Sibling-file unit tests for `src/tools.rs`, wired in via `#[path =
// "tools_tests.rs"] mod tests;`. The `./**/*_tests.rs` rule in
// `.bcaignore` keeps this file out of the self-scan walker so the
// production-file metric caps stay tight even as the test suite
// grows.

#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

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

/// Regression for issue #297: `#include "../foo.h"` from
/// `src/lib/file.c` must resolve to `src/foo.h`, not the
/// same-directory `src/lib/foo.h` that the prior lexical
/// `normalize_path` collapse left as the closest match.
#[test]
fn guess_file_parent_dir_include_resolves_to_sibling() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/foo.h"),
            PathBuf::from("/proj/src/lib/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/lib/file.c");
    let result = guess_file(current, "../foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/foo.h")]);
}

/// `../inc/foo.h` from `src/lib/file.c` must resolve to
/// `src/inc/foo.h`, not some other `inc/foo.h` deeper in the
/// tree.
#[test]
fn guess_file_parent_subdir_include_resolves_to_correct_inc() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/inc/foo.h"),
            PathBuf::from("/proj/src/lib/inc/foo.h"),
            PathBuf::from("/proj/other/inc/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/lib/file.c");
    let result = guess_file(current, "../inc/foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/inc/foo.h")]);
}

/// A plain `foo.h` include from `src/lib/file.c` must keep the
/// existing same-directory preference and resolve to
/// `src/lib/foo.h`.
#[test]
fn guess_file_plain_include_keeps_same_directory_preference() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/foo.h"),
            PathBuf::from("/proj/src/lib/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/lib/file.c");
    let result = guess_file(current, "foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/lib/foo.h")]);
}

/// A `./foo.h` include from `src/lib/file.c` must still resolve
/// to the same-directory `src/lib/foo.h` (CurDir segments are
/// collapsed before joining).
#[test]
fn guess_file_curdir_include_resolves_to_same_directory() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/foo.h"),
            PathBuf::from("/proj/src/lib/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/lib/file.c");
    let result = guess_file(current, "./foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/lib/foo.h")]);
}

/// `../../foo.h` from `src/a/b/file.c` must resolve up two
/// levels to `src/foo.h`, not be lexically collapsed.
#[test]
fn guess_file_double_parent_include_resolves_two_levels_up() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/foo.h"),
            PathBuf::from("/proj/src/a/foo.h"),
            PathBuf::from("/proj/src/a/b/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/a/b/file.c");
    let result = guess_file(current, "../../foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/foo.h")]);
}

/// When the relative target does not match any candidate
/// exactly, the existing basename / same-directory / distance
/// fallback chain still applies. With a single candidate, that
/// candidate is returned even if its path differs from the
/// resolved target.
#[test]
fn guess_file_unique_basename_returns_only_candidate() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![PathBuf::from("/proj/src/lib/foo.h")],
    );
    let current = Path::new("/proj/src/lib/file.c");
    // Resolved target would be `/proj/foo.h`, which does not
    // exist; the unique-basename short-circuit still wins.
    let result = guess_file(current, "../../foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/lib/foo.h")]);
}

/// The `mozilla/` prefix strip must still apply, so
/// `#include "mozilla/foo.h"` from `src/lib/file.c` resolves
/// the same way a bare `foo.h` would.
#[test]
fn guess_file_mozilla_prefix_is_stripped_before_resolution() {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    all_files.insert(
        "foo.h".to_string(),
        vec![
            PathBuf::from("/proj/src/foo.h"),
            PathBuf::from("/proj/src/lib/foo.h"),
        ],
    );
    let current = Path::new("/proj/src/lib/file.c");
    let result = guess_file(current, "mozilla/foo.h", &all_files);
    assert_eq!(result, vec![PathBuf::from("/proj/src/lib/foo.h")]);
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
    // `ocaml` is a real interpreter the project does not target —
    // a stable sentinel for the "shebang names an interpreter
    // outside the supported set" case (independent of which
    // languages the workspace happens to recognise today).
    assert_eq!(get_shebang_lang(b"#!/usr/bin/ocaml\n"), None);
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
fn guess_language_shebang_detects_ruby_without_extension() {
    // Gem executables under `bin/` are extensionless Ruby scripts
    // identified solely by their `#!/usr/bin/env ruby` shebang.
    let buf = b"#!/usr/bin/env ruby\nputs 'hi'\n";
    assert_eq!(guess_language(buf, "run"), (Some(LANG::Ruby), "ruby"));
}

#[test]
fn guess_language_shebang_detects_elixir_without_extension() {
    // Extensionless Elixir scripts (`#!/usr/bin/env elixir`) must be
    // identified by their shebang alone — regression for #186.
    let buf = b"#!/usr/bin/env elixir\nIO.puts(\"hi\")\n";
    assert_eq!(guess_language(buf, "run"), (Some(LANG::Elixir), "elixir"));
}

#[test]
fn guess_language_shebang_detects_iex_without_extension() {
    // `iex` is Elixir's interactive shell; scripts that drive it via
    // `#!/usr/bin/env iex` should also map to Elixir.
    let buf = b"#!/usr/bin/env iex\nIO.puts(\"hi\")\n";
    assert_eq!(guess_language(buf, "run"), (Some(LANG::Elixir), "elixir"));
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

// ── guess_file strategy-chain helpers ──────────────────────────────

fn pb(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn unique_filter_returns_some_when_exactly_one_match() {
    let possibilities = vec![pb("src/a.h"), pb("src/b.h"), pb("src/c.h")];
    let current = pb("src/lib.c");
    let got = unique_filter(&possibilities, &current, |p| p.ends_with("b.h"));
    assert_eq!(got, Some(vec![pb("src/b.h")]));
}

#[test]
fn unique_filter_returns_none_when_zero_match() {
    let possibilities = vec![pb("src/a.h"), pb("src/b.h")];
    let current = pb("src/lib.c");
    let got = unique_filter(&possibilities, &current, |p| p.ends_with("xyz.h"));
    assert_eq!(got, None);
}

#[test]
fn unique_filter_returns_none_when_multiple_match() {
    let possibilities = vec![pb("a/foo.h"), pb("b/foo.h"), pb("c/bar.h")];
    let current = pb("lib.c");
    let got = unique_filter(&possibilities, &current, |p| p.ends_with("foo.h"));
    assert_eq!(got, None);
}

#[test]
fn unique_filter_excludes_current_path_from_matches() {
    let current = pb("src/lib.c");
    let possibilities = vec![current.clone(), pb("src/other.c")];
    // `lib.c` would match `ends_with("lib.c")` but is current_path, so the
    // surviving candidate is the unique `other.c` — wait, the predicate is
    // `ends_with("lib.c")` and `other.c` doesn't match. Pin the contract:
    // current_path is filtered out BEFORE the predicate counts toward
    // uniqueness, so a self-match becomes "zero candidates" not "one".
    let got = unique_filter(&possibilities, &current, |p| p.ends_with("lib.c"));
    assert_eq!(got, None);
}

#[test]
fn resolve_against_resolved_prefers_exact_over_suffix() {
    // Two candidates, both end_with the resolved target; the exact match
    // wins because the strategy tries exact first.
    let possibilities = vec![pb("a/b/foo.h"), pb("foo.h")];
    let current = pb("a/b/main.c");
    let resolved = pb("foo.h");
    let got = resolve_against_resolved(&possibilities, &current, Some(&resolved));
    // Only one candidate (`foo.h`) matches `== resolved`, so it wins.
    assert_eq!(got, Some(vec![pb("foo.h")]));
}

#[test]
fn resolve_against_resolved_returns_none_without_resolved_path() {
    let possibilities = vec![pb("foo.h")];
    let current = pb("main.c");
    assert_eq!(
        resolve_against_resolved(&possibilities, &current, None),
        None
    );
}

#[test]
fn resolve_against_parent_keeps_only_siblings() {
    let possibilities = vec![pb("src/a/foo.h"), pb("src/b/foo.h")];
    let current = pb("src/a/main.c");
    let got = resolve_against_parent(&possibilities, &current);
    // Only `src/a/foo.h` starts with `src/a`, so unique.
    assert_eq!(got, Some(vec![pb("src/a/foo.h")]));
}

#[test]
fn resolve_against_parent_returns_none_when_no_parent() {
    let possibilities = vec![pb("foo.h")];
    let current = pb("main.c");
    // `current.parent()` is `Some("")`, and `pb("foo.h").starts_with("")`
    // is true, so the unique-match candidate survives. Document the
    // empty-parent case: same-directory unqualified files DO match.
    let got = resolve_against_parent(&possibilities, &current);
    assert_eq!(got, Some(vec![pb("foo.h")]));
}

#[test]
fn min_distance_candidates_empty_returns_empty() {
    let possibilities: Vec<PathBuf> = vec![];
    let current = pb("src/main.c");
    assert!(min_distance_candidates(&possibilities, &current).is_empty());
}

#[test]
fn min_distance_candidates_single_returns_single() {
    let possibilities = vec![pb("src/foo.h")];
    let current = pb("src/main.c");
    let got = min_distance_candidates(&possibilities, &current);
    assert_eq!(got, vec![pb("src/foo.h")]);
}

#[test]
fn min_distance_candidates_excludes_current_path() {
    let current = pb("src/main.c");
    // Self-match: only candidate IS current; result must be empty, not
    // [current]. This is the invariant that protects guess_file from
    // emitting `#include "main.c"` resolving to itself.
    let possibilities = vec![current.clone()];
    assert!(min_distance_candidates(&possibilities, &current).is_empty());
}

#[test]
fn min_distance_candidates_returns_all_ties_at_minimum() {
    // Two candidates at distance 1 (siblings of `src/main.c`), one at
    // distance > 1. Both ties survive; the farther one is dropped.
    let possibilities = vec![pb("src/a.h"), pb("src/b.h"), pb("far/c.h")];
    let current = pb("src/main.c");
    let mut got = min_distance_candidates(&possibilities, &current);
    got.sort(); // get_paths_dist preserves walk order; sort for stable assertion
    assert_eq!(got, vec![pb("src/a.h"), pb("src/b.h")]);
}

#[test]
fn min_distance_candidates_strictly_decreasing_distances() {
    // Pin the `Ordering::Less` arm: each candidate beats the prior min,
    // so the prior survivor set is cleared and replaced. Only the last
    // (closest) candidate should remain. This is the pathological case
    // for the prior `Vec<PathBuf>` implementation that allocated +
    // dropped on every Less arm.
    let possibilities = vec![pb("a/b/c/d/x.h"), pb("a/b/c/x.h"), pb("a/b/x.h")];
    let current = pb("a/b/main.c");
    let got = min_distance_candidates(&possibilities, &current);
    assert_eq!(got, vec![pb("a/b/x.h")]);
}
