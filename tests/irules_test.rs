//! End-to-end integration test for F5 iRules support.
//!
//! Drives the public [`analyze`] API on a realistic multi-handler iRules
//! script and checks the full `FuncSpace` tree — the same surface the CLI
//! and downstream library consumers use. This complements the per-metric
//! unit tests under `src/metrics/` by exercising the whole pipeline
//! (parse → space tree → metric rollups) for a representative file.
//!
//! Self-contained: unlike the `php`/`csharp` corpus tests, it holds the
//! fixture inline and asserts via the public API, so it touches neither the
//! `big-code-analysis-output` submodule nor any snapshot.
#![allow(missing_docs)]
// Metric sums are exact integer-valued counts (1.0, 7.0, …), so `==` is
// the right comparison — matching the per-metric test modules and the
// cross-language parity integration tests, which carry the same allow.
#![allow(clippy::float_cmp)]

use big_code_analysis::{LANG, MetricsOptions, Source, SpaceKind, analyze};

/// Idiomatic iRules: two event handlers (`when CLIENT_ACCEPTED` /
/// `when HTTP_REQUEST`) plus a `proc`. The HTTP handler exercises
/// `if`/`elseif`/`else`, a nested `if`, `&&`, the word-form string
/// comparators (`starts_with`/`eq`/`contains`), a `switch` with a `default`
/// arm, an early `return`, and command substitutions (`[HTTP::uri]`). The
/// grammar README documents these commands but ships no full sample, so the
/// fixture is hand-written.
const SOURCE: &str = r#"when CLIENT_ACCEPTED {
    set start [clock clicks]
}

when HTTP_REQUEST {
    if { [HTTP::uri] starts_with "/api" && [HTTP::method] eq "GET" } {
        pool api_pool
    } elseif { [HTTP::uri] contains "admin" } {
        if { [IP::client_addr] starts_with "10." } {
            pool admin_pool
        } else {
            HTTP::respond 403 content "Forbidden"
            return
        }
    } else {
        switch [HTTP::host] {
            "a.example.com" { pool pool_a }
            "b.example.com" { pool pool_b }
            default { pool pool_default }
        }
    }
}

proc rewrite_path { prefix uri } {
    return "$prefix$uri"
}
"#;

/// The analyzed tree must expose both `when` handlers and the `proc` as
/// `Function` spaces under the file `Unit`, with per-space metrics matching
/// the constructs each contains, and the file-level rollups summing them.
#[test]
fn irules_end_to_end_funcspace_tree() {
    let unit = analyze(
        Source::new(LANG::Irules, SOURCE.as_bytes()).with_name(Some("app.irule".to_string())),
        MetricsOptions::default(),
    )
    .expect("iRules source must parse and analyze cleanly");

    // Top-level space is the file Unit.
    assert_eq!(unit.kind, SpaceKind::Unit, "top-level space must be Unit");

    // Two `when` handlers + one `proc`, each a Function space (the
    // handlers-as-function-spaces decision, end to end).
    assert_eq!(
        unit.spaces.len(),
        3,
        "expected 3 function spaces (2 handlers + 1 proc), got {:?}",
        unit.spaces
            .iter()
            .map(|s| (s.name.clone(), s.kind))
            .collect::<Vec<_>>(),
    );
    assert!(
        unit.spaces.iter().all(|s| s.kind == SpaceKind::Function),
        "every child space must be a Function",
    );
    assert_eq!(
        unit.metrics.nom.functions_sum(),
        3.0,
        "nom counts both handlers and the proc as functions",
    );
    assert_eq!(
        unit.metrics.nom.closures_sum(),
        0.0,
        "iRules has no closures",
    );

    // The proc — located by name (the only named space). Two formal
    // parameters; one `return`; branch-free.
    let proc = unit
        .spaces
        .iter()
        .find(|s| s.name.as_deref() == Some("rewrite_path"))
        .expect("rewrite_path proc must be a function space");
    assert_eq!(proc.metrics.nargs.fn_args_sum(), 2.0, "proc has two params");
    assert_eq!(proc.metrics.nexits.exit_sum(), 1.0, "proc has one return");
    assert_eq!(
        proc.metrics.cyclomatic.cyclomatic_sum(),
        1.0,
        "proc body is branch-free",
    );

    // The complex HTTP_REQUEST handler — located by its cyclomatic
    // signature (handlers are anonymous). base 1 + if + elseif + nested if
    // + `&&` + two non-default switch arms = 7. One `return` early-exit; no
    // formal parameters (event context is implicit).
    let complex = unit
        .spaces
        .iter()
        .find(|s| s.metrics.cyclomatic.cyclomatic_sum() == 7.0)
        .expect("the HTTP_REQUEST handler should have cyclomatic 7");
    assert_eq!(complex.kind, SpaceKind::Function);
    assert_eq!(complex.metrics.cognitive.cognitive_sum(), 9.0);
    assert_eq!(complex.metrics.nexits.exit_sum(), 1.0);
    assert_eq!(
        complex.metrics.nargs.fn_args_sum(),
        0.0,
        "handlers take no formal parameters",
    );

    // The simple CLIENT_ACCEPTED handler — the remaining anonymous space:
    // branch-free, no exit, no nested decisions.
    let simple = unit
        .spaces
        .iter()
        .find(|s| {
            s.name.as_deref() != Some("rewrite_path")
                && s.metrics.cyclomatic.cyclomatic_sum() == 1.0
        })
        .expect("the CLIENT_ACCEPTED handler should have cyclomatic 1");
    assert_eq!(simple.metrics.cognitive.cognitive_sum(), 0.0);
    assert_eq!(simple.metrics.nexits.exit_sum(), 0.0);

    // File-level rollups sum across every space.
    assert_eq!(unit.metrics.cyclomatic.cyclomatic_sum(), 10.0);
    assert_eq!(unit.metrics.cognitive.cognitive_sum(), 9.0);
    assert_eq!(unit.metrics.nexits.exit_sum(), 2.0);
    assert_eq!(unit.metrics.nargs.nargs_total(), 2.0);
}
