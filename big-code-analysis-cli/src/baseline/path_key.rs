//! Path canonicalisation and percent-encoding for baseline identity
//! keys.
//!
//! A key is produced by resolving a path against the baseline file's
//! *anchor* ([`anchor_for`]), folding `.`/`..` lexically, stripping the
//! anchor prefix, and percent-encoding the remaining `OsStr` bytes so
//! the result is valid UTF-8 for TOML without collapsing distinct paths
//! onto one key. [`normalize_path`] documents the pipeline and its one
//! deliberate non-injectivity (the `\` → `/` separator fold).

use std::path::{Component, Path, PathBuf};

/// Derive the canonical anchor for a baseline file at `baseline_path`:
/// the lexically-absolute directory the file lives in. Used at both
/// write and read time so the key shape is independent of `--paths`
/// form, working directory drift, or whether the path was passed
/// relative or absolute.
///
/// Lexical, not symlink-following: `bca` should not surprise users by
/// resolving `src/` through a symlinked directory. The cost is that a
/// baseline written via a symlinked invocation and read via the real
/// path (or vice-versa) does not match — but that mirrors how every
/// other tool (cargo, git) treats workdir identity.
pub(crate) fn anchor_for(baseline_path: &Path) -> PathBuf {
    // `std::path::absolute` only fails when the path is empty or the
    // platform cannot obtain the CWD (effectively never in a normal
    // shell). Fall back to the input path so the rest of the pipeline
    // degrades to pre-#376 behaviour (no canonicalisation) instead of
    // dying — the worst case is the path-stickiness this fix targets,
    // not a hard error.
    let abs = std::path::absolute(baseline_path).unwrap_or_else(|_| baseline_path.to_path_buf());
    let mut abs = lexical_normalize(&abs);
    // `pop` returns false if the path is already a root or empty; in
    // that degenerate case the path itself becomes the anchor.
    abs.pop();
    abs
}

/// Lexically normalise `p` by folding `.` and `..` components without
/// touching the filesystem. POSIX-style folding:
/// - `..` after a `Normal` component pops it (`a/b/../c` → `a/c`).
/// - `..` immediately after a `RootDir` or Windows `Prefix` is a no-op
///   (`/..` → `/`, `C:\..` → `C:\`) — you cannot go above the root.
/// - `..` with no prior component to consume is preserved literally
///   (`../a` → `../a`, `a/../../b` → `../b`). This keeps identity for
///   baselines that legitimately reference a sibling of the anchor.
pub(crate) fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                // Pop the previous Normal component (typical case).
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // POSIX/Windows: `..` past a root or drive prefix is
                // a no-op. `/..` resolves to `/`, not `/..`. Without
                // this case the normalised path would be non-canonical
                // and downstream `strip_prefix` would mis-match.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                // No prior Normal/Root/Prefix component (e.g.,
                // relative path starting with `..` or accumulating
                // multiple leading `..`s). Preserve the `..` literally
                // so a baseline that legitimately points at a sibling
                // of the anchor keeps a distinct identity.
                _ => out.push(c),
            },
        }
    }
    out
}

/// Normalize a path for use as a baseline identity key.
///
/// 1. The path is made lexically absolute (using `anchor` as the base
///    directory when relative) and `.` / `..` components are folded.
/// 2. If the result is under `anchor`, the anchor prefix is stripped so
///    the key form is `src/foo.rs` rather than `{anchor}/src/foo.rs`.
///    Paths outside the anchor (rare; e.g. a baseline that records
///    files from a sibling crate) keep their absolute form.
/// 3. The resulting `OsStr` is fed through the byte-level
///    percent-encoder for TOML safety (backslash → forward slash,
///    non-unreserved bytes → `%XX`, Windows unpaired surrogates →
///    `%uHHHH`). The encoding is injective **except** for the deliberate
///    `\` → `/` separator fold: a Windows-style `src\foo.rs` and a Unix
///    `src/foo.rs` are *intended* to collapse onto the same key so a
///    baseline written on one platform matches on the other. Apart from
///    that single equivalence, distinct byte sequences produce distinct
///    strings. The fold is applied uniformly across every encoder branch
///    (UTF-8, raw-byte, WTF-16) so the equivalence holds for non-UTF-8
///    paths carrying a literal `\` byte too (#704) — previously only the
///    UTF-8 fast path folded, so those keyed inconsistently.
///
/// Non-UTF-8 paths cannot be represented verbatim in a TOML string
/// (TOML mandates UTF-8). Falling back to `Path::display()` would
/// replace every invalid byte with U+FFFD and collapse distinct paths
/// onto the same key — exactly the lossy identity collision we have to
/// avoid. The per-byte encoder preserves identity by emitting `%XX` for
/// every byte not in the unreserved path set.
pub(super) fn normalize_path(anchor: &Path, p: &Path) -> String {
    // Resolve to an absolute, lexically-normalised PathBuf so the key
    // is independent of CWD and the `--paths` form the user passed.
    // An already-absolute `p` and an empty anchor both bypass the join
    // (empty-anchor is the in-memory-test case; absolute is the
    // `--paths "$PWD"` form).
    let abs = if p.is_absolute() || anchor.as_os_str().is_empty() {
        lexical_normalize(p)
    } else {
        lexical_normalize(&anchor.join(p))
    };
    let stripped = abs.strip_prefix(anchor).unwrap_or(&abs);
    encode_os_path(stripped.as_os_str())
}

