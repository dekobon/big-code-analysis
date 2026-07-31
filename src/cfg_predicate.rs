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

use std::ops::Range;

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

/// Byte offsets of every comma in a cfg predicate, bucketed by the
/// paren nesting depth the comma sits at.
///
/// A predicate is classified one *region* at a time: first the whole
/// predicate, then the argument list of every `all(...)` / `any(...)`
/// operand found inside it. A comma splits a region into operands
/// exactly when it sits at that region's own nesting depth, and a
/// region's depth is always its nesting level — a region begins right
/// after the `(` of an operand that itself starts at the parent
/// region's depth, so each descent adds exactly one. That makes the
/// depth a comma is recorded at directly comparable to the depth of
/// the region asking about it, so a single forward scan indexes the
/// split points of every region at once.
///
/// Before issue #1105 each region instead re-scanned its whole
/// interior just to learn whether it held a top-level comma, so a
/// predicate nested `d` levels deep was scanned `d` times over —
/// O(len²) in the attribute body, and a denial-of-service vector for
/// any `exclude_tests` run over machine-generated Rust.
struct CommaIndex {
    /// `(depth, offset)` pairs in ascending order, so the commas that
    /// split any one region occupy a contiguous run. Ordering by depth
    /// before offset is load-bearing — it is what groups a region's
    /// split points together for [`CommaIndex::splits`].
    entries: Vec<(usize, usize)>,
}

impl CommaIndex {
    /// Index every comma in `pred` by the paren depth it appears at.
    ///
    /// Depth is a signed running count, matching the split rule this
    /// replaced: an unbalanced `)` drives it negative and a later `(`
    /// brings it back up. A comma stranded at a negative depth belongs
    /// to no region and is dropped.
    fn build(pred: &str) -> Self {
        let mut entries = Vec::new();
        let mut depth = 0_isize;
        for (offset, byte) in pred.bytes().enumerate() {
            match byte {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' => {
                    if let Ok(comma_depth) = usize::try_from(depth) {
                        entries.push((comma_depth, offset));
                    }
                }
                _ => {}
            }
        }
        entries.sort_unstable();
        Self { entries }
    }

    /// Offsets of the commas that split `region`, whose own operands
    /// sit at `depth`.
    fn splits(&self, region: &Range<usize>, depth: usize) -> impl Iterator<Item = usize> {
        let first = self
            .entries
            .partition_point(|entry| *entry < (depth, region.start));
        let end = region.end;
        self.entries[first..]
            .iter()
            .take_while(move |(entry_depth, offset)| *entry_depth == depth && *offset < end)
            .map(|(_, offset)| *offset)
    }
}

/// Return `true` if the cfg predicate `pred` marks the item as
/// test-only.
///
/// Driven by an explicit work stack rather than mutual recursion: a
/// pathological deeply-nested input such as
/// `cfg(all(all(all(…test…))))` would otherwise recurse once per
/// nesting level (`cfg_predicate_marks_test` → operand walk →
/// `cfg_predicate_marks_test`) and overflow the stack on adversarial
/// or machine-generated attribute bodies (issue #709). The work stack
/// keeps live state on the heap, so nesting depth is bounded by
/// available memory rather than the call-frame limit.
///
/// Every operand is classified by [`classify_cfg_operand`]; the
/// [`CommaIndex`] turns "where does this region split" into a lookup
/// instead of a rescan, so the whole walk is linear in `pred` up to
/// the index sort (issue #1105).
fn cfg_predicate_marks_test(pred: &str) -> bool {
    let commas = CommaIndex::build(pred);
    // Regions still to classify, as `(byte range, nesting depth)`. Both
    // this and the index are bounded by `pred`'s length, so a deeper
    // predicate costs proportionally more memory, never more per byte.
    let mut stack = vec![(0..pred.len(), 0_usize)];
    while let Some((region, depth)) = stack.pop() {
        // Bare comma-separated predicate lists like `cfg(test, foo)`
        // — pre-#278 callers relied on this form being treated as
        // `cfg(all(test, foo))`. Splitting the region MUST happen
        // before an operand meets the `not`/`all`/`any` prefix checks:
        // those classify by leading prefix and trailing `)`, which only
        // describe a single operand. For a list whose first operand is
        // `not(...)` and last ends in `)` — e.g. `not(foo), all(test)`
        // — `strip_prefix("not")` leaves `(foo), all(test)`, which both
        // starts with `(` and ends with `)`, so the `not` short-circuit
        // would otherwise swallow the whole list and drop the trailing
        // `test` (regression for #763). The index respects paren depth,
        // so a comma nested inside a predicate's own parens —
        // `not(foo, bar)`, `all(test, unix)` — is not a split point and
        // the operand reaches the prefix checks intact.
        let mut operand_start = region.start;
        for split in commas.splits(&region, depth) {
            if classify_cfg_operand(pred, operand_start..split, depth, &mut stack) {
                return true;
            }
            operand_start = split + 1;
        }
        if classify_cfg_operand(pred, operand_start..region.end, depth, &mut stack) {
            return true;
        }
    }
    false
}

