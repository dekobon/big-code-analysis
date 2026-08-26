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
        // UTF-16 LE/BE BOM: the file is skipped, not stripped-and-parsed
        // (issue #803) — the interleaved-NUL body is not UTF-8 source.
        (b"\xFF\xFEabc".to_vec(), None),
        (b"\xFE\xFFabc".to_vec(), None),
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
    assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "cpp"));

    let buf = b"// -*- c++ -*-\n";
    assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "cpp"));

    let buf = b"// -*- foo: bar; bar-mode: c++; hello: world\n";
    assert_eq!(
        guess_language(buf, "foo.py"),
        (Some(LANG::Python), "python")
    );

    let buf = b"/* hello world */\n";
    assert_eq!(guess_language(buf, "foo.cpp"), (Some(LANG::Cpp), "cpp"));

    // Since #721 `.c` routes to the dedicated `LANG::C`. When the file
    // extension and an emacs/vim modeline disagree, `guess_language`
    // deliberately trusts the extension (see the "rely on extension"
    // branch), so a `.c` file with a `ft=c++` modeline still resolves to
    // C — the modeline does not override the extension.
    let buf = b"\n\n\n\n\n\n\n\n\n// vim: set ts=4 ft=c++\n\n\n";
    assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::C), "c"));

    let buf = b"\n\n\n\n\n\n\n\n\n\n\n\n";
    assert_eq!(guess_language(buf, "foo.txt"), (None, ""));

    // Objective-C (`.m`) gets its own `LANG::Objc` and reports `"objc"`
    // since #724. Objective-C++ (`.mm`) stays on the C++ grammar (the
    // ObjC grammar can't parse the C++ half of a `.mm` file) and reports
    // `"cpp"` natively — the former `fake`/`#540` override is gone.
    let buf = b"// -*- mode: objc -*-\n";
    assert_eq!(guess_language(buf, "foo.m"), (Some(LANG::Objc), "objc"));
    let buf = b"// -*- foo: bar; mode: Objective-C++; hello: world\n";
    assert_eq!(guess_language(buf, "foo.mm"), (Some(LANG::Cpp), "cpp"));
}

#[test]
fn guess_language_emacs_mode_on_fifth_line() {
    // Regression test for issue #709. The forward scan used `splitn(5)`
    // with an `i == 3` break, so it inspected only the first 4 lines while
    // splitting off an unbounded 5th remainder it never matched against —
    // a mode-line on the 5th line was silently missed. With four blank
    // leading lines, the emacs header sits on the 5th line and must now be
    // detected against an extension-free path so the mode drives the result.
    let buf = b"\n\n\n\n// -*- mode: c++ -*-\n";
    assert_eq!(guess_language(buf, "noext"), (Some(LANG::Cpp), "cpp"));

    // A mode-line on the 6th line is outside the window and must NOT match;
    // the off-by-one fix widens the window to exactly 5 real lines, not the
    // unbounded remainder the old `splitn` tail used to (accidentally) span.
    let buf = b"\n\n\n\n\n// -*- mode: c++ -*-\n";
    assert_eq!(guess_language(buf, "noext"), (None, ""));
}

#[test]
fn guess_language_vim_modeline_with_trailing_blank_lines() {
    // Regression test for issue #709. The backward scan used `rsplitn(5)`,
    // whose first slot is the empty piece after the file's trailing
    // newline; trailing blank lines then consumed the rest of the window
    // before a real modeline was reached. Filtering empty pieces lets the
    // scan cover five real trailing lines regardless of trailing blanks.
    //
    // The six leading body lines push the modeline past the forward
    // scan window (first five lines), so the match can only come from the
    // *backward* scan — the path the fix changed. The three trailing
    // blank lines then fill the old `rsplitn(5)` window's first slots
    // with empty pieces, so the unfiltered scan broke before reaching the
    // modeline. This buffer therefore fails against the pre-fix
    // `rsplitn(5)` and passes only with the empty-piece filter.
    let buf = b"a\nb\nc\nd\ne\nf\n// vim: set ft=c++\n\n\n\n";
    assert_eq!(guess_language(buf, "noext"), (Some(LANG::Cpp), "cpp"));
}

