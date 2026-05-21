//! Ad-hoc parser for Rust `cfg(...)` attribute predicates.
//!
//! Determines whether a Rust attribute body marks the annotated item as
//! test-only. Inputs are the *contents* of a `#[...]` / `#![...]`
//! attribute (e.g. `"test"`, `"cfg(test)"`, `"cfg(all(unix, test))"`),
//! not AST nodes — the predicate walker is intentionally a string-level
//! mini-parser because tree-sitter-rust does not expand attribute
//! macros for us.
//!
//! Extracted from `checker.rs` so the cfg parsing rules live next to
//! each other and can be exercised in isolation. The single public
//! entry point is [`attribute_marks_test`]; everything else is module-
//! private.

/// Return `true` if the Rust attribute body marks the annotated item
/// as test-only.
///
/// Recognised forms:
///
/// - Bare test-attribute aliases: `test`, `rstest`, `wasm_bindgen_test`,
///   `test_case`.
/// - Path-form test attributes: `tokio::test`, `ext::module::test(args)`,
///   etc. — detected without entering the predicate walker.
/// - `cfg(...)` predicates where `test` appears as an operand of `all`,
///   `any`, or a bare comma list, at any depth. A `not(test)` operand
///   short-circuits — the item is included in production builds, so it
///   is not test-only (regression test for #278).
///
/// The slow path collapses interior whitespace and retries, tolerating
/// unusual spacing like `# [ cfg ( test ) ]`.
pub(crate) fn attribute_marks_test(body: &str) -> bool {
    let matches_test = |s: &str| {
        matches!(s, "test" | "rstest" | "wasm_bindgen_test" | "test_case")
            || s.ends_with("::test")
            || s.contains("::test(")
            || cfg_inner(s).is_some_and(cfg_predicate_marks_test)
    };

    let trimmed = body.trim();
    if matches_test(trimmed) {
        return true;
    }
    // Slow path is only worth running when the input actually has
    // interior whitespace; the common cases hit the fast path above.
    if trimmed.bytes().any(|b| b.is_ascii_whitespace()) {
        return matches_test(&strip_whitespace(trimmed));
    }
    false
}

/// Strip interior whitespace from `s`, preserving multi-byte UTF-8.
///
/// Uses `chars()` (not `bytes().map(char::from)`) so a multi-byte
/// sequence like `é` (`0xC3 0xA9`) survives as a single `é` rather
/// than getting mangled into the two Latin-1 codepoints `Ã©`.
fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Return the inner predicate text of a `cfg(...)` attribute body,
/// stripping the `cfg(` prefix and matching `)`. Whitespace inside
/// is tolerated; callers receive a slice with surrounding spacing
/// preserved so the predicate walker can re-split on commas / parens.
fn cfg_inner(body: &str) -> Option<&str> {
    let rest = body.trim_start().strip_prefix("cfg")?.trim_start();
    let after_open = rest.strip_prefix('(')?;
    let inner = after_open.strip_suffix(')')?;
    Some(inner)
}

fn cfg_predicate_marks_test(pred: &str) -> bool {
    let trimmed = pred.trim();
    if trimmed == "test" {
        return true;
    }
    // `all(...)` and `any(...)` use the same "contains a `test`
    // operand" rule here. Strictly, `any(test, foo)` is over-broad
    // (the item is included in production when `foo` holds), but the
    // pre-#278 code treated both identically and the issue spec
    // preserves that behavior.
    if let Some(rest) = trimmed
        .strip_prefix("all")
        .or_else(|| trimmed.strip_prefix("any"))
        && let Some(args) = rest.trim_start().strip_prefix('(')
        && let Some(args) = args.strip_suffix(')')
    {
        return cfg_args_any_marks_test(args);
    }
    // Bare comma-separated predicate lists like `cfg(test, foo)`
    // — pre-#278 callers relied on this form being treated as
    // `cfg(all(test, foo))`. Skip if no top-level comma exists, so a
    // single ident does not accidentally fall through.
    if cfg_split_top_level_args(trimmed).nth(1).is_some() {
        return cfg_args_any_marks_test(trimmed);
    }
    false
}

/// Iterator over the comma-separated arguments of a cfg predicate
/// body, splitting at top-level commas only (commas inside nested
/// parens belong to a child predicate). Single-pass byte scan.
fn cfg_split_top_level_args(args: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0_i32;
    let mut start = 0_usize;
    let mut done = false;
    let bytes = args.as_bytes();
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 0 => {
                    let slice = &args[start..i];
                    start = i + 1;
                    return Some(slice);
                }
                _ => {}
            }
            i += 1;
        }
        done = true;
        Some(&args[start..])
    })
}

/// Walk a comma-separated argument list of a cfg predicate and return
/// true if any operand marks the item as test-only. Key=value forms
/// like `feature = "test"` never match.
fn cfg_args_any_marks_test(args: &str) -> bool {
    cfg_split_top_level_args(args).any(cfg_arg_marks_test)
}

