use std::collections::BTreeMap;
use std::collections::HashMap;
use tree_sitter::Language;

/// Lifts an `askama` render failure onto the `io::Result` channel every
/// generator already returns.
///
/// The templates are compile-time checked, so a render failure means a
/// formatting error rather than a malformed template — but "unlikely"
/// is not "impossible", and each caller is three lines from an `Err`
/// it can return (#1227).
#[must_use]
pub fn render_error(err: askama::Error) -> std::io::Error {
    std::io::Error::other(err)
}

/// Rewrites a tree-sitter node-kind string into a valid Rust identifier.
///
/// Punctuation becomes its symbolic name (`+` -> `PLUS`), separated
/// from any preceding alphanumeric run by `_`; characters with no
/// mapping are dropped. A handful of whole-token special cases (the
/// BOM, `_`, `self`, `Self`) are translated up front because the
/// generic rule would leave them empty or reserved.
#[must_use]
pub fn sanitize_identifier(name: &str) -> String {
    // Match both the canonical U+FEFF (a UTF-8-decoded BOM token, the
    // shape tree-sitter actually produces from `node_kind_for_id`) and
    // the three-codepoint mojibake form (U+00EF U+00BB U+00BF) the
    // original literal `"ï»¿"` decoded to — covers whichever the
    // backing grammar happens to expose. See issue #345.
    if name == "\u{FEFF}" || name == "\u{00EF}\u{00BB}\u{00BF}" {
        return "BOM".to_string();
    }
    if name == "_" {
        return "UNDERSCORE".to_string();
    }
    if name == "self" {
        return "Zelf".to_string();
    }
    if name == "Self" {
        return "SELF".to_string();
    }

    let mut result = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            result.push(c);
        } else {
            let replacement = match c {
                '~' => "TILDE",
                '`' => "BQUOTE",
                '!' => "BANG",
                '@' => "AT",
                '#' => "HASH",
                '$' => "DOLLAR",
                '%' => "PERCENT",
                '^' => "CARET",
                '&' => "AMP",
                '*' => "STAR",
                '(' => "LPAREN",
                ')' => "RPAREN",
                '-' => "DASH",
                '+' => "PLUS",
                '=' => "EQ",
                '{' => "LBRACE",
                '}' => "RBRACE",
                '[' => "LBRACK",
                ']' => "RBRACK",
                '\\' => "BSLASH",
                '|' => "PIPE",
                ':' => "COLON",
                ';' => "SEMI",
                '"' => "DQUOTE",
                '\'' => "SQUOTE",
                '<' => "LT",
                '>' => "GT",
                ',' => "COMMA",
                '.' => "DOT",
                '?' => "QMARK",
                '/' => "SLASH",
                '\n' => "LF",
                '\r' => "CR",
                '\t' => "TAB",
                _ => continue,
            };
            if !result.is_empty() && !result.ends_with('_') {
                result.push('_');
            }
            result += replacement;
        }
    }
    result
}

/// Escapes a tree-sitter node-kind string for embedding in a string
/// literal.
///
/// Emits the single-backslash form, for a literal that is parsed exactly
/// once. Every generator here wants that: the Rust, Go and JSON outputs
/// each interpolate the value into one string literal and no output
/// format has a second source-string interpretation layer. A
/// double-backslash variant existed behind an `escape: bool` until
/// #1241; its last caller was the JSON generator, and #862 established
/// that caller was double-escaping by mistake. The flag is gone so the
/// #862 bug is unrepresentable rather than one flipped boolean away.
#[must_use]
pub fn sanitize_string(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\"' => result += "\\\"",
            '\\' => result += "\\\\",
            '\t' => result += "\\t",
            '\n' => result += "\\n",
            '\r' => result += "\\r",
            _ => result.push(c),
        }
    }
    result
}

