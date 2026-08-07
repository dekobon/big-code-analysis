use std::collections::BTreeMap;
use std::collections::hash_map::{Entry, HashMap};
use tree_sitter::Language;

/// Lifts an `askama` render failure onto the `io::Result` channel every
/// generator already returns.
///
/// The templates are compile-time checked, so a render failure means a
/// formatting error rather than a malformed template — but "unlikely"
/// is not "impossible", and each caller is three lines from an `Err`
/// it can return (#1227).
pub fn render_error(err: askama::Error) -> std::io::Error {
    std::io::Error::other(err)
}

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

pub fn sanitize_string(name: &str, escape: bool) -> String {
    let mut result = String::with_capacity(name.len());
    if escape {
        for c in name.chars() {
            match c {
                '\"' => result += "\\\\\\\"",
                '\\' => result += "\\\\\\\\",
                '\t' => result += "\\\\t",
                '\n' => result += "\\\\n",
                '\r' => result += "\\\\r",
                _ => result.push(c),
            }
        }
    } else {
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
    }
    result
}

pub fn camel_case(name: String) -> String {
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

pub fn get_token_names(language: &Language, escape: bool) -> Vec<(String, bool, String)> {
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
            let name = sanitize_identifier(kind);
            let ts_name = sanitize_string(kind, escape);
            let mut name = camel_case(name);
            if name.is_empty() {
                name = format!("Anon{i}");
            }
            let e = match name_count.entry(name.clone()) {
                Entry::Occupied(mut e) => {
                    *e.get_mut() += 1;
                    (format!("{}{}", name, e.get()), true, ts_name)
                }
                Entry::Vacant(e) => {
                    e.insert(1);
                    (name, false, ts_name)
                }
            };
            names.insert(i, e);
        }
    }
    let mut names: Vec<_> = names.values().cloned().collect();
    // The tree-sitter ERROR sentinel is appended last. If the grammar already
    // defines an "error" keyword that camel-cased to "Error", increment the
    // counter so this sentinel gets a unique name (e.g. "Error2").
    let error_name = match name_count.entry("Error".to_string()) {
        Entry::Occupied(mut e) => {
            *e.get_mut() += 1;
            format!("Error{}", e.get())
        }
        Entry::Vacant(e) => {
            e.insert(1);
            "Error".to_string()
        }
    };
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

    // escape=false emits the single-backslash (Rust source) form.
    #[test]
    fn sanitize_string_no_escape() {
        assert_eq!(
            sanitize_string("a\"b\\c\td\ne\rf", false),
            "a\\\"b\\\\c\\td\\ne\\rf"
        );
    }

    // escape=true emits the double-backslash form (the value survives a
    // second round of source-string interpretation).
    #[test]
    fn sanitize_string_escape() {
        assert_eq!(
            sanitize_string("a\"b\\c\td\ne\rf", true),
            "a\\\\\\\"b\\\\\\\\c\\\\td\\\\ne\\\\rf"
        );
    }

    #[test]
    fn camel_case_simple() {
        assert_eq!(camel_case("foo_bar".to_string()), "FooBar");
    }

    // Underscores only toggle the capitalize-next flag; they are never
    // emitted. Leading, trailing, and doubled underscores therefore
    // collapse away entirely.
    #[test]
    fn camel_case_underscore_variants() {
        assert_eq!(camel_case("_foo".to_string()), "Foo");
        assert_eq!(camel_case("foo_".to_string()), "Foo");
        assert_eq!(camel_case("foo__bar".to_string()), "FooBar");
    }

    #[test]
    fn camel_case_empty_is_empty() {
        assert_eq!(camel_case(String::new()), "");
    }

    #[test]
    fn get_token_names_appends_error_sentinel_last() {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let names = get_token_names(&language, false);
        let (rust_name, renamed, ts_name) = names.last().expect("non-empty token list");
        assert_eq!(ts_name, "ERROR");
        assert_eq!(rust_name, "Error");
        assert!(!renamed, "the ERROR sentinel is not a deduplicated entry");
    }

    #[test]
    fn get_token_names_have_unique_rust_names() {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let names = get_token_names(&language, false);
        let mut seen = std::collections::HashSet::new();
        for (rust_name, _, _) in &names {
            assert!(
                seen.insert(rust_name.clone()),
                "duplicate Rust name emitted: {rust_name}"
            );
        }
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
        let names = get_token_names(&language, false);
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