/// Classify a single cfg predicate operand. Bare `test` matches;
/// `not(...)` never matches (its presence flips the gate); `all(...)`
/// and `any(...)` recurse; everything else (including `feature =
/// "test"`, plain idents, key=value pairs) does not match.
fn cfg_arg_marks_test(arg: &str) -> bool {
    let arg = arg.trim();
    if arg == "test" {
        return true;
    }
    // `not(...)` short-circuits: we do not look inside, because
    // `not(test)` excludes the item from test builds.
    if let Some(rest) = arg.strip_prefix("not").map(str::trim_start)
        && rest.starts_with('(')
        && rest.ends_with(')')
    {
        return false;
    }
    cfg_predicate_marks_test(arg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_attr_test_marks_bare_test_attribute() {
        // Direct attribute names (and aliases) match without ever
        // entering the cfg predicate walker. Locks in pre-#278
        // behavior so the rewrite does not regress the common case.
        assert!(attribute_marks_test("test"));
        assert!(attribute_marks_test("rstest"));
        assert!(attribute_marks_test("wasm_bindgen_test"));
        assert!(attribute_marks_test("test_case"));
        assert!(attribute_marks_test("tokio::test"));
        assert!(attribute_marks_test(
            "tokio::test(flavor = \"current_thread\")"
        ));
    }

    #[test]
    fn rust_attr_test_marks_cfg_test_variants() {
        // Pre-#278 forms with `test` in the first position must
        // still match.
        assert!(attribute_marks_test("cfg(test)"));
        assert!(attribute_marks_test("cfg(test, foo)"));
        assert!(attribute_marks_test("cfg(all(test, unix))"));
        assert!(attribute_marks_test("cfg(any(test, foo))"));
    }

    #[test]
    fn rust_attr_test_marks_cfg_with_test_not_first() {
        // Regression for #278. `test` was previously required to be
        // the first operand of `all(...)` / `any(...)`. The predicate
        // walker now matches it anywhere.
        assert!(
            attribute_marks_test("cfg(all(unix, test))"),
            "test as second all() operand must mark test-only"
        );
        assert!(
            attribute_marks_test("cfg(any(feature = \"x\", test))"),
            "test as second any() operand must mark test-only"
        );
        // Nested predicate: `any(test, ...)` inside `all(...)` still
        // counts as test-only via recursion.
        assert!(attribute_marks_test(
            "cfg(all(unix, any(test, feature = \"x\")))"
        ));
    }

    #[test]
    fn rust_attr_test_skips_not_test_and_feature_named_test() {
        // `cfg(not(test))` is *production-only*; it must not be
        // treated as test-only or `exclude_tests` would strip
        // production code.
        assert!(!attribute_marks_test("cfg(not(test))"));
        assert!(!attribute_marks_test("cfg(all(unix, not(test)))"));
        // A feature literally named "test" is a string-valued
        // key/value pair, not the bare `test` predicate.
        assert!(!attribute_marks_test("cfg(feature = \"test\")"));
        assert!(!attribute_marks_test("cfg(all(unix, feature = \"test\"))"));
        // Unrelated predicates remain unmatched.
        assert!(!attribute_marks_test("cfg(unix)"));
        assert!(!attribute_marks_test("derive(Debug)"));
        // `all(...)` / `any(...)` with no `test` operand anywhere must
        // not match — guards against an over-eager walker that treats
        // any combinator as test-only regardless of contents.
        assert!(!attribute_marks_test(
            "cfg(all(unix, target_os = \"linux\"))"
        ));
        assert!(!attribute_marks_test("cfg(any(unix, windows))"));
        assert!(!attribute_marks_test(
            "cfg(all(unix, any(feature = \"x\", feature = \"y\")))"
        ));
        // Nested `not(test)` inside `any(...)` is still non-matching;
        // `not(...)` short-circuits at any depth.
        assert!(!attribute_marks_test("cfg(any(unix, not(test)))"));
    }

    #[test]
    fn rust_attr_test_tolerates_internal_whitespace() {
        // The slow path strips ASCII whitespace before re-running
        // both checks, so spaced forms still resolve correctly.
        assert!(attribute_marks_test("cfg( all( unix , test ) )"));
        assert!(!attribute_marks_test("cfg( not ( test ) )"));
    }

    #[test]
    fn strip_whitespace_preserves_non_ascii_utf8() {
        // Regression test for #312. The slow path previously rebuilt
        // the compact string with `bytes().map(char::from).collect()`,
        // which interprets each byte as a Latin-1 codepoint and
        // mangles any multi-byte UTF-8 sequence. `é` (`0xC3 0xA9`)
        // would emerge as the two-char string `Ã©`. Iterating over
        // `chars()` decodes UTF-8 correctly.
        assert_eq!(strip_whitespace("é test"), "étest");
        assert_eq!(strip_whitespace("crate ::ñ::test"), "crate::ñ::test");
        assert_eq!(strip_whitespace("  日本語  test"), "日本語test");
        // ASCII-only inputs round-trip identically to the old code.
        assert_eq!(
            strip_whitespace("cfg( all( unix , test ) )"),
            "cfg(all(unix,test))"
        );
    }
}
