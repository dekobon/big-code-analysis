//! A `[ … ]` test scores like its `&&` / `||` twins (#1536).
//!
//! Bash joins two tests three ways: `[ X -a Y ]`, `[ X ] && [ Y ]` and
//! `[[ X && Y ]]` (and `-o` / `||` likewise). Before #1536 the first
//! scored no cyclomatic or cognitive decision for its `-a`, ABC counted
//! that `-a` as a comparison, and a `=` comparison scored no ABC
//! condition in any spelling. Every row groups the three spellings,
//! and each must score the row's decisions.

use std::cell::Cell;

use crate::test_support::check_func_space_only;
use crate::*;

/// `[cyclomatic, abc conditions, cognitive]` sums of the root space.
/// The cyclomatic sum carries the unit's and the function's base of 1
/// each. Bash ABC pays one condition for the `if` and one for each
/// comparison, and nothing for a connective.
#[cfg(feature = "bash")]
type Decisions = [u64; 3];

#[cfg(feature = "bash")]
fn measure(body: &str) -> Decisions {
    let out = Cell::new([0; 3]);
    check_func_space_only::<BashParser, _>(
        &format!("f() {{\n  {body}\n}}\n"),
        "foo.sh",
        &[Metric::Cyclomatic, Metric::Abc, Metric::Cognitive],
        |space| {
            let m = &space.metrics;
            out.set([
                m.cyclomatic.cyclomatic_sum(),
                m.abc.conditions_sum(),
                m.cognitive.cognitive_sum(),
            ]);
        },
    );
    out.get()
}

#[cfg(feature = "bash")]
#[test]
fn bash_test_connectives_score_like_their_twins_1536() {
    // `(single_bracket, chained_commands, double_bracket, decisions)`.
    let rows: [(&str, &str, &str, Decisions); 8] = [
        (
            r#"if [ "$a" = 1 -a "$b" = 2 ]; then :; fi"#,
            r#"if [ "$a" = 1 ] && [ "$b" = 2 ]; then :; fi"#,
            r#"if [[ "$a" = 1 && "$b" = 2 ]]; then :; fi"#,
            [4, 3, 2],
        ),
        (
            r#"if [ "$a" == 1 -a "$b" == 2 ]; then :; fi"#,
            r#"if [ "$a" == 1 ] && [ "$b" == 2 ]; then :; fi"#,
            r#"if [[ "$a" == 1 && "$b" == 2 ]]; then :; fi"#,
            [4, 3, 2],
        ),
        (
            r#"if [ "$a" -eq 1 -a "$b" -eq 2 ]; then :; fi"#,
            r#"if [ "$a" -eq 1 ] && [ "$b" -eq 2 ]; then :; fi"#,
            r#"if [[ "$a" -eq 1 && "$b" -eq 2 ]]; then :; fi"#,
            [4, 3, 2],
        ),
        (
            r#"if [ "$a" = 1 -o "$b" = 2 ]; then :; fi"#,
            r#"if [ "$a" = 1 ] || [ "$b" = 2 ]; then :; fi"#,
            r#"if [[ "$a" = 1 || "$b" = 2 ]]; then :; fi"#,
            [4, 3, 2],
        ),
        // A switch of operation is a second cognitive sequence in every
        // spelling.
        (
            r#"if [ "$a" = 1 -a "$b" = 2 -o "$c" = 3 ]; then :; fi"#,
            r#"if [ "$a" = 1 ] && [ "$b" = 2 ] || [ "$c" = 3 ]; then :; fi"#,
            r#"if [[ "$a" = 1 && "$b" = 2 || "$c" = 3 ]]; then :; fi"#,
            [5, 4, 3],
        ),
        // `-a` and `-o` key to the symbols they stand for: a run of one
        // is a single sequence. The grammar nests `=`, `==` and `!=`
        // around the connectives (`"$a" = (1 -o "$b") = …`), so these
        // rows also pin `ComparisonRuns`; `[[ … = … || … ]]` scored 3
        // before #1536 for the same reason.
        (
            r#"if [ "$a" = 1 -o "$b" = 2 -o "$c" = 3 ]; then :; fi"#,
            r#"if [ "$a" = 1 ] || [ "$b" = 2 ] || [ "$c" = 3 ]; then :; fi"#,
            r#"if [[ "$a" = 1 || "$b" = 2 || "$c" = 3 ]]; then :; fi"#,
            [5, 4, 2],
        ),
        (
            r#"if [ "$a" == 1 -o "$b" == 2 -o "$c" == 3 ]; then :; fi"#,
            r#"if [ "$a" == 1 ] || [ "$b" == 2 ] || [ "$c" == 3 ]; then :; fi"#,
            r#"if [[ "$a" == 1 || "$b" == 2 || "$c" == 3 ]]; then :; fi"#,
            [5, 4, 2],
        ),
        (
            r#"if [ "$a" != 1 -o "$b" != 2 -o "$c" != 3 ]; then :; fi"#,
            r#"if [ "$a" != 1 ] || [ "$b" != 2 ] || [ "$c" != 3 ]; then :; fi"#,
            r#"if [[ "$a" != 1 || "$b" != 2 || "$c" != 3 ]]; then :; fi"#,
            [5, 4, 2],
        ),
    ];
    for (single, chained, double, decisions) in rows {
        assert!(
            single.contains(" -a ") || single.contains(" -o "),
            "`{single}` lost its connective"
        );
        for body in [single, chained, double] {
            assert_eq!(measure(body), decisions, "`{body}`");
        }
    }
}

