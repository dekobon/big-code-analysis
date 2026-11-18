use super::super::streaming::{
    CoverageKind, LegacyEvent,
    evidence::{Question, Request, SelectionKey, View},
};
use super::*;

#[test]
fn extra_evidence_is_lazy_and_unbound_retention_is_explicit() {
    let builds = std::cell::Cell::new(0);
    let build = || {
        builds.set(builds.get() + 1);
        7
    };
    let mut legacy = Request::LegacyOnly;
    legacy.emit(true, build);
    legacy.emit(false, build);
    assert_eq!(builds.get(), 0);
    let mut events = Vec::new();
    let mut sink = |event| events.push(event);
    let mut source = Request::Source {
        retain_unbound: false,
        sink: &mut sink,
    };
    source.emit(false, build);
    assert_eq!(builds.get(), 0);
    source.emit(true, build);
    assert_eq!(builds.get(), 1);
    let mut evidence = Request::Source {
        retain_unbound: true,
        sink: &mut sink,
    };
    evidence.emit(false, build);
    assert_eq!(builds.get(), 2);
    assert_eq!(events, [7, 7]);
}

#[cfg(feature = "java")]
#[test]
fn selection_identity_includes_view_profile_question_and_canonical_context() {
    let parser = AnyParser::parse(LANG::Java, b"/**/\nclass C {}".to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let node = context.node(parser.ts_tree().root_node()).unwrap();
    let legacy = SelectionKey {
        node,
        view: View::Legacy,
        question: Question::Candidate,
    };
    let same = SelectionKey {
        node,
        view: View::Legacy,
        question: Question::Candidate,
    };
    assert_eq!(legacy, same);
    assert_ne!(
        legacy,
        SelectionKey {
            node,
            view: View::Source(Convention("Jdk25")),
            question: Question::Candidate
        }
    );
    assert_ne!(
        legacy,
        SelectionKey {
            node,
            view: View::Legacy,
            question: Question::Placement
        }
    );
    let source = SelectionKey {
        node,
        view: View::Source(Convention("Jdk25")),
        question: Question::Binding,
    };
    assert_ne!(
        source,
        SelectionKey {
            node,
            view: View::Source(Convention("BcaOther")),
            question: Question::Binding
        }
    );
    let shifted = context
        .node(
            parser
                .ts_tree()
                .root_node_with_offset(2, tree_sitter::Point::new(1, 0)),
        )
        .unwrap();
    assert_ne!(
        legacy,
        SelectionKey {
            node: shifted,
            view: View::Legacy,
            question: Question::Candidate
        }
    );
}

#[cfg(feature = "java")]
#[test]
fn legacy_events_preserve_marker_vs_placement_and_inclusive_coverage_order() {
    let parser = AnyParser::parse(LANG::Java, b"/**/\nclass C {}".to_vec(), None, None).unwrap();
    let node = parser.root_node().children().next().unwrap();
    let events = [
        LegacyEvent::Comment {
            node,
            marker: false,
            by_placement: true,
        },
        LegacyEvent::BoundComment(node),
        LegacyEvent::Coverage {
            kind: CoverageKind::PythonStringRun,
            first: 3,
            last: 7,
        },
    ];
    assert!(matches!(
        events[0],
        LegacyEvent::Comment {
            marker: false,
            by_placement: true,
            ..
        }
    ));
    let LegacyEvent::BoundComment(bound) = events[1] else {
        panic!("bound event");
    };
    assert_eq!(bound.as_tree_sitter().byte_range(), 0..4);
    assert!(matches!(
        events[2],
        LegacyEvent::Coverage {
            kind: CoverageKind::PythonStringRun,
            first: 3,
            last: 7
        }
    ));
}
