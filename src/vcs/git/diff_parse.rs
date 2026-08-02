//! Unified-diff parser backing [`score_diff`] (issue #580), split out of
//! `jit.rs` (issue #585) once the parser plus its regression tests pushed
//! that file past the self-scan limits.
//!
//! This is concern 2 of the JIT backend: turning arbitrary `git diff` text
//! into the same per-file [`Touched`](super::jit::Touched) shape
//! `collect_touched` (its commit-side counterpart in `jit`) produces from a
//! tree diff, then feeding the *shared*
//! [`size_features`](super::jit::size_features)
//! / [`diffusion_features`](super::jit::diffusion_features) math — so a diff
//! score and a commit score never fork their metric computation. The
//! dependency is one-way: this module borrows the domain types and feature
//! helpers from `jit`; `jit` knows nothing of the text parser.

use std::borrow::Cow;
use std::path::PathBuf;

use super::jit::{Touched, diffusion_features, size_features};
use crate::vcs::error::Error;
use crate::vcs::jit::{
    JIT_SCHEMA_VERSION, JIT_SCORE_VERSION, JitDiffReport, JitSource, score_diff_features,
};

/// Score an arbitrary unified diff (issue #580). See
/// [`crate::vcs::score_diff`] for the public contract: only the size and
/// diffusion groups are computable, so the result is a partial
/// [`JitDiffReport`] that is **not comparable** to a commit score.
///
/// The diff text is parsed into the same per-file `(added, deleted, hunks,
/// path)` shape `collect_touched` produces, then fed through the *same*
/// [`size_features`](super::jit::size_features) /
/// [`diffusion_features`](super::jit::diffusion_features) / scoring path as a
/// commit — no forked metric math.
pub(crate) fn score_diff(diff: &str) -> Result<JitDiffReport, Error> {
    let touched = parse_unified_diff(diff)?;
    let size = size_features(&touched);
    let diffusion = diffusion_features(&touched);
    let (partial_risk_score, contributions) = score_diff_features(size, diffusion);
    Ok(JitDiffReport {
        jit_schema_version: JIT_SCHEMA_VERSION,
        jit_score_version: JIT_SCORE_VERSION,
        source: JitSource::Diff,
        partial_risk_score,
        size,
        diffusion,
        contributions,
    })
}

/// Parse a unified diff into the per-file [`Touched`] shape, counting added
/// (`+`) and removed (`-`) body lines and the hunk (`@@`) count for each
/// file. Binary-file stanzas contribute a touched file with zero line churn
/// (mirroring `collect_touched`, which skips binary blobs' line counts);
/// renames without a body change still count as a touched file.
///
/// The scored-commit-side path is the new (`+++ b/…`) name so diffusion
/// keys on the post-change tree, matching the commit path; `parent_path` is
/// always `None` (a bare diff has no prior history to look up).
///
/// # Errors
///
/// [`Error::InvalidDiff`] when a `@@` hunk header is malformed, a `+`/`-`
/// body line appears before any hunk header (a structurally broken diff),
/// or the input carries diff content but no `diff --git` file header at all
/// (plain `diff -u` or a combined/merge diff) — so a garbage or unsupported
/// input is a clean client error rather than a silent mis-count or a
/// misleading zero-churn score.
fn parse_unified_diff(diff: &str) -> Result<Vec<Touched>, Error> {
    let mut files: Vec<Touched> = Vec::new();
    // The file currently being accumulated: its parsed new-side path (once a
    // `+++` line is seen) and running counters. `None` until the first file
    // header opens a stanza.
    let mut current: Option<DiffFile> = None;
    // Set when a hunk header or combined-diff marker appears while no stanza
    // is open. If the input never opens a single `diff --git` stanza, this
    // turns an otherwise-silent empty result (plain `diff -u`, a
    // `git diff --cc` combined diff, other non-git diff text) into a clean
    // `InvalidDiff` instead of a misleading zero-churn score.
    let mut saw_orphan_marker = false;

    for raw in diff.lines() {
        // Tolerate CRLF: `lines()` strips `\n`, this strips a trailing `\r`.
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if let Some(rest) = line.strip_prefix("diff --git ") {
            // A new file stanza begins. Flush the previous file (if any) and
            // seed the path from the `a/… b/…` header as a fallback for a
            // binary or rename-only stanza that carries no `+++` line.
            flush_diff_file(&mut files, current.take());
            current = Some(DiffFile::new(diff_git_new_path(rest)));
            continue;
        }
        let Some(file) = current.as_mut() else {
            // No file stanza is open. `git diff` opens one with a
            // `diff --git` header above any hunk, so a hunk header or a
            // `diff --cc`/`diff --combined` marker here means the input is
            // not a supported git diff (plain `diff -u` carries no
            // `diff --git`; combined/merge diffs use `diff --cc`). Flag it
            // and reject after the walk *only if* no stanza ever opens, so a
            // `git show`/`git log -p` preamble that merely mentions `@@`
            // before its real `diff --git` stanzas is still accepted.
            if line.starts_with("@@")
                || line.starts_with("diff --cc ")
                || line.starts_with("diff --combined ")
            {
                saw_orphan_marker = true;
            }
            // Other preamble (commit message, `index` lines, blank): ignore.
            continue;
        };

        file.classify_body_line(line)?;
    }
    flush_diff_file(&mut files, current.take());
    if files.is_empty() && saw_orphan_marker {
        return Err(Error::InvalidDiff(
            "no `diff --git` file headers found; expected a git-style unified \
             diff (plain `diff -u` and combined/merge diffs are not supported)"
                .to_owned(),
        ));
    }
    Ok(files)
}