/// Classify one operand of a cfg predicate, given as a byte range of
/// `pred` at nesting depth `depth`.
///
/// Returns `true` when the operand is a bare `test`. An `all(...)` /
/// `any(...)` operand instead pushes its argument list onto `stack` as
/// a fresh region one level deeper. Everything else — `not(...)`, plain
/// idents, `feature = "test"` and other key/value pairs — neither
/// matches nor descends.
fn classify_cfg_operand(
    pred: &str,
    operand: Range<usize>,
    depth: usize,
    stack: &mut Vec<(Range<usize>, usize)>,
) -> bool {
    let raw = &pred[operand.clone()];
    let trimmed = raw.trim();
    if trimmed == "test" {
        return true;
    }
    // `not(...)` short-circuits: we do not look inside, because
    // `not(test)` excludes the item from test builds (#278). Kept as an
    // explicit arm even though it is currently redundant — an operand
    // starting with `not` cannot match the `all`/`any` prefixes below,
    // so it would fall through to `false` anyway. Deleting it would make
    // the #278 rule an emergent property of a prefix test three lines
    // down; stating it here keeps the rule visible if that test is ever
    // loosened.
    if trimmed
        .strip_prefix("not")
        .map(str::trim_start)
        .is_some_and(|rest| rest.starts_with('(') && rest.ends_with(')'))
    {
        return false;
    }
    // `all(...)` and `any(...)` use the same "contains a `test`
    // operand" rule here. Strictly, `any(test, foo)` is over-broad (the
    // item is included in production when `foo` holds), but the
    // pre-#278 code treated both identically and the issue spec
    // preserves that behavior.
    //
    // The three conditions are one shape check, so they read as one
    // chain: combinator name, then its opening paren, then a closing
    // paren as the operand's *last* byte. That last byte is not
    // necessarily the opening paren's match — `all(a)(b)` has argument
    // list `a)(b`, and the walk must reproduce that.
    if let Some(rest) = trimmed
        .strip_prefix("all")
        .or_else(|| trimmed.strip_prefix("any"))
        && let Some(inside) = rest.trim_start().strip_prefix('(')
        && let Some(args) = inside.strip_suffix(')')
    {
        // `inside` is a suffix of `trimmed`, and `args` a prefix of
        // `inside`, so both map back onto `pred` by length alone.
        let trimmed_end = operand.end - (raw.len() - raw.trim_end().len());
        let args_start = trimmed_end - inside.len();
        stack.push((args_start..args_start + args.len(), depth + 1));
    }
    false
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
    fn rust_attr_test_not_led_comma_list_keeps_later_test_operand() {
        // Regression for #763. A top-level comma list whose FIRST
        // operand is `not(...)` and whose LAST operand ends in `)` was
        // misclassified as a single `not(...)` operand: the `not`
        // short-circuit fired on the entire list, discarding the
        // trailing `test`-bearing operand. These must mark test-only.
        assert!(
            attribute_marks_test("cfg(not(foo), all(test))"),
            "not(foo), all(test) list must still see the trailing test"
        );
        assert!(
            attribute_marks_test("cfg(not(unix), any(test))"),
            "not(unix), any(test) list must still see the trailing test"
        );
        // Cases that were already correct must keep working: the
        // wrapped form, and a list not ending in `)`.
        assert!(attribute_marks_test("cfg(all(not(foo), all(test)))"));
        assert!(attribute_marks_test("cfg(not(foo), test)"));
        // A pure `not(test)` (single operand, no top-level comma) still
        // short-circuits to production-only.
        assert!(!attribute_marks_test("cfg(not(test))"));
        // A comma INSIDE the `not(...)` predicate's own parens is a
        // single operand, not a list — `not(foo, bar)` must remain a
        // short-circuiting non-match, and `all(test, unix)` must still
        // match via its own operand walk.
        assert!(!attribute_marks_test("cfg(not(foo, bar))"));
        assert!(!attribute_marks_test("cfg(not(test, unix))"));
        assert!(attribute_marks_test("cfg(all(test, unix))"));
    }

    #[test]
    fn rust_attr_test_tolerates_internal_whitespace() {
        // The slow path strips ASCII whitespace before re-running
        // both checks, so spaced forms still resolve correctly.
        assert!(attribute_marks_test("cfg( all( unix , test ) )"));
        assert!(!attribute_marks_test("cfg( not ( test ) )"));
    }

    #[test]
    fn rust_attr_test_handles_deeply_nested_cfg_without_overflow() {
        // Regression test for issue #709. The former mutual recursion
        // (`cfg_predicate_marks_test` ⇄ operand walk) recursed once per
        // nesting level and overflowed the stack on pathological input.
        // This depth comfortably blows a recursive stack (a recursive
        // walker overflows in the low tens of thousands of frames) yet
        // the work-stack walker must terminate and preserve semantics.
        const DEPTH: usize = 50_000;

        // Build `cfg(comb(comb(… inner …)))` directly — O(n) — rather than
        // by repeated `format!`, which is O(n²) in the nesting depth.
        fn nest(comb: &str, inner: &str) -> String {
            let mut s = String::with_capacity(DEPTH * (comb.len() + 1) + inner.len() + DEPTH + 5);
            s.push_str("cfg(");
            for _ in 0..DEPTH {
                s.push_str(comb);
                s.push('(');
            }
            s.push_str(inner);
            for _ in 0..DEPTH {
                s.push(')');
            }
            s.push(')');
            s
        }

        // all(all(all(... test ...))) — `test` buried at the bottom marks
        // the item test-only.
        assert!(
            attribute_marks_test(&nest("all", "test")),
            "deeply nested all(...) wrapping `test` must mark test-only"
        );

        // Same depth wrapping a non-test operand must still return false
        // rather than overflowing.
        assert!(
            !attribute_marks_test(&nest("any", "unix")),
            "deeply nested any(...) without `test` must not mark test-only"
        );

        // A deeply nested `not(test)` must short-circuit at the wrapping
        // depth without descending — still non-matching, still no overflow.
        assert!(
            !attribute_marks_test(&nest("all", "not(test)")),
            "deeply nested not(test) must remain production-only"
        );
    }

    #[test]
    fn cfg_predicate_classification_matches_pre_1105_walker() {
        // Issue #1105 replaced the pop-and-rescan predicate walker with
        // a comma index plus one classification pass. Every expectation
        // below was produced by running the *pre-#1105* walker over the
        // input, then transcribed here, so the table pins the exact
        // behaviour the rewrite had to preserve — including the corners
        // no hand-written test covered: unbalanced parens, empty and
        // whitespace-only operands, `test` as a substring, and the
        // long-standing blind spot that parens and commas inside string
        // literals are counted as structure.
        //
        // The rewrite was additionally checked against the old walker
        // over millions of generated predicates with zero disagreements;
        // this table is the cheap, checked-in residue of that run. Note
        // the generator alphabet has to be able to spell `test`:
        // `trimmed == "test"` is the only check in either implementation
        // that can return `true`, so a sweep over an alphabet without
        // `e` and `s` agrees trivially on every input and proves nothing
        // about the bug class that matters (a missed match).
        let cases: &[(&str, bool)] = &[
            // Unbalanced or truncated parens: the `all(...)` shape check
            // needs the operand's *last* byte to be `)`, so trailing
            // junk or a missing paren drops the whole operand.
            ("all(test", false),
            ("all(test))", false),
            ("all((test)", false),
            // `all(test)(x)` is the load-bearing member of this pair:
            // under matching-paren semantics it would be `true`. Its
            // neighbour classifies the same either way — keep both, but
            // do not drop this one.
            ("all(test)(x)", false),
            ("all(a)(test)", false),
            ("all(test)x", false),
            (")test", false),
            ("test)", false),
            ("(test", false),
            // A stray `)` drives the depth counter negative. A comma at
            // negative depth belongs to no region and is dropped, so it
            // splits nothing: `a),test` is one dead operand rather than
            // two. Clamping the depth at 0 instead would make that comma
            // a top-level split and wrongly surface the trailing `test`,
            // so this row is the end-to-end guard on the signed counter.
            ("a),test", false),
            // These two are *not* negative-depth cases despite the stray
            // `)`: the following `(` restores depth to 0, so the comma
            // does split. They are false because the second operand ends
            // in a trailing paren. Kept for the last-byte rule, not the
            // depth rule.
            ("all(a))(b, test)", false),
            ("all(a))(b, all(test))", false),
            ("any(test", false),
            // Empty and whitespace-only operands.
            ("", false),
            ("   ", false),
            ("all()", false),
            ("any()", false),
            ("not()", false),
            ("all( )", false),
            ("all(,)", false),
            (",", false),
            (",,", false),
            ("all(,test)", true),
            ("all(test,)", true),
            ("all(test,,)", true),
            // `test` as a substring of another identifier must not match.
            ("testing", false),
            ("not_test", false),
            ("x_test", false),
            ("alltest", false),
            ("nottest", false),
            ("anytest", false),
            ("all(testing)", false),
            ("all(x_test, testing)", false),
            // String literals are not lexed: a `(` or `,` inside one
            // still moves the depth counter. `all(v = "(", test)` is
            // therefore read as one operand and misses the `test`. This
            // is pre-existing behaviour, pinned here so a future lexer
            // change is a deliberate decision rather than a surprise.
            ("feature = \"test\"", false),
            ("all(feature = \"test\")", false),
            ("any(v = \"a,b\")", false),
            ("all(v = \"(\", test)", false),
            ("all(v = \"r#\\\"test(\\\"#\")", false),
            // Only `all` / `any` descend; every other combinator, `cfg`
            // and `cfg_attr` included, is an opaque operand.
            ("cfg_attr(test, derive(Debug))", false),
            ("cfg(test)", false),
            ("all(cfg(test))", false),
            // `not(...)` short-circuits at any depth, even around a
            // combinator that would otherwise match.
            ("not(all(test))", false),
            ("not(any(test))", false),
            ("not(not(test))", false),
            ("all(not(test), test)", true),
            ("any(not(test), unix)", false),
            // Whitespace between the combinator and its parens, and
            // around operands, is tolerated.
            (" all ( test ) ", true),
            ("all\t(test)", true),
            ("all\n(\ntest\n)", true),
            ("not (test)", false),
            ("all( unix , test )", true),
            // Non-ASCII operands neither match nor break byte offsets.
            ("all(é, test)", true),
            ("all(日本語)", false),
            ("тест", false),
            ("all(тест, test)", true),
            // Ordinary nesting.
            ("all(all(all(test)))", true),
            ("any(all(any(test)))", true),
            ("all(any(unix), test)", true),
            ("all(a, b, c, test)", true),
            ("all(a, b, c, unix)", false),
            // Sibling regions at the same depth, where the *earlier*
            // sibling contains a comma. Nothing else in this table has
            // that shape, and without it the lower bound of the index
            // lookup is never varied: dropping it leaves every other row
            // passing while these panic on an inverted slice range.
            ("any(all(unix, test), all(windows, foo))", true),
            ("all(all(a,b), all(c,d))", false),
            // A top-level comma list whose `test` follows a nested
            // region. The index must be ordered for the trailing operand
            // to be reached at all.
            ("a,all(b,c),test", true),
        ];
        for &(pred, expected) in cases {
            assert_eq!(
                cfg_predicate_marks_test(pred),
                expected,
                "predicate {pred:?} must classify as {expected}"
            );
        }
    }

    #[test]
    fn comma_index_buckets_by_paren_depth() {
        // The index is what makes classification linear: a region asks
        // for the commas at its own nesting depth instead of rescanning
        // its interior. Seed a predicate whose commas sit at three
        // different depths so each bucket is distinguishable from the
        // others and from an empty one.
        let pred = "a,all(b,c),any(d,all(e,f))";
        let index = CommaIndex::build(pred);

        let depth0: Vec<usize> = index.splits(&(0..pred.len()), 0).collect();
        assert_eq!(depth0, vec![1, 10], "commas outside any parens");
        // `all(b,c)` spans 2..10; its argument list 6..9 holds one
        // depth-1 comma, and the depth-1 comma of `any(...)` is outside
        // that range and must not leak in.
        let args: Vec<usize> = index.splits(&(6..9), 1).collect();
        assert_eq!(args, vec![7], "only the commas inside this region");
        let depth1: Vec<usize> = index.splits(&(0..pred.len()), 1).collect();
        assert_eq!(depth1, vec![7, 16], "both depth-1 commas");
        let depth2: Vec<usize> = index.splits(&(0..pred.len()), 2).collect();
        assert_eq!(depth2, vec![22], "the comma inside the inner all()");
        assert!(
            index.splits(&(0..pred.len()), 3).next().is_none(),
            "no region nests three deep here"
        );

        // A `)` with no opener drives the depth counter negative, so the
        // comma that follows splits nothing and is dropped entirely —
        // the behaviour the former `cfg_split_top_level_args` had, and
        // what makes `a),test` a single dead operand.
        //
        // Seeded with a depth-0 comma *before* the stray `)` so the
        // assertion distinguishes "dropped the negative-depth comma"
        // from "recorded no commas at all"; against an empty index both
        // readings look identical (.claude/rules/testing.md).
        let stray = CommaIndex::build("x,y),z");
        assert_eq!(
            stray.entries,
            vec![(0, 1)],
            "a comma at negative depth belongs to no region"
        );
        // The following `(` brings the counter back to zero, restoring
        // the comma as a top-level split point.
        let restored = CommaIndex::build("a)(b,c");
        assert_eq!(restored.entries, vec![(0, 4)]);
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
