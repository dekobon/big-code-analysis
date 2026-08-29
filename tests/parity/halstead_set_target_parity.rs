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

// A parity claim needs both dialects, so the test is absent (not
// vacuously green, not spuriously red) unless both features are on.
#[cfg(all(feature = "tcl", feature = "irules"))]
#[test]
fn set_target_counts_as_operand_in_both_dialects() {
    // `s` appears as a `set` target and (wrapped) as `$s`; `t` only as a
    // target. Pre-#1294 Tcl reported neither `s` nor `t`.
    let source = "set s 1\nset t $s\n";

    for (lang, ext) in [(LANG::Tcl, "tcl"), (LANG::Irules, "irule")] {
        let name = format!("parity.{ext}");

        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.clone())))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));
        let mut operands = Vec::new();
        flatten_operands(&ops, &mut operands);

        // Exact occurrence counts distinguish the fix from a regression
        // in either direction: a blanket kind exclusion drops `s` and
        // `t` entirely, while losing the parent guard double-counts the
        // `$s` leaf as a second `s`.
        for operand in ["s", "t", "1", "$s"] {
            assert_eq!(
                operands.iter().filter(|o| o.as_str() == operand).count(),
                1,
                "{lang:?}: `{operand}` must be exactly one operand; got {operands:?}",
            );
        }
        assert_eq!(
            operands.len(),
            4,
            "{lang:?}: operands must be exactly s, t, 1, $s; got {operands:?}",
        );

        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name)),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
        // expected: n2 = 4 (s, t, 1, $s), N2 = 4 in both dialects.
        assert_eq!(space.metrics.halstead.unique_operands(), 4, "{lang:?}");
        assert_eq!(space.metrics.halstead.total_operands(), 4, "{lang:?}");
    }
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
    let source = "set v {a b}\nset w \"a b\"\nif {$q} { set u 1 }\n";

    for (lang, ext) in [(LANG::Tcl, "tcl"), (LANG::Irules, "irule")] {
        let name = format!("parity.{ext}");

        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.clone())))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));
        let mut operands = Vec::new();
        flatten_operands(&ops, &mut operands);

        for operand in ["v", "w", "{a b}", "\"a b\"", "$q", "u", "1"] {
            assert_eq!(
                operands.iter().filter(|o| o.as_str() == operand).count(),
                1,
                "{lang:?}: `{operand}` must be exactly one operand; got {operands:?}",
            );
        }
        assert_eq!(
            operands.len(),
            7,
            "{lang:?}: the braced and quoted words are one operand each, \
             and the `if` body is none; got {operands:?}",
        );

        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name)),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
        // expected: n2 = N2 = 7 — the seven operands above. The two
        // spellings of the value score alike, which they did not before
        // #1354: the braced one was three operands (`a`, `b` and the
        // word) against the quoted one's single operand, and the `if`
        // body was a tenth.
        assert_eq!(space.metrics.halstead.unique_operands(), 7, "{lang:?}");
        assert_eq!(space.metrics.halstead.total_operands(), 7, "{lang:?}");
    }
}