/// Accumulator for one file stanza while parsing a unified diff.
struct DiffFile {
    /// New-side path, seeded from the `diff --git` header and refined by the
    /// `+++ b/…` line. `None` only for a `/dev/null` new side (a deletion).
    new_path: Option<PathBuf>,
    /// Whether a `@@` hunk header has been seen yet (body lines before one
    /// are a malformed diff).
    saw_hunk: bool,
    added: u64,
    deleted: u64,
    hunks: u32,
}

impl DiffFile {
    fn new(new_path: Option<PathBuf>) -> Self {
        Self {
            new_path,
            saw_hunk: false,
            added: 0,
            deleted: 0,
            hunks: 0,
        }
    }

    /// Refine the new-side path from a `+++ ` header, dropping the `b/`
    /// prefix. A `/dev/null` new side marks a *deletion*: keep the
    /// `diff --git` header fallback (the `b/<old>` path) so the deleted file
    /// still has a name and counts toward the features.
    fn set_new_path(&mut self, raw: &str) {
        if let Some(path) = unified_path(raw) {
            self.new_path = Some(path);
        }
    }

    /// Set the new-side path authoritatively from a `rename to <path>` line.
    /// Unlike the ambiguous `diff --git a/<old> b/<new>` header (whose
    /// space-containing halves cannot be split unambiguously), the
    /// `rename to` line carries the bare new path on its own, so it is the
    /// reliable source for a rename-only stanza with a spaced or otherwise
    /// tricky name (issue #813). The path is unquoted with git's C-style
    /// decoder but carries no `a/`/`b/` prefix to strip.
    fn set_rename_to_path(&mut self, raw: &str) {
        if let Some(path) = rename_to_path(raw) {
            self.new_path = Some(path);
        }
    }

    /// Confirm a hunk header has opened, so a subsequent body line is
    /// well-formed (a `+`/`-` line before any `@@` is a malformed diff).
    fn require_open_hunk(&self) -> Result<(), Error> {
        if self.saw_hunk {
            Ok(())
        } else {
            Err(Error::InvalidDiff(
                "a +/- line appears before any @@ hunk header".to_owned(),
            ))
        }
    }