#[test]
fn guess_language_name_outlives_input_buffer() {
    // The returned name is `&'static str`: it must remain valid after
    // the owned input buffer is dropped. This pins the honest lifetime
    // (issue #506) — the name borrows from static tables / literals
    // (`name()`, `fake::get_true`), never from `buf`.
    let name: &'static str = {
        let buf: Vec<u8> = b"// -*- foo: bar; mode: Objective-C++; hello: world\n".to_vec();
        let (lang, name) = guess_language(&buf, "foo.mm");
        assert_eq!(lang, Some(LANG::Cpp));
        // `.mm` stays on `Cpp`; `name` is `LANG::Cpp.name()` directly
        // (the `fake` override was retired with #724) — a `&'static str`
        // from static tables / literals, never borrowed from `buf`
        // (issue #506).
        name
    };
    assert_eq!(name, "cpp");
}

#[test]
fn guess_language_no_match_name_is_static_empty() {
    // The empty/no-match arm must also yield a `&'static str`.
    let name: &'static str = {
        let buf: Vec<u8> = b"\n\n\n\n".to_vec();
        let (lang, name) = guess_language(&buf, "foo.txt");
        assert_eq!(lang, None);
        name
    };
    assert_eq!(name, "");
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

/// Regression for #1111: the modeline scan is now computed lazily, only
/// when the extension fails to resolve. That is a pure computation change,
/// so these pin the *result* of every extension/modeline combination the
/// short-circuit could have perturbed — agreement, disagreement, an
/// unrecognised modeline, and an unrecognised extension that must still
/// fall through to the modeline.
#[test]
fn guess_language_extension_beats_disagreeing_modeline() {
    // Disagreement: the extension wins, and the modeline's language must
    // not leak into the result even though it is no longer consulted.
    let buf = b"// -*- mode: python -*-\nint a = 42;\n";
    assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::C), "c"));

    // Same via a trailing Vim modeline, which the backward scan would
    // otherwise reach.
    let buf = b"int a = 42;\n// vim: set ft=python\n";
    assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::C), "c"));
}

#[test]
fn guess_language_extension_agrees_with_modeline() {
    // Agreement: the pre-#1111 code returned the *modeline's* language
    // here, which is the same value by construction. Pin the equivalence.
    let buf = b"// -*- mode: python -*-\nx = 1\n";
    assert_eq!(
        guess_language(buf, "foo.py"),
        (Some(LANG::Python), "python")
    );
}

#[test]
fn guess_language_unknown_extension_falls_back_to_modeline() {
    // An extension that resolves to nothing must still reach the modeline
    // scan — the short-circuit only skips it when the extension resolved.
    let buf = b"# -*- mode: python -*-\nx = 1\n";
    assert_eq!(
        guess_language(buf, "foo.txt"),
        (Some(LANG::Python), "python")
    );
}

#[test]
fn guess_language_unrecognised_modeline_falls_through() {
    // A modeline naming a language we do not support leaves the extension
    // in charge, as before. Note this case never consults the modeline at
    // all — `foo.c` resolves first and short-circuits — so it is the two
    // below that pin the unrecognised-modeline branch itself.
    let buf = b"// -*- mode: cobol -*-\nint a = 42;\n";
    assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::C), "c"));

    // With no extension to resolve, an unrecognised modeline must yield to
    // the shebang rather than end the search. `get_emacs_mode` does find
    // `cobol` here; it is `get_from_emacs_mode` that returns `None`, and
    // that `None` has to keep flowing. Perturbing the chain so an
    // unrecognised modeline resolves to *something* (`get_from_emacs_mode(
    // &mode).or(Some(LANG::Rust))`) failed zero tests across the whole
    // workspace before this assertion existed.
    let buf = b"#!/usr/bin/env python3\n// -*- mode: cobol -*-\nx = 1\n";
    assert_eq!(guess_language(buf, "run"), (Some(LANG::Python), "python"));

    // …and with nothing left to fall through to, the answer is "unknown",
    // not the unrecognised mode name.
    let buf = b"// -*- mode: cobol -*-\nint a = 42;\n";
    assert_eq!(guess_language(buf, "foo.txt"), (None, ""));
}

