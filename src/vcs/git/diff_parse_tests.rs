//! Unit tests for the unified-diff parser backing `score_diff` (issue
//! #580). These exercise the `(added, deleted, hunks, path)` extraction
//! and the partial-report contract directly on diff text — no
//! repository needed.
#![allow(clippy::float_cmp)]

use super::{parse_unified_diff, score_diff};
use crate::vcs::JitSource;
use crate::vcs::error::Error;
use std::path::Path;

/// Find the parsed touched-file entry for a path, by its new-side name.
fn touched<'a>(files: &'a [super::Touched], path: &str) -> &'a super::Touched {
    files
        .iter()
        .find(|t| t.path == Path::new(path))
        .unwrap_or_else(|| panic!("no touched entry for {path}; got {:?}", paths(files)))
}

fn paths(files: &[super::Touched]) -> Vec<String> {
    files.iter().map(|t| t.path.display().to_string()).collect()
}

#[test]
fn single_file_added_and_deleted_lines() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 ctx
-old
+new1
+new2
 tail
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    let t = touched(&files, "src/a.rs");
    // expected: two `+` body lines, one `-` body line, one hunk; the
    // ` ctx` / ` tail` context lines and the `---`/`+++`/`index`
    // headers are NOT counted.
    assert_eq!(t.added, 2, "two added lines");
    assert_eq!(t.deleted, 1, "one deleted line");
    assert_eq!(t.hunks, 1, "one hunk");
}

#[test]
fn multi_file_diff_separates_files_and_counts_hunks() {
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,2 @@
 keep
+added
diff --git a/docs/b.md b/docs/b.md
--- a/docs/b.md
+++ b/docs/b.md
@@ -1,2 +1,1 @@
-removed1
-removed2
+merged
@@ -10,1 +9,1 @@
-x
+y
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 2, "two distinct files");
    let a = touched(&files, "src/a.rs");
    assert_eq!((a.added, a.deleted, a.hunks), (1, 0, 1));
    let b = touched(&files, "docs/b.md");
    // expected: across two hunks, 2 `+` and 3 `-` body lines.
    assert_eq!((b.added, b.deleted, b.hunks), (2, 3, 2));
}

#[test]
fn rename_uses_new_side_path() {
    let diff = "\
diff --git a/old/name.rs b/new/name.rs
similarity index 95%
rename from old/name.rs
rename to new/name.rs
--- a/old/name.rs
+++ b/new/name.rs
@@ -1,1 +1,1 @@
-a
+b
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    // The new-side path is what diffusion keys on (matches the commit
    // path); the `rename from`/`rename to` lines are not body lines.
    let t = touched(&files, "new/name.rs");
    assert_eq!((t.added, t.deleted), (1, 1));
}

#[test]
fn new_file_has_no_dev_null_path() {
    let diff = "\
diff --git a/created.rs b/created.rs
new file mode 100644
index 000..abc
--- /dev/null
+++ b/created.rs
@@ -0,0 +1,2 @@
+line1
+line2
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    // `/dev/null` on the old side must not become the path; the new
    // side wins.
    let t = touched(&files, "created.rs");
    assert_eq!((t.added, t.deleted), (2, 0));
}

#[test]
fn deleted_file_dev_null_new_side_falls_back_to_header() {
    let diff = "\
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    // The new side is `/dev/null`; the path falls back to the `b/gone.rs`
    // from the `diff --git` header so the deletion still counts.
    let t = touched(&files, "gone.rs");
    assert_eq!((t.added, t.deleted), (0, 2));
}

#[test]
fn binary_file_counts_as_touched_with_zero_lines() {
    let diff = "\
diff --git a/logo.png b/logo.png
index 111..222 100644
Binary files a/logo.png and b/logo.png differ
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1, "binary file still counts as touched");
    let t = touched(&files, "logo.png");
    // No line churn or hunks for a binary blob (mirrors the commit
    // path, which skips binary blobs' line counts).
    assert_eq!((t.added, t.deleted, t.hunks), (0, 0, 0));
}