    /// Classify one `line` that falls *inside* this open file stanza,
    /// updating the hunk count and added/deleted body-line counters. The
    /// caller ([`parse_unified_diff`]) handles stanza framing (the
    /// `diff --git` header and the no-stanza-open preamble); this method owns
    /// only the per-line body grammar so the two concerns read separately.
    fn classify_body_line(&mut self, line: &str) -> Result<(), Error> {
        if line.starts_with("@@") {
            // Check the hunk header before the `+`/`-` content branches so a
            // `@@@`/`@@` line is never mistaken for a body line.
            parse_hunk_header(line)?;
            self.saw_hunk = true;
            self.hunks = self.hunks.saturating_add(1);
        } else if !self.saw_hunk && line.starts_with("+++ ") {
            // The `+++ b/<path>` new-side header only ever appears *before*
            // the first `@@` of a file. Once a hunk is open, a `+++ …` line
            // is a real added body line whose content starts with `++ `
            // (e.g. a `++` operator), so it falls through to the `+` branch.
            if let Some(path) = line.strip_prefix("+++ ") {
                self.set_new_path(path);
            }
        } else if !self.saw_hunk && line.starts_with("rename to ") {
            // A pure rename (similarity 100%, no body) emits no `+++ b/<new>`
            // line to self-correct, so the new-side path would otherwise be
            // taken solely from the ambiguous `diff --git a/<old> b/<new>`
            // header — truncated at the first space for a spaced name (issue
            // #813). The `rename to <new>` line carries the bare new path
            // unambiguously; use it. Gating on `!saw_hunk` keeps a post-hunk
            // body line that happens to start `rename to ` counted as content.
            if let Some(path) = line.strip_prefix("rename to ") {
                self.set_rename_to_path(path);
            }
        } else if !self.saw_hunk && line.starts_with("--- ") {
            // Pre-hunk old-side path: not needed (diffusion keys on the new
            // side), and must not be counted as a deleted body line. After a
            // hunk opens, a `--- …` line is a real deleted body line (e.g. a
            // SQL/Lua/Haskell `--` comment) and falls through to the `-`
            // branch — gating on `!saw_hunk` is what stops that deletion from
            // being silently dropped.
        } else if line.starts_with('+') {
            self.require_open_hunk()?;
            self.added = self.added.saturating_add(1);
        } else if line.starts_with('-') {
            self.require_open_hunk()?;
            self.deleted = self.deleted.saturating_add(1);
        }
        // Everything else is ignored: context lines (' '), `index`,
        // `old/new mode`, `rename from`, a `Binary files … differ` marker
        // (the file still flushes as a zero-churn touched entry, like the
        // commit path skips binary blobs), and `\ No newline at end of file`.
        Ok(())
    }
}

/// Push a finished [`DiffFile`] onto the touched list as a [`Touched`],
/// preferring the new-side path and falling back to the repo root for a
/// pure deletion (`/dev/null` new side) so the file still counts toward the
/// size and diffusion features.
fn flush_diff_file(files: &mut Vec<Touched>, file: Option<DiffFile>) {
    let Some(file) = file else { return };
    let path = file.new_path.unwrap_or_else(|| PathBuf::from(""));
    files.push(Touched {
        path,
        parent_path: None,
        added: file.added,
        deleted: file.deleted,
        hunks: file.hunks,
    });
}

/// Validate a `@@ -a,b +c,d @@` hunk header: it must carry both a `-` and a
/// `+` range marker. A line starting `@@` without them is a malformed
/// header. A `@@@` header (a combined / merge diff, `git diff --cc`) is
/// rejected outright: its 2-column `+`/`-` body prefixes would be
/// miscounted, and combined diffs are outside this parser's documented
/// `git diff` / `diff -u` scope.
fn parse_hunk_header(line: &str) -> Result<(), Error> {
    if line.starts_with("@@@") {
        return Err(Error::InvalidDiff(
            "combined/merge diffs (@@@ headers) are not supported".to_owned(),
        ));
    }
    // The minimal well-formed header is `@@ -l +l @@`; require both range
    // markers rather than fully parsing the (optional) counts, which the
    // size features do not use.
    if line.contains(" -") && line.contains(" +") {
        Ok(())
    } else {
        Err(Error::InvalidDiff(format!(
            "malformed hunk header: {line:?}"
        )))
    }
}