/// The guard for #1111's laziness rather than for its results.
///
/// Every other assertion in this file pins what `guess_language`
/// *returns*, and all of them hold just as well if the modeline scan is
/// computed eagerly and thrown away — which is precisely the work #1111
/// removed. Only a scan count sees the difference.
///
/// Runs on its own thread so the count starts from zero regardless of
/// which other tests shared this one.
#[test]
fn a_resolving_extension_never_runs_the_modeline_scan() {
    std::thread::spawn(|| {
        assert_eq!(
            modeline_scans::observed(),
            0,
            "a fresh thread must not have scanned yet"
        );

        // A disagreeing modeline is present precisely so that an eager
        // implementation would have something to find; the extension
        // still wins, and the scan must not happen at all.
        let buf = b"// -*- mode: python -*-\nint a = 42;\n";
        for _ in 0..4 {
            assert_eq!(guess_language(buf, "foo.c"), (Some(LANG::C), "c"));
        }
        assert_eq!(
            modeline_scans::observed(),
            0,
            "a recognised extension must short-circuit before the scan"
        );

        // The counter is not stuck at zero: an unresolved extension does
        // reach the scan, once per call.
        assert_eq!(
            guess_language(buf, "foo.unknownext"),
            (Some(LANG::Python), "python")
        );
        assert_eq!(
            modeline_scans::observed(),
            1,
            "an unresolved extension must fall through to the scan"
        );
    })
    .join()
    .expect("detection thread must not panic");
}

#[test]
fn guess_language_uppercase_extension_matches_lowercase_table() {
    // #1111 replaced an unconditional `to_lowercase` with a borrow in the
    // already-lowercase case. Mixed and upper case extensions must keep
    // resolving exactly as they did.
    assert_eq!(
        guess_language(b"x = 1\n", "foo.PY"),
        (Some(LANG::Python), "python")
    );
    assert_eq!(
        guess_language(b"x = 1\n", "foo.Py"),
        (Some(LANG::Python), "python")
    );
    // `.C` folds to `.c`, i.e. `LANG::C` — the long-standing behaviour of
    // this lookup, not the Unix convention that upper-case `.C` is C++.
    assert_eq!(guess_language(b"int a;\n", "foo.C"), (Some(LANG::C), "c"));
    assert_eq!(get_language_for_file(Path::new("foo.C")), Some(LANG::C));
    assert_eq!(get_language_for_file(Path::new("foo.RS")), Some(LANG::Rust));
}

#[test]
fn guess_language_empty_buffer_and_no_extension() {
    // No extension, no modeline, no shebang, nothing to read.
    assert_eq!(guess_language(b"", "run"), (None, ""));
    // A bare dotfile has no extension at all (`Path::extension` is `None`).
    assert_eq!(guess_language(b"x = 1\n", ".bashrc"), (None, ""));
}

