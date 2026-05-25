use std::collections::BTreeMap;
use std::collections::hash_map::{Entry, HashMap};
use tree_sitter::Language;

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
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
            let anonymous = !language.node_kind_is_named(i as u16);
            if anonymous != *anon {
                continue;
            }
            let kind = language.node_kind_for_id(i as u16).unwrap();
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
}