#[test]
fn crlf_line_endings_are_tolerated() {
    let diff =
        "diff --git a/a.rs b/a.rs\r\n--- a/a.rs\r\n+++ b/a.rs\r\n@@ -1,1 +1,1 @@\r\n-x\r\n+y\r\n";
    let files = parse_unified_diff(diff).expect("parse");
    let t = touched(&files, "a.rs");
    assert_eq!((t.added, t.deleted, t.hunks), (1, 1, 1));
}

#[test]
fn empty_diff_yields_no_files() {
    let files = parse_unified_diff("").expect("parse empty");
    assert!(files.is_empty(), "an empty diff touches no files");
}

#[test]
fn body_line_before_hunk_is_malformed() {
    // A `+`/`-` body line before any `@@` header is structurally broken.
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n+orphan\n";
    let err = parse_unified_diff(diff).expect_err("must reject");
    assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
}

#[test]
fn malformed_hunk_header_is_rejected() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n";
    let err = parse_unified_diff(diff).expect_err("must reject");
    assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
}

#[test]
fn quoted_non_ascii_path_is_unquoted() {
    // Default `core.quotePath=true` wraps a non-ASCII path in quotes and
    // octal-escapes its bytes: `naïve.txt` → `"na\303\257ve.txt"` (ï =
    // U+00EF = UTF-8 0xC3 0xAF). Both the `diff --git` header and the
    // `+++` line carry the quoted form; the parsed new-side path must be
    // the decoded `naïve.txt`, not the literal quoted string, so the
    // diffusion subsystem grouping keys on the real name.
    let diff = concat!(
        "diff --git \"a/na\\303\\257ve.txt\" \"b/na\\303\\257ve.txt\"\n",
        "index 111..222 100644\n",
        "--- \"a/na\\303\\257ve.txt\"\n",
        "+++ \"b/na\\303\\257ve.txt\"\n",
        "@@ -1,1 +1,1 @@\n",
        "-old\n",
        "+new\n",
    );
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    let t = touched(&files, "naïve.txt");
    assert_eq!((t.added, t.deleted, t.hunks), (1, 1, 1));
}

#[test]
fn spaced_binary_path_keeps_full_name() {
    // A binary file whose name contains a space is NOT quoted by git (a
    // space is not a quote-triggering byte) and carries no `+++ b/<path>`
    // line to self-correct, so the new-side path must be recovered from
    // the symmetric `a/X b/X` header rather than truncated at the first
    // space by `rsplit(' ')`.
    let diff = concat!(
        "diff --git a/my file.bin b/my file.bin\n",
        "index 111..222 100644\n",
        "Binary files a/my file.bin and b/my file.bin differ\n",
    );
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    let t = touched(&files, "my file.bin");
    assert_eq!((t.added, t.deleted, t.hunks), (0, 0, 0));
}

#[test]
fn quoted_spaced_non_ascii_path_is_unquoted() {
    // A name with BOTH a space and a non-ASCII byte is quoted (the
    // non-ASCII byte triggers quoting), so the quoted span is
    // self-delimiting and the embedded space must not split it. `é` =
    // U+00E9 = UTF-8 0xC3 0xA9.
    let diff = concat!(
        "diff --git \"a/caf\\303\\251 menu.txt\" \"b/caf\\303\\251 menu.txt\"\n",
        "--- \"a/caf\\303\\251 menu.txt\"\n",
        "+++ \"b/caf\\303\\251 menu.txt\"\n",
        "@@ -1,1 +1,1 @@\n",
        "-x\n",
        "+y\n",
    );
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    let t = touched(&files, "café menu.txt");
    assert_eq!((t.added, t.deleted, t.hunks), (1, 1, 1));
}

#[test]
fn unquote_git_path_decodes_escapes_and_passes_plain_through() {
    use super::unquote_git_path;
    // Unquoted tokens are returned borrowed, unchanged.
    assert_eq!(unquote_git_path("b/plain.rs").as_ref(), "b/plain.rs");
    // Octal byte escapes reassemble a UTF-8 multibyte character.
    assert_eq!(
        unquote_git_path("\"b/na\\303\\257ve.txt\"").as_ref(),
        "b/naïve.txt"
    );
    // Named C escapes decode to their control bytes.
    assert_eq!(unquote_git_path("\"a\\tb\\nc\"").as_ref(), "a\tb\nc");
    // Escaped quote and backslash decode to the literal characters.
    assert_eq!(unquote_git_path("\"q\\\"x\"").as_ref(), "q\"x");
    assert_eq!(
        unquote_git_path("\"back\\\\slash\"").as_ref(),
        "back\\slash"
    );
}