/// The new-side path from a `diff --git a/<old> b/<new>` header line (the
/// text after `diff --git `), used as a fallback before the `+++` line is
/// seen. Returns the `b/<new>` portion with its `b/` prefix dropped, or
/// `None` if the header carries no recognizable `b/` side.
///
/// Three header shapes are handled, in priority order: a C-quoted new side
/// (`"b/…"`, the `core.quotePath=true` default for non-ASCII names), an
/// unquoted symmetric modify (`a/X b/X`, which tolerates a space in `X`), and
/// a plain unquoted rename/copy (`a/<old> b/<new>`).
fn diff_git_new_path(rest: &str) -> Option<PathBuf> {
    let rest = rest.trim();
    // A quoted path (`core.quotePath=true`, the default for non-ASCII names)
    // is wrapped in `"…"` with its bytes escaped, so the quoted span is
    // self-delimiting even when the name contains spaces. When the new side
    // is quoted, the last top-level quoted token is unambiguous.
    if let Some(tok) = last_quoted_token(rest)
        && let Some(p) = unquote_git_path(tok)
            .strip_prefix("b/")
            .filter(|p| !p.is_empty())
    {
        return Some(PathBuf::from(p));
    }
    // Unquoted `a/<old> b/<new>`. A modify repeats the path (`a/X b/X`), so a
    // symmetric split recovers a name containing spaces that `rsplit(' ')`
    // would truncate — e.g. a binary file with a space, which carries no
    // `+++ b/<path>` line to self-correct.
    if let Some(p) = symmetric_modify_new_path(rest) {
        return Some(p);
    }
    // Rename / copy without special characters: the new side is the last
    // whitespace-separated token starting with `b/`.
    rest.rsplit(' ')
        .find_map(|tok| tok.strip_prefix("b/"))
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Index one past the closing quote of the quoted span opening at `start`
/// (which must be the opening `"`), clamped to `bytes.len()` for an
/// unterminated span. A `\<x>` escape spans two bytes, so a `\"` inside the
/// span does not close it.
fn scan_quoted_span(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && bytes[i] != b'"' {
        // Clamp so a trailing backslash cannot push the index past the end.
        i = if bytes[i] == b'\\' {
            (i + 2).min(bytes.len())
        } else {
            i + 1
        };
    }
    // Advance past the closing quote.
    (i + 1).min(bytes.len())
}

/// Index of the next space or quote at or after `start`, or `bytes.len()`.
fn scan_unquoted_span(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'"' {
        i += 1;
    }
    i
}

/// The last double-quoted top-level token of a `diff --git` header rest, with
/// its surrounding quotes, or `None` if the header carries no quoted token. A
/// quoted span is delimited by unescaped `"` and may contain escaped quotes
/// (`\"`) and spaces, so a quoted path is parsed as a single token here even
/// when `rsplit(' ')` would shred it.
fn last_quoted_token(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    let mut last = None;
    while i < bytes.len() {
        match bytes[i] {
            b' ' => i += 1,
            b'"' => {
                let start = i;
                i = scan_quoted_span(bytes, i);
                // The slice `[start..i]` spans the whole token, both quotes
                // included.
                last = Some(&rest[start..i]);
            }
            // Unquoted token: skip to the next space or quote. Bytes inside a
            // UTF-8 multibyte sequence are all >= 0x80, so comparing to ASCII
            // never lands mid-character.
            _ => i = scan_unquoted_span(bytes, i),
        }
    }
    last
}

/// Recover the new-side path from an unquoted `a/X b/X` header where old and
/// new are equal (a modify, not a rename). The symmetric shape lets the path
/// contain spaces: `body == X + " b/" + X`, so the midpoint split is exact
/// where `rsplit(' ')` would truncate at the first space. The `first == second`
/// equality also rejects any non-symmetric header (rename, mismatched halves).
fn symmetric_modify_new_path(rest: &str) -> Option<PathBuf> {
    let body = rest.strip_prefix("a/")?;
    // `body == X " b/" X` ⟹ `len(body) == 2*len(X) + 3`.
    let x_len = body.len().checked_sub(3)? / 2;
    let (first, tail) = body.split_at_checked(x_len)?;
    let second = tail.strip_prefix(" b/")?;
    (first == second && !first.is_empty()).then(|| PathBuf::from(first))
}

/// Decode one of git's named single-character C escapes to its byte, or
/// `None` if `b` is not a recognized named escape (the caller then treats it
/// as an octal escape or a literal backslash). The control bytes without a
/// Rust escape (`\a` BEL, `\b` BS, `\v` VT, `\f` FF) are spelled in hex.
fn decode_named_escape(b: u8) -> Option<u8> {
    Some(match b {
        b'a' => 0x07,
        b'b' => 0x08,
        b't' => b'\t',
        b'n' => b'\n',
        b'v' => 0x0b,
        b'f' => 0x0c,
        b'r' => b'\r',
        b'"' => b'"',
        b'\\' => b'\\',
        _ => return None,
    })
}

/// Decode git's C-style path quoting (`core.quotePath`, on by default).
///
/// `git diff` wraps a path in double quotes and escapes any byte that is
/// non-ASCII, a control character, a `"`, or a `\` — non-ASCII and other
/// raw bytes as 3-digit octal `\ooo`, the usual C controls as `\n`/`\t`/…
/// (e.g. `"a/na\303\257ve.txt"` for `a/naïve.txt`, where `ï` is UTF-8
/// `0xC3 0xAF`). An unquoted token is returned borrowed unchanged; a quoted
/// token has its quotes removed and escapes decoded back to bytes, which are
/// then read as UTF-8. A non-UTF-8 byte sequence (a path under a non-UTF-8
/// filesystem encoding) is decoded lossily: the diffusion subsystem grouping
/// this feeds is best-effort, so an approximate name beats the literal
/// quoted string.
fn unquote_git_path(token: &str) -> Cow<'_, str> {
    let Some(inner) = token.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
        return Cow::Borrowed(token);
    };
    let raw = inner.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let next = (raw[i] == b'\\').then(|| raw.get(i + 1).copied()).flatten();
        let Some(next) = next else {
            out.push(raw[i]);
            i += 1;
            continue;
        };
        if let Some(byte) = decode_named_escape(next) {
            out.push(byte);
            i += 2;
        } else if next.is_ascii_digit() && next < b'8' {
            // Up to three octal digits (starting just after the `\`) encode
            // one byte (git emits `\ooo`, always <= `\377`).
            let mut val: u16 = 0;
            let mut j = i + 1;
            while j < raw.len() && j < i + 4 && raw[j].is_ascii_digit() && raw[j] < b'8' {
                val = val * 8 + u16::from(raw[j] - b'0');
                j += 1;
            }
            // Mask to the low byte: git emits at most `\377`, but three octal
            // digits could spell `\400`–`\777`; keep one byte.
            out.push((val & 0xFF) as u8);
            i = j;
        } else {
            // Unknown escape: git does not emit one, so keep the backslash
            // literally rather than silently dropping it.
            out.push(b'\\');
            i += 1;
        }
    }
    match String::from_utf8(out) {
        Ok(s) => Cow::Owned(s),
        Err(e) => Cow::Owned(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

/// The new path from a `rename to <path>` line (the text after
/// `rename to `). Unlike [`unified_path`], this carries no `a/`/`b/` prefix
/// to strip: git writes the bare repo-relative path, quoted with its C-style
/// escaping (`core.quotePath`) only when the name has special bytes. Decode
/// the quoting and return the path, or `None` if it is empty.
fn rename_to_path(raw: &str) -> Option<PathBuf> {
    let decoded = unquote_git_path(raw.trim());
    let path = decoded.as_ref();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn unified_path(raw: &str) -> Option<PathBuf> {
    // `git` appends nothing, but POSIX `diff -u` appends a tab + timestamp;
    // cut at the first tab to be tolerant of both.
    let trimmed = raw.split('\t').next().unwrap_or(raw).trim();
    // Decode git's C-style quoting first, so a `"a/na\303\257ve.txt"` form
    // yields the real name before the `a/`/`b/` prefix is stripped (the
    // prefix lives *inside* the quotes). `/dev/null` is never quoted, so the
    // decode is a no-op there.
    let path = unquote_git_path(trimmed);
    let path = path.as_ref();
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    let stripped = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    if stripped.is_empty() {
        None
    } else {
        Some(PathBuf::from(stripped))
    }
}

#[cfg(test)]
#[path = "diff_parse_tests.rs"]
mod tests;
