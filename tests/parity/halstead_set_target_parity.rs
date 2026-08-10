//! Tcl / iRules parity test for the Halstead classification of a `set`
//! target versus a `variable_substitution` leaf.
//!
//! Both grammars emit the same `id` kind for a standalone assignment
//! target (`set s …` → `s`) and for the leaf inside `$s`, so the getter
//! must tell them apart by parent, not by kind. iRules always did;
//! Tcl carried a blanket kind exclusion instead, which silently dropped
//! every assigned variable from `n2`/`N2` (#1294). The two dialects had
//! drifted in opposite directions — this fixture keeps them agreeing.

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