#[test]
fn rename_to_non_ascii_quotes_only_new_side() {
    // A rename from an ASCII name to a non-ASCII one quotes only the new
    // (`b/`) side: `diff --git a/old.txt "b/na\303\257ve.txt"`. The old
    // side stays unquoted, so `last_quoted_token` must pick the quoted new
    // side rather than a quoted old side, and the `+++` line (also quoted)
    // confirms it. Exercises the single-side-quoted path distinctly from
    // the both-sides-quoted modify.
    let diff = concat!(
        "diff --git a/old.txt \"b/na\\303\\257ve.txt\"\n",
        "similarity index 100%\n",
        "rename from old.txt\n",
        "rename to \"na\\303\\257ve.txt\"\n",
        "--- a/old.txt\n",
        "+++ \"b/na\\303\\257ve.txt\"\n",
        "@@ -1,1 +1,1 @@\n",
        "-a\n",
        "+b\n",
    );
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    // Diffusion keys on the new side, which is the decoded non-ASCII name.
    let t = touched(&files, "naïve.txt");
    assert_eq!((t.added, t.deleted), (1, 1));
}

#[test]
fn deleted_line_starting_with_dash_dash_is_counted() {
    // Regression (#580): a deleted line whose CONTENT begins with `-- `
    // (a SQL/Lua/Haskell/Ada comment) renders, under git's single-char
    // `-` prefix, as a body line literally beginning `--- `. The old
    // ungated `starts_with("--- ")` header branch SILENTLY DROPPED it.
    // Gating the header on `!saw_hunk` makes the post-hunk `--- …` line
    // count as a deletion.
    let diff = "\
diff --git a/q.sql b/q.sql
--- a/q.sql
+++ b/q.sql
@@ -1,2 +1,1 @@
 SELECT 1;
--- this is a sql comment
+SELECT 2;
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    let t = touched(&files, "q.sql");
    // expected: one real `+` line and one `-` line (the `-- comment`);
    // the pre-fix parser dropped the deletion entirely (deleted == 0).
    assert_eq!(t.added, 1, "one added line");
    assert_eq!(
        t.deleted, 1,
        "the `-- sql comment` deletion must be counted, not dropped"
    );
    assert_eq!(t.hunks, 1);
}

#[test]
fn added_line_starting_with_plus_plus_keeps_path_and_counts() {
    // Regression (#580): an added line whose CONTENT begins with `++ `
    // renders as a body line literally beginning `+++ `. The old
    // ungated `strip_prefix("+++ ")` header branch REWROTE the file path
    // to the line's content AND dropped the addition. Gating the header
    // on `!saw_hunk` keeps the path and counts the line.
    let diff = "\
diff --git a/m.cpp b/m.cpp
--- a/m.cpp
+++ b/m.cpp
@@ -1,1 +1,2 @@
 int x = 0;
+++ foo bar baz
";
    let files = parse_unified_diff(diff).expect("parse");
    assert_eq!(files.len(), 1);
    // The path must stay the real `m.cpp` from the header, NOT be
    // corrupted to `foo bar baz` by the misread body line.
    let t = touched(&files, "m.cpp");
    assert_eq!(t.added, 1, "the `++ …` line must be counted as added");
    assert_eq!(t.deleted, 0);
    assert_eq!(
        paths(&files),
        vec!["m.cpp".to_owned()],
        "the `+++ …` body line must not rewrite the file path"
    );
}

