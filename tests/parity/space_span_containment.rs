//! Cross-language invariant: a space's reported line span lies inside
//! its parent's, and the file-level `Unit` ends on the file's last line.
//!
//! Both walks turn tree-sitter's 0-based end row into a 1-based end line
//! in `spaces::line_span`. Getting that conversion wrong produces a tree
//! that still serializes, still carries correct metrics, and still
//! passes the ops/metrics parity check next door — both walks share the
//! arithmetic, so they agree on the *same* wrong number. What it breaks
//! is anything that slices source by a span: `bca check`'s offender
//! lines, the SARIF `region`, an editor integration.
//!
//! #1163 is the instance. `line_span` keyed the `+ 1` on
//! `SpaceKind::Unit` rather than on the node's end column, so Perl — the
//! one grammar whose trailing `sub` ends at column 0 of the row *below*
//! its closing brace, exactly where the root ends — reported
//! `unit 1..4, function 1..5` for a four-line file: a child extending
//! past its parent and past EOF, the shape that produced the release
//! `usize` underflow in #1051.
//!
//! Neither claim needs to know the right span per grammar, which is what
//! makes them checkable across every language rather than the handful
//! anyone thought to write a fixture for.

use big_code_analysis::{Ast, LANG, MetricsOptions, Source, SpaceKind, analyze};

use super::ops_metrics_space_parity::{SpaceTree, fixture};

/// Lines in `source`, counting the way an editor does: a trailing
/// newline terminates the last line rather than opening an empty one.
fn line_count(source: &str) -> usize {
    source.lines().count()
}

/// Asserts the containment invariant over one space subtree, returning
/// the number of spaces visited so the caller can rule out a vacuous
/// pass.
fn check_containment<T: SpaceTree>(
    lang: LANG,
    walk: &str,
    node: &T,
    parent: Option<(Option<&str>, SpaceKind, usize, usize)>,
) -> usize {
    let (name, kind, start, end) = node.describe();

    assert!(
        start <= end,
        "{lang:?}/{walk}: {kind:?} {name:?} reports an inverted span {start}..{end}",
    );

    if let Some((parent_name, parent_kind, parent_start, parent_end)) = parent {
        assert!(
            start >= parent_start,
            "{lang:?}/{walk}: {kind:?} {name:?} starts at line {start}, before its \
             parent {parent_kind:?} {parent_name:?} at {parent_start}",
        );
        assert!(
            end <= parent_end,
            "{lang:?}/{walk}: {kind:?} {name:?} ends at line {end}, past its parent \
             {parent_kind:?} {parent_name:?} which ends at {parent_end}",
        );
    }

    let mut visited = 1;
    for child in node.children() {
        visited += check_containment(lang, walk, child, Some((name, kind, start, end)));
    }
    visited
}

/// Asserts both invariants over both walks for one source, and returns
/// the number of spaces the metrics walk produced.
fn check_source(lang: LANG, source: &str, ext: &str) -> usize {
    let name = format!("span.{ext}");

    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name.clone())),
        MetricsOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
    let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name)))
        .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
        .ops()
        .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));

    let visited = check_containment(lang, "metrics", &space, None);
    check_containment(lang, "ops", &ops, None);

    let lines = line_count(source);
    for (walk, (start, end)) in [
        ("metrics", (space.start_line, space.end_line)),
        ("ops", (ops.start_line, ops.end_line)),
    ] {
        assert_eq!(
            (start, end),
            (1, lines),
            "{lang:?}/{walk}: the file-level unit reports lines {start}..{end} for a \
             {lines}-line file",
        );
    }

    visited
}

#[test]
fn every_space_lies_within_its_parent_in_every_language() {
    let mut checked = 0;

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;

        let (source, ext) = fixture(lang);
        let visited = check_source(lang, source, ext);

        // Containment holds vacuously over a one-node tree, and
        // `Ccomment` / `Preproc` genuinely have no function-space concept
        // (see the fixture table's comment on those two arms). Everything
        // else must open at least one nested space or this language is
        // not actually being tested.
        if !matches!(lang, LANG::Ccomment | LANG::Preproc) {
            assert!(
                visited > 1,
                "{lang:?}: fixture opens no nested space, so the containment \
                 assertions above hold vacuously",
            );
        }
    }

    // Every language is feature-gated, so a build with none enabled
    // leaves a zero-iteration loop and a test that reports green while
    // asserting nothing.
    assert!(
        checked > 0,
        "at least one language feature must be enabled for this test to mean anything"
    );
}