/// Encode an OS string into a TOML-safe key. Routes through the
/// UTF-8 fast path when possible (still per-byte percent-encoded for
/// `%` safety), falls back to the platform-specific byte/WTF-16
/// encoders for non-UTF-8 paths. See [`normalize_path`] for the
/// injectivity guarantee.
fn encode_os_path(s: &std::ffi::OsStr) -> String {
    match s.to_str() {
        Some(s) => {
            let mut out = String::with_capacity(s.len());
            // The `\` → `/` separator fold lives in
            // `push_percent_encoded_byte` so every encoder branch (this
            // UTF-8 fast path, the non-UTF-8 byte / WTF-16 fallbacks)
            // folds identically — a Windows-style `src\foo.rs` and a
            // Unix `src/foo.rs` produce the same baseline key on either
            // platform. Previously only this branch folded, so a non-UTF-8
            // path carrying a literal `\` byte keyed differently (#704).
            for b in s.bytes() {
                push_percent_encoded_byte(&mut out, b);
            }
            out
        }
        None => encode_non_utf8_os_str(s),
    }
}

#[cfg(unix)]
fn encode_non_utf8_os_str(s: &std::ffi::OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    percent_encode_path_bytes(s.as_bytes())
}

#[cfg(windows)]
fn encode_non_utf8_os_str(s: &std::ffi::OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    percent_encode_wtf16(s.encode_wide())
}

#[cfg(not(any(unix, windows)))]
fn encode_non_utf8_os_str(s: &std::ffi::OsStr) -> String {
    // Exotic targets (wasm, etc.) where neither `OsStrExt` is available.
    // Reuse the per-byte encoder on the lossy UTF-8 form so output is
    // still TOML-safe; injectivity is best-effort here because the
    // platform itself has already destroyed the original bytes via
    // `to_string_lossy`. Prefix with U+FFFD so the key can never collide
    // with one produced through the `to_str()` branch above.
    //
    // This is the documented exception to `AGENTS.md`'s blanket ban on
    // `to_string_lossy()` for identifier paths: the two branches that
    // can recover the real bytes are `#[cfg]`-ed out on this target, so
    // there is nothing lossless left to call, and the U+FFFD prefix
    // confines the lossiness to a key space of its own.
    let mut out = String::from("\u{FFFD}");
    for &b in s.to_string_lossy().as_bytes() {
        push_percent_encoded_byte(&mut out, b);
    }
    out
}

/// Percent-encode the raw bytes of a non-UTF-8 path so the result is
/// (1) valid UTF-8 (required by TOML), (2) injective for distinct byte
/// sequences (required to keep baseline identities from collapsing),
/// and (3) human-recognizable for the common case where most bytes are
/// printable ASCII path characters. The unreserved set mirrors the
/// "safe for use in a filename" subset of RFC 3986 unreserved with
/// `/` added (path separator) and `%` excluded (escape introducer).
#[cfg(unix)]
fn percent_encode_path_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        push_percent_encoded_byte(&mut out, b);
    }
    out
}

/// Append a single byte to `out`, either verbatim (if it falls in the
/// unreserved path set) or as `%XX` (uppercase hex). The `%` byte
/// itself is not unreserved, so the output is unambiguous: every `%`
/// in the result was emitted by this function and is followed by
/// either two hex digits (from this function) or `u` followed by four
/// hex digits (from [`percent_encode_wtf16`]).
///
/// A backslash byte (`\`, 0x5C) is folded to `/` *before* the
/// unreserved check, so a Windows-style separator keys identically to a
/// Unix one regardless of which encoder branch produced the byte. This
/// is the single fold point shared by the UTF-8, raw-byte, and WTF-16
/// encoders — keeping them consistent was the #704 fix. The fold is
/// deliberately non-injective (`\` and `/` collapse to one key); the
/// `normalize_path` doc records that exception.
fn push_percent_encoded_byte(out: &mut String, b: u8) {
    use std::fmt::Write;
    let b = if b == b'\\' { b'/' } else { b };
    let is_unreserved = b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'~' | b'/' | b':' | b'+' | b',' | b' '
        );
    if is_unreserved {
        out.push(b as char);
    } else {
        // Writing to a String can only fail on allocation failure, which
        // already panics in the standard library.
        let _ = write!(out, "%{b:02X}");
    }
}

/// Percent-encode a WTF-16 code-unit sequence into a TOML-safe UTF-8
/// string. Valid scalar values are encoded as their UTF-8 bytes through
/// [`push_percent_encoded_byte`]; unpaired surrogates are emitted as
/// `%uHHHH` (uppercase 4-digit hex), a form the byte encoder never
/// produces. The result is:
///
/// 1. **Injective**: every code unit maps to a distinct token (either
///    a sequence of `%XX` byte escapes / unreserved bytes for a paired
///    scalar, or one `%uHHHH` for an unpaired surrogate). Two distinct
///    WTF-16 sequences therefore always produce distinct strings.
/// 2. **Stable**: deterministic; no allocation order or hashing
///    influences output.
/// 3. **Human-debuggable enough**: ASCII path components survive
///    unchanged.
///
/// Compiled under `test` as well as `windows` purely so the unit tests
/// can drive it with synthetic input on any platform (the production
/// caller is `#[cfg(windows)]` only).
#[cfg(any(windows, test))]
fn percent_encode_wtf16(units: impl IntoIterator<Item = u16>) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let mut buf = [0u8; 4];
    for r in char::decode_utf16(units) {
        match r {
            Ok(c) => {
                for &b in c.encode_utf8(&mut buf).as_bytes() {
                    push_percent_encoded_byte(&mut out, b);
                }
            }
            Err(e) => {
                let _ = write!(out, "%u{:04X}", e.unpaired_surrogate());
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "path_key_tests.rs"]
mod tests;