#[test]
fn combined_merge_diff_is_rejected() {
    // A combined / merge diff (`git diff --cc`) uses `@@@` headers and
    // 2-column +/- prefixes that this parser would miscount. Reject it
    // cleanly as a malformed diff instead of silently mis-scoring.
    // Use a `diff --git` header so the stanza opens and the `@@@`
    // header reaches `parse_hunk_header`; a real `git diff --cc`
    // preamble varies but the `@@@` hunk header is the invariant marker.
    let diff = "\
diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@@ -1,1 -1,1 +1,1 @@@
- a
 -b
++c
";
    let err = parse_unified_diff(diff).expect_err("combined diff must be rejected");
    assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
}

#[test]
fn plain_diff_u_without_git_header_is_rejected() {
    // POSIX `diff -u a.c b.c` output carries `---`/`+++`/`@@` but no
    // `diff --git` header, so no stanza ever opens. Without the
    // orphan-marker guard this parses to zero files and scores a
    // misleading 0.0; it must be rejected as an unsupported diff so a
    // `--fail-over` gate cannot silently pass a risky change.
    let diff = "\
--- a.c\t2026-01-01 00:00:00
+++ b.c\t2026-01-02 00:00:00
@@ -1,2 +1,2 @@
 keep
-old
+new
";
    let err = parse_unified_diff(diff).expect_err("plain diff -u must be rejected");
    assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
}

#[test]
fn combined_cc_diff_with_real_header_is_rejected() {
    // A real `git diff --cc` combined diff opens with a `diff --cc`
    // file header (not `diff --git`), so the parser never opens a
    // stanza and the `@@@` rejection in `parse_hunk_header` is never
    // reached. The `diff --cc` orphan marker is what rejects it here,
    // matching the documented "combined/merge diffs not supported".
    let diff = "\
diff --cc describe.c
index abc,def..ghi
--- a/describe.c
+++ b/describe.c
@@@ -1,1 -1,1 +1,1 @@@
- a
 -b
++c
";
    let err = parse_unified_diff(diff).expect_err("git diff --cc must be rejected");
    assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
}

#[test]
fn show_style_preamble_mentioning_at_at_still_parses() {
    // Guard against a false-positive: a `git show` / commit-message
    // preamble that merely contains a line starting with `@@` *before*
    // a real `diff --git` stanza must still parse (the stanza opens, so
    // the orphan marker is moot once files are non-empty).
    let diff = "\
commit deadbeef
Author: A B <a@b.c>

    Refactor the @@ dispatch table

diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,1 +1,2 @@
 keep
+added
";
    let files = parse_unified_diff(diff).expect("show-style preamble parses");
    assert_eq!(files.len(), 1, "one real file stanza");
    assert_eq!(files[0].added, 1);
}

#[test]
fn score_diff_marks_unavailable_groups_and_is_partial() {
    // A two-subsystem diff: size + diffusion are present; the report
    // type itself has NO history / experience / purpose fields, so an
    // absent group can never be read as a real zero (the #580 trap).
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,3 @@
 keep
+one
+two
diff --git a/docs/b.md b/docs/b.md
--- a/docs/b.md
+++ b/docs/b.md
@@ -1,1 +1,2 @@
 title
+body
";
    let report = score_diff(diff).expect("score diff");
    assert_eq!(report.source, JitSource::Diff);
    assert_eq!(report.size.files_touched, 2);
    assert_eq!(report.size.lines_added, 3);
    assert_eq!(report.diffusion.subsystems, 2, "src + docs");
    // The partial score is exactly size + diffusion contributions (the
    // unavailable groups contribute nothing because they are absent).
    let expected = report.contributions.size + report.contributions.diffusion;
    assert!(
        (report.partial_score - expected).abs() < 1e-12,
        "partial score {} != size+diffusion {expected}",
        report.partial_score
    );
    assert!(report.partial_score > 0.0);

    // The serialized JSON must NOT carry history / experience / purpose
    // keys at all — proving "unavailable" is distinct from "zero".
    let json = serde_json::to_value(&report).expect("serialize");
    let obj = json.as_object().expect("object");
    assert_eq!(obj["source"], "diff");
    for absent in ["history", "experience", "purpose", "commit", "score"] {
        assert!(
            !obj.contains_key(absent),
            "diff report must not carry `{absent}` (would be misread as a real value)"
        );
    }
    assert!(obj.contains_key("partial_score"));
}