/// What the gates must leave alone: a unary `-a FILE` is a file test
/// like `-f FILE`, a `=` keeps comparing however the test nests it, `=`
/// inside `(( … ))` is an assignment, and a correctly nested switch of
/// operation is still three sequences.
#[cfg(feature = "bash")]
#[test]
fn bash_test_connective_gates_keep_their_controls_1536() {
    // `(body, twin, decisions)`; `twin` spells the same test another
    // way and must agree.
    let rows: [(&str, &str, Decisions); 6] = [
        (
            "if [ -a x ]; then :; fi",
            "if [ -f x ]; then :; fi",
            [3, 2, 1],
        ),
        (
            r#"if [ "$a" = 1 ]; then :; fi"#,
            r#"if [ "$a" == 1 ]; then :; fi"#,
            [3, 2, 1],
        ),
        (
            r#"if [[ ( "$a" = 1 ) ]]; then :; fi"#,
            r#"if [[ ( "$a" == 1 ) ]]; then :; fi"#,
            [3, 2, 1],
        ),
        (
            r#"if [[ ! ( "$a" = 1 ) ]]; then :; fi"#,
            r#"if [[ ! ( "$a" == 1 ) ]]; then :; fi"#,
            [3, 2, 1],
        ),
        ("(( x = 1 ))", "x=1", [2, 0, 0]),
        (
            r#"if [[ ( "$a" = 1 || "$b" = 2 ) && ( "$c" = 3 || "$d" = 4 ) ]]; then :; fi"#,
            r#"if [[ ( "$a" == 1 || "$b" == 2 ) && ( "$c" == 3 || "$d" == 4 ) ]]; then :; fi"#,
            [6, 5, 4],
        ),
    ];
    for (body, twin, decisions) in rows {
        assert_eq!(
            measure(twin),
            decisions,
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(
            measure(body),
            decisions,
            "`{body}` must score like `{twin}`"
        );
    }
}

/// An outer comparison run resumes after a nested one: past the `&&`
/// group, `"$c" = 3` continues the `||` run `"$b" = 2` restarted, so the
/// two are one sequence. The mis-nested tree scores a mixed run in
/// textual order (`ComparisonRuns`), one more than the `==` twin's 3, so
/// the value here is the one the ancestor climb `ComparisonRuns`
/// replaced scored; a tracker that forgot the outer run at the group
/// scores 5.
#[cfg(feature = "bash")]
#[test]
fn bash_comparison_run_resumes_after_a_nested_run_1536() {
    assert_eq!(
        measure(
            r#"if [[ "$a" = 1 || ( "$x" = 5 && "$y" = 6 ) || "$b" = 2 || "$c" = 3 || "$d" = 4 ]]; then :; fi"#
        ),
        [8, 7, 4]
    );
}

/// `=~` shares `=`'s precedence, so tree-sitter-bash mis-nests a run of
/// regex matches the same way (`"$a" =~ ($re || "$b") =~ …`), and the run
/// must stay one sequence, as its `==` twin's is. The other two rows pin
/// what the run tracking and the `=` count reproduce: a run that resumes
/// after a nested one keeps its sequences, and an assignment inside a
/// test's `$(( … ))` is no comparison.
#[cfg(feature = "bash")]
#[test]
fn bash_comparison_runs_score_like_their_twins_1536() {
    // `(body, twin, decisions)`; `twin` spells the same test another
    // way and must agree.
    let rows: [(&str, &str, Decisions); 3] = [
        (
            r#"if [[ "$a" =~ $re || "$b" =~ $re || "$c" =~ $re ]]; then :; fi"#,
            r#"if [[ "$a" == $re || "$b" == $re || "$c" == $re ]]; then :; fi"#,
            [5, 4, 2],
        ),
        (
            "if [[ ( $a = 1 || $b = 2 ) && $c = 3 || $d = 4 ]]; then :; fi",
            "if [[ ( $a == 1 || $b == 2 ) && $c == 3 || $d == 4 ]]; then :; fi",
            [6, 5, 4],
        ),
        (
            "if [[ $(( x = 1 )) -eq 1 ]]; then :; fi",
            "if [[ $(( x + 1 )) -eq 1 ]]; then :; fi",
            [3, 2, 1],
        ),
    ];
    for (body, twin, decisions) in rows {
        assert_eq!(
            measure(twin),
            decisions,
            "`{twin}` moved; re-derive the row"
        );
        assert_eq!(
            measure(body),
            decisions,
            "`{body}` must score like `{twin}`"
        );
    }
}