/// The #1163 reproducer and its two controls, with the spans spelled out
/// rather than only checked for containment — the invariant above says
/// the tree is *representable*, these say it is *right*.
///
/// The three differ only in whether the `sub` is the last thing in the
/// file and whether the file ends in a newline, which is precisely what
/// decides the `function_definition`'s end column and therefore which
/// branch of `line_span` runs.
///
/// These reach `analyze(Source::new(..))` directly, so the source is
/// analysed byte-for-byte. Do not route them through a helper that
/// regularises the trailing newline (`.claude/rules/testing.md`): the
/// third fixture *is* the missing-newline case, and appending one turns
/// it into the first.
#[test]
#[cfg(feature = "perl")]
fn perl_sub_spans_stay_inside_the_unit() {
    // A `sub` that is the last thing in the file: its
    // `function_definition` absorbs the trailing newline and ends at
    // (row 4, column 0) — the same position as the root. Before #1163
    // this reported `unit 1..4, function 1..5`.
    let sub_last = "sub f {\n    my ($a) = @_;\n    return $a + 1;\n}\n";
    // The same file with no trailing newline. The root and the sub both
    // end at (row 3, column 1) here, so the unit moves too: before
    // #1163 this reported `unit 1..3, function 1..4`.
    let no_trailing_newline = "sub f {\n    my ($a) = @_;\n    return $a + 1;\n}";
    // A statement after the sub. The `function_definition` now ends
    // mid-row at (row 3, column 1) like every other language's, so this
    // case was already correct and must stay unchanged.
    let sub_not_last = "sub f {\n    my ($a) = @_;\n    return $a + 1;\n}\n\nmy $x = f(1);\n";

    // Collected and compared in one shot rather than asserted per case:
    // all three shapes move under a wrong `line_span`, and a per-case
    // assertion would panic on the first and hide whether the other two
    // are right.
    let measured: Vec<_> = [
        ("sub last", sub_last),
        ("no trailing newline", no_trailing_newline),
        ("sub not last", sub_not_last),
    ]
    .into_iter()
    .map(|(label, source)| {
        let space = analyze(
            Source::new(LANG::Perl, source.as_bytes()).with_name(Some("span.pl".to_owned())),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{label}: analyze failed: {e}"));

        assert_eq!(space.kind, SpaceKind::Unit, "{label}: root is not the unit");
        let [sub] = space.spaces.as_slice() else {
            panic!(
                "{label}: expected exactly one nested space, got {}",
                space.spaces.len()
            );
        };
        assert_eq!(
            (sub.kind, sub.name.as_deref()),
            (SpaceKind::Function, Some("f")),
            "{label}: nested space is not the `sub`",
        );

        // `Ast::functions` (behind `bca functions`, and the web
        // `/function` endpoint) walks the AST separately from `analyze`
        // and applied the same blanket `+ 1`. Asserted here rather than
        // in its own test so a future divergence between the two spans
        // for one source shows up as a diff on one line.
        let spans = Ast::parse(Source::new(LANG::Perl, source.as_bytes()))
            .unwrap_or_else(|e| panic!("{label}: parse failed: {e}"))
            .functions();
        let [span] = spans.as_slice() else {
            panic!("{label}: expected exactly one function span, got {spans:?}");
        };

        format!(
            "{label}: unit {}..{}, function {}..{}, functions() {}..{}",
            space.start_line,
            space.end_line,
            sub.start_line,
            sub.end_line,
            span.start_line,
            span.end_line,
        )
    })
    .collect();

    assert_eq!(
        measured,
        [
            "sub last: unit 1..4, function 1..4, functions() 1..4",
            "no trailing newline: unit 1..4, function 1..4, functions() 1..4",
            "sub not last: unit 1..6, function 1..4, functions() 1..4",
        ],
    );
}