#[test]
fn lowercase_ext_borrows_only_when_already_ascii_lowercase() {
    // Asserting the `Cow` variant is the only coverage available for the
    // `is_ascii` guard: no language claims extension `k`, so the
    // `U+212A KELVIN SIGN` fold is unobservable through `guess_language`.
    // Rewriting these as `assert_eq!(lowercase_ext(…), "…")` would compile,
    // pass, and silently stop testing the borrow/own split.

    // Already lowercase: no allocation.
    assert!(matches!(lowercase_ext("rs"), Cow::Borrowed("rs")));
    assert!(matches!(lowercase_ext("php7"), Cow::Borrowed("php7")));
    assert!(matches!(lowercase_ext(""), Cow::Borrowed("")));

    // Uppercase ASCII: folded through an owned buffer. Matching on the
    // variant, not just the contents, is what distinguishes the two paths.
    assert!(matches!(lowercase_ext("RS"), Cow::Owned(s) if s == "rs"));
    assert!(matches!(lowercase_ext("Rs"), Cow::Owned(s) if s == "rs"));

    // Non-ASCII must take the owned path even with no ASCII uppercase
    // byte: `to_lowercase` folds `U+212A KELVIN SIGN` onto ASCII `k`, so
    // borrowing it would change the lookup key.
    assert!(matches!(lowercase_ext("\u{212a}"), Cow::Owned(s) if s == "k"));
    // A non-ASCII character that lowercases to itself is still owned — the
    // guard keys on `is_ascii`, not on whether the fold changed anything.
    assert!(matches!(lowercase_ext("é"), Cow::Owned(s) if s == "é"));
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
fn normalize_eol_adds_missing_trailing_newline() {
    // The issue #640 reproducer: `fn f(){}` with no final newline must gain
    // exactly one `\n` so in-memory callers match the CLI's `read_file_with_eol`.
    assert_eq!(super::normalize_eol(b"fn f(){}".to_vec()), b"fn f(){}\n");
}

#[test]
fn normalize_eol_collapses_crlf_and_terminates() {
    // CRLF endings collapse to LF and a single trailing newline is guaranteed,
    // matching the on-disk normalisation.
    assert_eq!(super::normalize_eol(b"a\r\nb".to_vec()), b"a\nb\n");
}

#[test]
fn normalize_eol_collapses_lone_cr() {
    assert_eq!(super::normalize_eol(b"a\rb\r".to_vec()), b"a\nb\n");
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

// Probe-window UTF-8 validation (issues #746 and #758). These cases need
// files longer than the 64-byte probe so the boundary behaviour is actually
// exercised; the inline `test_read` table above covers only sub-probe sizes.
//
// Each test writes a uniquely named temp file so the suite stays safe under
// the default parallel test runner.
fn write_tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(name);
    write_file(&path, bytes).unwrap();
    path
}

#[test]
fn read_eol_accepts_multibyte_split_at_probe_boundary() {
    // #746 (acceptance side): a valid UTF-8 file whose first 64 bytes end in
    // the middle of a multibyte character must not be rejected. 63 ASCII bytes
    // push the two-byte `é` (0xC3 0xA9) so its lead byte lands at offset 63 and
    // its continuation byte lands at offset 64 — outside the probe window. The
    // tail completing the sequence proves the classifier validates the prefix
    // up to `valid_up_to()` rather than rejecting on the split.
    let mut bytes = vec![b'a'; 63];
    bytes.extend_from_slice("é tail content past the probe window\n".as_bytes());
    let path = write_tmp("bca_read_eol_split_multibyte", &bytes);

    let got = read_file_with_eol(&path).unwrap();
    let mut expected = vec![b'a'; 63];
    expected.extend_from_slice("é tail content past the probe window\n".as_bytes());
    assert_eq!(got, Some(expected));
}

#[test]
fn read_eol_rejects_invalid_byte_hidden_by_old_pop() {
    // #746 (rejection side): the old classifier `from_utf8_lossy(...).pop()`
    // unconditionally dropped the trailing replacement character, so a probe
    // whose only non-ASCII byte was an invalid lead byte at the very end
    // (here 0xFF at offset 63) was wrongly accepted — the pop hid the only
    // U+FFFD. A real 0xFF byte is never valid UTF-8 (`error_len` is `Some`),
    // so the byte-level classifier must reject regardless of more file
    // following. This case fails against the pre-fix code.
    let mut bytes = vec![b'a'; 63];
    bytes.push(0xFF);
    bytes.extend_from_slice(&[b'a'; 40]); // ensure file > probe window
    let path = write_tmp("bca_read_eol_invalid_byte_hidden", &bytes);

    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

#[test]
fn read_eol_accepts_replacement_char_in_probe() {
    // #758: U+FFFD is a legal Unicode scalar; a file that legitimately
    // contains it within the probe window must be accepted, not mistaken for
    // the lossy-decode sentinel the old classifier scanned for.
    let mut bytes = "prefix \u{FFFD} suffix".as_bytes().to_vec();
    // Pad past the probe so the file is longer than the 64-byte window.
    bytes.extend_from_slice(&[b'z'; 80]);
    bytes.push(b'\n');
    let path = write_tmp("bca_read_eol_replacement_char", &bytes);

    let got = read_file_with_eol(&path).unwrap();
    assert_eq!(got, Some(bytes));
}

#[test]
fn read_eol_rejects_stray_invalid_byte_in_probe() {
    // A genuinely invalid byte (0xFF is never a valid UTF-8 lead byte) inside
    // the probe is real corruption and must be rejected, even when more file
    // follows (so it cannot be confused with a truncated trailing sequence).
    let mut bytes = b"valid prefix \xFF more".to_vec();
    bytes.extend_from_slice(&[b'q'; 80]);
    bytes.push(b'\n');
    let path = write_tmp("bca_read_eol_stray_invalid_byte", &bytes);

    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

#[test]
fn read_eol_rejects_short_file_truncated_mid_multibyte() {
    // A short file (<= probe size) that ends in an incomplete multibyte
    // sequence is genuinely truncated — there is no further data to complete
    // it — so it must be rejected. `0xC3` is the lead byte of a two-byte
    // sequence with no continuation byte following.
    let path = write_tmp("bca_read_eol_short_truncated", b"abcd\xC3");
    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

#[test]
fn read_eol_accepts_plain_ascii_past_probe() {
    // Sanity: an ordinary ASCII file longer than the probe is accepted and
    // line-ending normalised.
    let bytes = b"the quick brown fox jumps over the lazy dog several times over\n".repeat(3);
    let path = write_tmp("bca_read_eol_plain_ascii", &bytes);
    assert_eq!(read_file_with_eol(&path).unwrap(), Some(bytes));
}

// Build a UTF-16 byte stream (without BOM) from an ASCII string by
// interleaving a NUL after each byte, in the requested endianness.
fn utf16_ascii_body(text: &str, little_endian: bool) -> Vec<u8> {
    text.bytes()
        .flat_map(|b| if little_endian { [b, 0x00] } else { [0x00, b] })
        .collect()
}

#[test]
fn read_eol_rejects_utf16_le_bom() {
    // #803: a UTF-16-LE file (FF FE BOM + interleaved-NUL ASCII body) must be
    // skipped, not stripped-and-parsed. Before the fix the BOM was dropped and
    // the body — every byte <= 0x7F, so valid single-byte UTF-8 — passed the
    // probe, feeding NUL-interleaved garbage to the parser. The body is padded
    // well past the 64-byte probe so the file is unambiguously not too-small.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(utf16_ascii_body(
        "int main() { return 0; } // padding to exceed the probe window\n",
        true,
    ));
    let path = write_tmp("bca_read_eol_utf16_le_bom", &bytes);
    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

#[test]
fn read_eol_rejects_utf16_be_bom() {
    // #803, big-endian sibling: FE FF BOM + NUL-then-byte body. Same rejection.
    let mut bytes = vec![0xFE, 0xFF];
    bytes.extend(utf16_ascii_body(
        "int main() { return 0; } // padding to exceed the probe window\n",
        false,
    ));
    let path = write_tmp("bca_read_eol_utf16_be_bom", &bytes);
    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

#[test]
fn read_eol_strips_utf8_bom_and_keeps_body() {
    // #803 (acceptance side): a UTF-8 BOM prefixes genuine UTF-8, so it is
    // stripped and the body is read normally — UTF-8 BOM handling must NOT
    // regress when UTF-16 BOMs start being rejected.
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"abc");
    let path = write_tmp("bca_read_eol_utf8_bom_body", &bytes);
    assert_eq!(read_file_with_eol(&path).unwrap(), Some(b"abc\n".to_vec()));
}

#[cfg(unix)]
#[test]
fn read_eol_propagates_non_eof_io_error() {
    // #804: a real I/O fault during the probe read must propagate as `Err`,
    // not collapse to `Ok(None)`. On Unix, opening a directory succeeds but
    // `read_exact` fails with a non-`UnexpectedEof` error (`IsADirectory`),
    // exercising the propagation branch. Before the fix every `read_exact`
    // failure became `Ok(None)`, swallowing this fault.
    let dir = std::env::temp_dir();
    let err = read_file_with_eol(&dir).expect_err("reading a directory must error");
    assert_ne!(err.kind(), std::io::ErrorKind::UnexpectedEof);
}

#[test]
fn read_eol_reports_tiny_readable_file_as_none() {
    // The "≤ 3 bytes is not worth parsing" shortcut still holds for a file
    // the process can read: `Ok(None)`, no error.
    let path = write_tmp("bca_read_eol_tiny_readable", b"a;\n");
    assert_eq!(read_file_with_eol(&path).unwrap(), None);
}

/// #1060: a tiny file the process cannot read must surface the permission
/// error, not the `Ok(None)` that means "empty, nothing to do". `stat`
/// succeeds without read permission, so the size-only shortcut made an
/// unreadable 3-byte file indistinguishable from an empty one — and
/// `bca check` counted it as analysed and exited 0 on a tree it never
/// read. Unix-only: it relies on POSIX permission bits.
#[cfg(unix)]
#[test]
fn read_eol_surfaces_permission_error_for_tiny_file() {
    use std::os::unix::fs::PermissionsExt;

    // A private `TempDir` rather than the shared `write_tmp` path: an
    // unreadable file left behind at a fixed name makes the *next* run of
    // this test fail in setup, and `TempDir` cleans up even when an
    // assertion panics. Unlinking a mode-000 file needs write permission
    // on the directory, not the file, so no chmod-back is required.
    let dir = tempfile::tempdir().unwrap();
    // 3 bytes, so the file lands inside the too-small shortcut.
    let path = dir.path().join("tiny.c");
    write_file(&path, b"a;\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Probe the real capability rather than the uid: root ignores mode
    // bits, and then the scenario cannot be staged at all.
    if std::fs::read(&path).is_ok() {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    }

    let err = read_file_with_eol(&path).expect_err("an unreadable file must error");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

/// #1287: every skip branch of the reader names its own reason. Before the
/// classified variant existed, all four collapsed into `Ok(None)` and the CLI
/// announced each of them as "skipping empty file" — including a 4 KB binary
/// and a one-byte source. One fixture per branch, each crafted so no other
/// branch can claim it: the two BOM files and the stray-`0xFF` file are padded
/// past the 64-byte probe so they cannot be mistaken for too-small.
#[test]
fn read_eol_classified_names_every_skip_branch() {
    let dir = tempfile::tempdir().unwrap();

    let mut utf16_le = vec![0xFF, 0xFE];
    utf16_le.extend(utf16_ascii_body(
        "int main() { return 0; } // padding to exceed the probe window\n",
        true,
    ));
    let mut utf16_be = vec![0xFE, 0xFF];
    utf16_be.extend(utf16_ascii_body(
        "int main() { return 0; } // padding to exceed the probe window\n",
        false,
    ));
    let mut binary = b"valid prefix \xFF more".to_vec();
    binary.extend_from_slice(&[b'q'; 80]);

    let cases: [(&str, &[u8], SkipReason); 6] = [
        ("empty.rs", b"", SkipReason::Empty),
        ("one_byte.rs", b"x", SkipReason::TooSmall),
        ("three_bytes.rs", b"a;\n", SkipReason::TooSmall),
        ("utf16_le.rs", &utf16_le, SkipReason::Utf16Bom),
        ("utf16_be.rs", &utf16_be, SkipReason::Utf16Bom),
        ("binary.rs", &binary, SkipReason::NotUtf8),
    ];

    for (name, bytes, expected) in cases {
        let path = dir.path().join(name);
        write_file(&path, bytes).unwrap();
        assert_eq!(
            read_file_with_eol_classified(&path).unwrap(),
            Err(expected),
            "{name} must be skipped as {expected:?}"
        );
        // The `Option`-shaped shim is a projection of the same call, so it
        // must still report the skip — just without the reason.
        assert_eq!(
            read_file_with_eol(&path).unwrap(),
            None,
            "{name} must remain a skip through the shim"
        );
    }
}

/// The accepted path is untouched by the classification: a file that clears
/// every gate yields the same EOL-normalised bytes through both entry points.
/// Without this, a shim that returned `None` unconditionally would still pass
/// the skip assertions above.
#[test]
fn read_eol_classified_and_shim_agree_on_an_accepted_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accepted.rs");
    // CRLF and a missing trailing newline, so the assertion pins the
    // normalisation rather than a byte-for-byte echo of the input.
    write_file(&path, b"fn main() {}\r\nfn other() {}").unwrap();
    let expected = b"fn main() {}\nfn other() {}\n".to_vec();

    assert_eq!(
        read_file_with_eol_classified(&path).unwrap(),
        Ok(expected.clone())
    );
    assert_eq!(read_file_with_eol(&path).unwrap(), Some(expected));
}

/// The wording contract behind #1287: only `Empty` may claim the file is
/// empty. The CLI splices these phrases straight into `skipping {reason}:
/// {path}`, so a phrase that asserts emptiness for a non-empty file is the
/// exact defect this issue fixed.
#[test]
fn skip_reason_phrases_claim_emptiness_only_for_empty() {
    assert_eq!(SkipReason::Empty.to_string(), "empty file");

    for reason in [
        SkipReason::TooSmall,
        SkipReason::Utf16Bom,
        SkipReason::NotUtf8,
    ] {
        let phrase = reason.to_string();
        assert!(
            !phrase.contains("empty"),
            "{reason:?} renders {phrase:?}, which asserts emptiness"
        );
        // Each phrase is spliced after "skipping", so it must read as a noun
        // phrase naming a file — and must not re-spell a severity word, which
        // belongs to the presenting layer (`warn`).
        assert!(phrase.contains("file"), "{reason:?} renders {phrase:?}");
        assert!(
            !phrase.contains("warning") && !phrase.contains("error"),
            "{reason:?} renders {phrase:?}, which carries its own severity"
        );
    }
}