/// Converts a `snake_case` identifier to `CamelCase`.
///
/// Underscores only toggle the capitalize-next flag and are never
/// emitted, so leading, trailing, and doubled ones collapse away.
#[must_use]
pub fn camel_case(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut cap = true;
    for c in name.chars() {
        if c == '_' {
            cap = true;
        } else if cap {
            result.extend(c.to_uppercase().collect::<Vec<char>>());
            cap = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Claims a unique Rust name for `base`, returning it and whether a
/// collision-breaking suffix had to be minted.
///
/// `name_count` doubles as the taken-name set and the per-base counter:
/// every returned name is registered as a key, so a name minted here can
/// never be handed out twice and can never be claimed later by a literal
/// namesake either.
///
/// Registering the mint is the whole point (#1237). The previous code
/// incremented only the *base*'s counter and returned `base + counter`
/// without inserting it, so a minted name could silently duplicate a
/// node kind whose own sanitized name is literally `base + digits` —
/// and `tree-sitter-php` is one aliased rule away from exactly that: it
/// already emits `cast_type_token1` … `cast_type_token12`, so a single
/// new id carrying the kind string `cast_type_token1` would mint a
/// second `CastTypeToken12`. The generator would still exit `0` and the
/// duplicate would surface as a rustc duplicate-variant error in the
/// *parent* crate during a grammar bump, reading as upstream breakage
/// rather than a generator bug. Probing upward past a squatter makes the
/// name unique by construction instead.
///
/// The base counter advances to the suffix actually used, so a later
/// claim of the same base resumes above it and the walk stays linear
/// over repeated collisions rather than re-probing from the start.
fn claim_name(name_count: &mut HashMap<String, usize>, base: String) -> (String, bool) {
    let Some(&last) = name_count.get(&base) else {
        name_count.insert(base.clone(), 1);
        return (base, false);
    };
    let mut n = last;
    let candidate = loop {
        n += 1;
        let candidate = format!("{base}{n}");
        if !name_count.contains_key(&candidate) {
            break candidate;
        }
    };
    name_count.insert(base, n);
    name_count.insert(candidate.clone(), 1);
    (candidate, true)
}

/// Enumerates a grammar's node kinds as `(rust_name, renamed, ts_name)`
/// triples in node-kind id order, with the tree-sitter ERROR sentinel
/// appended last.
///
/// `renamed` marks an entry whose Rust name carries a numeric suffix to
/// break a collision with an earlier one. Named kinds are *resolved*
/// before anonymous ones, so a named kind keeps the unsuffixed name and
/// an anonymous namesake takes the suffix; that is a naming priority,
/// not the order of the returned `Vec`, which stays keyed on the id so
/// the generated enum's discriminants match tree-sitter's.
///
/// # Panics
///
/// Panics when the grammar reports a `node_kind_count` outside
/// tree-sitter's own `u16` id space, which means a provably-broken
/// grammar (#548). Wrapping the index instead would silently read a
/// different node kind, and this is a build-time generator rather than
/// shipped library code, so failing the run loudly is the cheaper
/// outcome.
#[must_use]
pub fn get_token_names(language: &Language) -> Vec<(String, bool, String)> {
    let count = language.node_kind_count();
    let mut names = BTreeMap::default();
    let mut name_count = HashMap::new();
    for anon in &[false, true] {
        for i in 0..count {
            // tree-sitter's node-kind id space is u16, but node_kind_count()
            // returns usize. A real grammar cannot exceed u16, so a failed
            // conversion means a provably-broken grammar; fail the codegen
            // binary loudly rather than wrap the index and read a different
            // node kind (see issue #548). This is a build-time generator,
            // not shipped library code.
            let Ok(id) = u16::try_from(i) else {
                panic!("grammar node_kind_count {count} exceeds u16 id space");
            };
            let anonymous = !language.node_kind_is_named(id);
            if anonymous != *anon {
                continue;
            }
            // `id < count` is a valid node kind by construction of the loop,
            // so node_kind_for_id never returns None here.
            let kind = language
                .node_kind_for_id(id)
                .expect("id < node_kind_count is a valid node kind");
            let ts_name = sanitize_string(kind);
            let mut name = camel_case(&sanitize_identifier(kind));
            if name.is_empty() {
                name = format!("Anon{i}");
            }
            let (name, renamed) = claim_name(&mut name_count, name);
            names.insert(i, (name, renamed, ts_name));
        }
    }
    let mut names: Vec<_> = names.values().cloned().collect();
    // The tree-sitter ERROR sentinel is appended last. If the grammar already
    // defines an "error" keyword that camel-cased to "Error", `claim_name`
    // mints a unique name for the sentinel (e.g. "Error2"), probing past any
    // literal kind already squatting that suffix (#1237). The sentinel's
    // `renamed` flag stays `false` whatever name it lands on: the flag marks
    // an entry the reverse string->kind mappings must drop, and the sentinel
    // is excluded from those for its own reasons — pinned by
    // `get_token_names_appends_error_sentinel_last`.
    let (error_name, _minted) = claim_name(&mut name_count, "Error".to_string());
    names.push((error_name, false, "ERROR".to_string()));

    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A render failure reaches the caller as an `io::Error` that still
    /// carries the `askama` cause, rather than panicking (#1227).
    ///
    /// Unreachable through the generators — the templates are
    /// compile-time checked, so `render` only fails on a formatting
    /// error — which is exactly why this asserts on the lift directly
    /// instead of through `generate_rust`.
    #[test]
    fn a_render_failure_becomes_an_io_error_carrying_its_cause() {
        let err = render_error(askama::Error::Fmt);

        // `other` rather than a more specific kind: a template that
        // failed to format is not bad *input*, and the generators have
        // no kind of their own to claim.
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(
            err.get_ref()
                .and_then(|inner| inner.downcast_ref::<askama::Error>())
                .is_some(),
            "the askama::Error must be retrievable, not stringified"
        );
    }

    // Issue #345: the previous `"ï»¿"` literal was the three-codepoint
    // mojibake form (U+00EF U+00BB U+00BF) — the three UTF-8 BOM bytes
    // reinterpreted as Latin-1 chars. A tree-sitter grammar that
    // exposes a BOM token returns the *canonical* one-char U+FEFF
    // form; pin both shapes to a stable "BOM" identifier so future
    // grammar bumps cannot introduce an `Anon<N>` variant.
    #[test]
    fn sanitize_identifier_canonical_bom() {
        assert_eq!(sanitize_identifier("\u{FEFF}"), "BOM");
    }

    #[test]
    fn sanitize_identifier_mojibake_bom() {
        assert_eq!(sanitize_identifier("\u{00EF}\u{00BB}\u{00BF}"), "BOM");
    }

    #[test]
    fn sanitize_identifier_passes_through_simple_ascii() {
        assert_eq!(sanitize_identifier("foo_bar"), "foo_bar");
    }

    // Internal punctuation is replaced by its symbolic name with a
    // leading `_` so the result remains a valid Rust identifier; the
    // following alphanumeric runs directly into the replacement
    // without a trailing separator.
    #[test]
    fn sanitize_identifier_translates_punctuation() {
        assert_eq!(sanitize_identifier("a+b"), "a_PLUSb");
    }

    #[test]
    fn sanitize_identifier_handles_reserved_keywords() {
        assert_eq!(sanitize_identifier("_"), "UNDERSCORE");
        assert_eq!(sanitize_identifier("self"), "Zelf");
        assert_eq!(sanitize_identifier("Self"), "SELF");
    }

    // The single-backslash form, for a literal parsed exactly once. This
    // is now the only form `sanitize_string` can emit; the
    // double-backslash variant and its `escape=true` test went with the
    // flag in #1241.
    #[test]
    fn sanitize_string_escapes_for_a_single_parse() {
        assert_eq!(
            sanitize_string("a\"b\\c\td\ne\rf"),
            "a\\\"b\\\\c\\td\\ne\\rf"
        );
    }

    #[test]
    fn camel_case_simple() {
        assert_eq!(camel_case("foo_bar"), "FooBar");
    }

    // Underscores only toggle the capitalize-next flag; they are never
    // emitted. Leading, trailing, and doubled underscores therefore
    // collapse away entirely.
    #[test]
    fn camel_case_underscore_variants() {
        assert_eq!(camel_case("_foo"), "Foo");
        assert_eq!(camel_case("foo_"), "Foo");
        assert_eq!(camel_case("foo__bar"), "FooBar");
    }

    #[test]
    fn camel_case_empty_is_empty() {
        assert_eq!(camel_case(""), "");
    }

    #[test]
    fn get_token_names_appends_error_sentinel_last() {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let names = get_token_names(&language);
        let (rust_name, renamed, ts_name) = names.last().expect("non-empty token list");
        assert_eq!(ts_name, "ERROR");
        assert_eq!(rust_name, "Error");
        assert!(!renamed, "the ERROR sentinel is not a deduplicated entry");
    }

    // Every grammar the crate links, not just Rust. A duplicate name is a
    // rustc duplicate-variant error in the *parent* crate, surfacing
    // during a grammar bump where it reads as upstream breakage; catching
    // it here names the generator instead (#1237). Rust alone could not:
    // it has no reachable collision, so the assertion was vacuous for the
    // hazard it is written against. The live near-misses are elsewhere —
    // Rust's own literal `Expr2021`, and PHP's `cast_type_token1` …
    // `cast_type_token12`, which are one aliased rule away from a real
    // collision.
    #[test]
    fn get_token_names_have_unique_rust_names_in_every_grammar() {
        let mut checked = 0_usize;
        for lang in crate::languages::Lang::into_enum_iter() {
            let names = get_token_names(&crate::languages::get_language(&lang));
            let mut seen = std::collections::HashSet::new();
            for (rust_name, _, _) in &names {
                assert!(
                    seen.insert(rust_name.clone()),
                    "duplicate Rust name emitted for {lang:?}: {rust_name}"
                );
            }
            checked += 1;
        }
        assert!(checked > 1, "the sweep must cover more than one grammar");
    }

    // The PHP shape, reproduced synthetically because no current grammar
    // reaches it: a literal kind already occupies the suffix the mint
    // would hand out. The mint must probe past the squatter rather than
    // duplicate it (#1237). Before the fix the third claim returned
    // `Foo2` — the name the second row already holds.
    #[test]
    fn claim_name_probes_past_a_literal_squatting_the_mint_suffix() {
        let mut name_count = HashMap::new();
        assert_eq!(
            claim_name(&mut name_count, "Foo".to_string()),
            ("Foo".to_string(), false)
        );
        // A *literal* kind whose own sanitized name is the base plus a
        // digit — `Foo2` here, `CastTypeToken12` in tree-sitter-php.
        assert_eq!(
            claim_name(&mut name_count, "Foo2".to_string()),
            ("Foo2".to_string(), false)
        );
        // The second claim of `Foo` must not mint `Foo2` again.
        assert_eq!(
            claim_name(&mut name_count, "Foo".to_string()),
            ("Foo3".to_string(), true)
        );
        // And a claim of the squatter itself dedups against its own
        // registration rather than colliding.
        assert_eq!(
            claim_name(&mut name_count, "Foo2".to_string()),
            ("Foo22".to_string(), true)
        );
    }

    // Repeated collisions on one base advance the counter monotonically,
    // so the mint stays deterministic and linear rather than re-probing
    // from the start each time (#1237).
    #[test]
    fn claim_name_resumes_above_the_last_suffix_it_used() {
        let mut name_count = HashMap::new();
        let claims: Vec<String> = (0..4)
            .map(|_| claim_name(&mut name_count, "Bar".to_string()).0)
            .collect();
        assert_eq!(claims, ["Bar", "Bar2", "Bar3", "Bar4"]);
    }

    // The bool flag marks entries whose Rust name was suffixed with a
    // numeric counter (>= 2) to break a collision with an earlier entry.
    // A renamed name must therefore be exactly an original (non-renamed)
    // entry's name followed by digits. The Rust grammar is expected to
    // produce at least one such collision, so the dedup path is actually
    // exercised rather than skipped (the assertion is not vacuous).
    #[test]
    fn get_token_names_renamed_flag_marks_deduplicated_entries() {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let names = get_token_names(&language);
        let originals: Vec<&str> = names
            .iter()
            .filter(|(_, renamed, _)| !renamed)
            .map(|(n, _, _)| n.as_str())
            .collect();
        let mut renamed_count = 0_usize;
        for (rust_name, renamed, _) in &names {
            if *renamed {
                renamed_count += 1;
                let is_dedup_suffix = originals.iter().any(|base| {
                    rust_name.strip_prefix(base).is_some_and(|suffix| {
                        !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                    })
                });
                assert!(
                    is_dedup_suffix,
                    "renamed entry {rust_name} should be an original name plus a numeric counter"
                );
            }
        }
        assert!(
            renamed_count > 0,
            "the Rust grammar is expected to produce at least one deduplicated token name"
        );
    }
}
