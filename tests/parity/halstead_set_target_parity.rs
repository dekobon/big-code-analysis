//! Tcl / iRules parity tests for the Halstead classification of the
//! constructs whose node kinds the two grammars spell identically: a
//! `set` target versus a `variable_substitution` leaf, and a braced
//! word versus the words inside it.
//!
//! Both grammars emit the same `id` kind for a standalone assignment
//! target (`set s …` → `s`) and for the leaf inside `$s`, so the getter
//! must tell them apart by parent, not by kind. iRules always did;
//! Tcl carried a blanket kind exclusion instead, which silently dropped
//! every assigned variable from `n2`/`N2` (#1294). The two dialects had
//! drifted in opposite directions — this fixture keeps them agreeing.
//!
//! `src/getter/tcl.rs` and `src/getter/irules.rs` are deliberate clones
//! and a defect in one is a defect in both, so a fix landed in only one
//! of them is what these tests exist to catch. Each drives the same
//! source through both dialects and asserts the same operand list.

#[cfg(all(feature = "tcl", feature = "irules"))]
use big_code_analysis::{Ast, LANG, MetricsOptions, Ops, Source, analyze};

/// One occurrence per operand, flattened across nested spaces so the
/// assertion does not depend on where each dialect opens spaces.
#[cfg(all(feature = "tcl", feature = "irules"))]
fn flatten_operands(ops: &Ops, out: &mut Vec<String>) {
    out.extend(ops.operands.iter().cloned());
    for space in &ops.spaces {
        flatten_operands(space, out);
    }
}

/// Drives `source` through both dialects and asserts that each of
/// `expected` occurs exactly once, that nothing else does, and that the
/// Halstead counts agree with the list. Exact occurrence counts
/// distinguish a fix from a regression in either direction: a dropped
/// operand and a double-billed one both change a count, and a wrapper
/// billed beside its parts shows up as an unexpected extra entry.
#[cfg(all(feature = "tcl", feature = "irules"))]
fn assert_each_operand_once_in_both_dialects(source: &str, expected: &[&str]) {
    let n = u64::try_from(expected.len()).expect("a fixture has fewer than 2^64 operands");
    for (lang, ext) in [(LANG::Tcl, "tcl"), (LANG::Irules, "irule")] {
        let name = format!("parity.{ext}");

        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.clone())))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));
        let mut operands = Vec::new();
        flatten_operands(&ops, &mut operands);

        for operand in expected {
            assert_eq!(
                operands.iter().filter(|o| o.as_str() == *operand).count(),
                1,
                "{lang:?}: `{operand}` must be exactly one operand; got {operands:?}",
            );
        }
        assert_eq!(
            operands.len(),
            expected.len(),
            "{lang:?}: operands must be exactly {expected:?}; got {operands:?}",
        );

        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name)),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
        // Every expected operand occurs once, so n2 = N2 = the list's
        // length in both dialects.
        assert_eq!(space.metrics.halstead.unique_operands(), n, "{lang:?}");
        assert_eq!(space.metrics.halstead.total_operands(), n, "{lang:?}");
    }
}

// A parity claim needs both dialects, so the test is absent (not
// vacuously green, not spuriously red) unless both features are on.
#[cfg(all(feature = "tcl", feature = "irules"))]
#[test]
fn set_target_counts_as_operand_in_both_dialects() {
    // `s` appears as a `set` target and (wrapped) as `$s`; `t` only as a
    // target. Pre-#1294 Tcl reported neither `s` nor `t`, and losing
    // the parent guard double-counts the `$s` leaf as a second `s`.
    assert_each_operand_once_in_both_dialects("set s 1\nset t $s\n", &["s", "t", "1", "$s"]);
}

// Same gating rationale as above: a parity claim needs both dialects.
#[cfg(all(feature = "tcl", feature = "irules"))]
#[test]
fn array_reference_counts_once_in_both_dialects() {
    // `$arr($i)` is the reference plus the index Tcl substitutes inside
    // the parens — two operands — and `arr(k)` as a `set` target is the
    // name plus the literal index. iRules listed the `array_index`
    // wrapper as a third operand for each; Tcl never did. The quoted
    // spelling is deliberate: the vendored Tcl grammar mis-parses a
    // bare `$arr(k)` in command-word position.
    assert_each_operand_once_in_both_dialects(
        "set arr(k) 1\nset z \"$arr($i)\"\n",
        &["arr", "k", "1", "z", "$arr($i)", "$i"],
    );
}

// Same gating rationale as above: a parity claim needs both dialects.
#[cfg(all(feature = "tcl", feature = "irules"))]
#[test]
fn braced_word_counts_once_in_both_dialects() {
    // Regression for #1354 / #1317. A braced word is one literal value
    // — Tcl substitutes nothing between braces — but the grammar models
    // its interior structurally, so the value was billed once for
    // itself and once per part: `{a b}` scored three operands where its
    // synonym `"a b"` scored one. A `braced_word` *script* body was an
    // operand too, spanning the whole block whose commands the walk had
    // already counted; the `if` body here is that form.
    //
    // Deliberately free of `proc` / `when`: neither dialect opens a
    // space for an `if` body, so `flatten_operands` reports each
    // occurrence once and the counts below are occurrence counts rather
    // than a per-space sum.
    //
    // Every operand is asserted by text and by occurrence count, so
    // both failure directions are caught: losing the guard restores
    // `a` / `b` beside the word, and dropping the wrapper in favour of
    // its children loses `{a b}` and with it the braced/quoted parity.
    // expected: n2 = N2 = 7 — the two spellings of the value score
    // alike, which they did not before #1354: the braced one was three
    // operands (`a`, `b` and the word) against the quoted one's single
    // operand, and the `if` body was a tenth.
    assert_each_operand_once_in_both_dialects(
        "set v {a b}\nset w \"a b\"\nif {$q} { set u 1 }\n",
        &["v", "w", "{a b}", "\"a b\"", "$q", "u", "1"],
    );
}
