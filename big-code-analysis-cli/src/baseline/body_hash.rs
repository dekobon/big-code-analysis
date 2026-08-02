//! Body hashing for the baseline's rename-tolerant fuzzy match (issue
//! #377): a digest of a normalised view of a function body that survives
//! reformatting and a rename of the function itself, plus the hex codec
//! that persists it in the baseline TOML.
//!
//! [`hash_body`] documents what the normalisation deliberately ignores
//! and why FNV-1a rather than the std hasher.

/// The bare (innermost) name of a qualified symbol: the segment after
/// the last `::`, or the whole string when there is no separator. Used
/// to match a v4 violation's qualified symbol (`MyStruct::do_thing`)
/// against a legacy v2/v3 baseline entry that stored only `do_thing`,
/// and to elide a function's own name from its body hash.
pub(crate) fn bare_name(qualified: &str) -> &str {
    qualified
        .rsplit_once("::")
        .map_or(qualified, |(_, tail)| tail)
}

/// Encode a body hash as the lowercase, zero-padded 16-digit hex form
/// stored in the TOML. Hex (not a TOML integer) because FNV-1a fills
/// the full `u64` range and TOML integers are `i64`.
pub(super) fn encode_body_hash(h: u64) -> String {
    format!("{h:016x}")
}

/// Decode a stored body hash. Returns `None` for any malformed digest
/// (wrong length, non-hex) so a hand-edited file degrades the entry to
/// "no fuzzy fallback" rather than aborting the whole load.
pub(super) fn decode_body_hash(s: &str) -> Option<u64> {
    (s.len() == 16)
        .then(|| u64::from_str_radix(s, 16).ok())
        .flatten()
}

/// FNV-1a offset basis and prime for the 64-bit variant. FNV is chosen
/// over the std `DefaultHasher` because the digest is persisted to disk
/// and must be byte-stable across bca versions, platforms, and process
/// runs — `DefaultHasher`'s algorithm and seed carry no such guarantee.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hash the normalised body of the space spanning `start_line..=end_line`
/// (1-based, inclusive) within `source`, with the function's own `name`
/// elided. This is the "fuzzy" in fuzzy matching: a digest of a view of
/// the body that survives the two changes a rename-or-relocate refactor
/// makes (issue #377, rule 3) while still distinguishing genuinely
/// different code:
///
/// 1. **Whitespace**: `\r` is dropped, each line's internal whitespace
///    runs collapse to one space, leading/trailing whitespace is
///    trimmed, and blank lines are skipped — so reformatting,
///    re-indentation, or blank-line churn does not change the digest.
/// 2. **The function's own name**: every whole-word occurrence of `name`
///    (the declaration and any recursive self-calls) is replaced with a
///    fixed sentinel, so renaming `classify` to `categorize` leaves the
///    digest unchanged. Whole-word means bounded by non-identifier bytes,
///    so a `name` of `is` does not corrupt `is_valid`.
///
/// Out-of-range lines are clamped (a `start_line` past EOF yields the
/// empty-body digest), so a malformed span never panics.
pub(crate) fn hash_body(source: &[u8], start_line: usize, end_line: usize, name: &str) -> u64 {
    let normalized = normalize_body(source, start_line, end_line);
    let elided = elide_identifier(&normalized, name.as_bytes());
    let mut hash = FNV_OFFSET_BASIS;
    for &b in &elided {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Build the whitespace-normalised byte view of a line span (step 1 of
/// [`hash_body`]). Lines are joined by `\n`; blank lines are dropped.
fn normalize_body(source: &[u8], start_line: usize, end_line: usize) -> Vec<u8> {
    let first = start_line.saturating_sub(1);
    let mut out = Vec::new();
    for line in source
        .split(|&b| b == b'\n')
        .skip(first)
        .take(end_line.saturating_sub(first))
    {
        let mut line_started = false;
        let mut pending_space = false;
        for &b in line {
            if b == b'\r' {
                continue;
            }
            if b.is_ascii_whitespace() {
                pending_space = true;
                continue;
            }
            if line_started && pending_space {
                out.push(b' ');
            }
            pending_space = false;
            out.push(b);
            line_started = true;
        }
        if line_started {
            out.push(b'\n');
        }
    }
    out
}

/// Sentinel emitted in place of the elided function name. `\x00` does not
/// appear in normalised source bytes, so it cannot collide with real
/// content.
const ELIDED_NAME_SENTINEL: u8 = 0x00;

/// Replace every whole-word occurrence of `name` in `haystack` with
/// [`ELIDED_NAME_SENTINEL`] (step 2 of [`hash_body`]). Whole-word means
/// neither neighbour is an identifier byte. An empty or non-identifier
/// `name` (e.g. the `<file>` / `<anon@L..>` sentinels) is left as-is.
fn elide_identifier(haystack: &[u8], name: &[u8]) -> Vec<u8> {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    if name.is_empty() || !name.iter().copied().all(is_ident) {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        let left_ok = i == 0 || !is_ident(haystack[i - 1]);
        let matches = left_ok
            && haystack[i..].starts_with(name)
            && haystack.get(i + name.len()).is_none_or(|&b| !is_ident(b));
        if matches {
            out.push(ELIDED_NAME_SENTINEL);
            i += name.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
#[path = "body_hash_tests.rs"]
mod tests;
