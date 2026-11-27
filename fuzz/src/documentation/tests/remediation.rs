use super::*;
use crate::documentation::{
    identity,
    observation::{self, Event},
    properties,
};

#[test]
fn decoder_mode_table_is_independent() {
    use Mode::{ActiveScope, CancelledScope, IncludedRanges, Raw};
    let cases = [
        (0, Raw, false, false),
        (1, Raw, true, false),
        (2, Raw, false, true),
        (3, Raw, true, true),
        (4, IncludedRanges, false, false),
        (5, IncludedRanges, true, false),
        (6, IncludedRanges, false, true),
        (7, IncludedRanges, true, true),
        (8, CancelledScope, false, false),
        (9, CancelledScope, true, false),
        (10, CancelledScope, false, true),
        (11, CancelledScope, true, true),
        (12, ActiveScope, false, false),
        (13, ActiveScope, true, false),
        (14, ActiveScope, false, true),
        (15, ActiveScope, true, true),
    ];
    for (byte, mode, repeat, identity) in cases {
        let bytes = [0, byte, 0x61, 0xff, b'\r'];
        let input = Input::decode(&bytes).unwrap();
        assert_eq!(
            (input.mode, input.repeat, input.identity),
            (mode, repeat, identity)
        );
        assert_eq!(input.options.profile, Profile::Rust);
        assert!(input.options.retain_unbound && input.options.retain_directives);
        assert_eq!(input.source, &[0xff, b'\r']);
    }
    for (bytes, profile, language) in [
        ([16, 0, 14], Profile::Rdoc, LANG::Ruby),
        ([6, 0, 8], Profile::PhpDocumentor, LANG::Php),
    ] {
        assert_eq!(
            exercise(&bytes),
            Ok(Outcome::Rejected(CollectionError::adapter_unavailable(
                language, profile
            )))
        );
    }
}

fn operations(events: &[Event]) -> Vec<Event> {
    let subjects = events
        .iter()
        .filter(|event| matches!(event, Event::NodeProjection))
        .count();
    let queries = events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::OwnerQuery
                    | Event::NodeProjection
                    | Event::NodeOwners
                    | Event::NodeAssociations
            )
        })
        .count();
    assert!(subjects <= 4);
    assert!(queries <= 20, "query ceiling: {events:?}");
    events
        .iter()
        .copied()
        .filter(|event| {
            !matches!(
                event,
                Event::OwnerQuery
                    | Event::NodeProjection
                    | Event::NodeOwners
                    | Event::NodeAssociations
            )
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)] // Explicit independent outcomes for all sixteen mode combinations.
fn mode_calls_and_active_scope_are_observed() {
    use Event::{
        ActiveWitness, CloneRejected, Collection, EditRejected, IncludedParse, Parse, Repeat,
        ShiftRejected,
    };
    let success = Collection(Ok(()));
    let cancelled = Collection(Err(CollectionError::Cancelled));
    let payload = b"///audit\nfn f() {}";
    let cases = [
        (0, vec![Parse, success]),
        (1, vec![Parse, success, success, Repeat]),
        (
            2,
            vec![Parse, success, CloneRejected, ShiftRejected, EditRejected],
        ),
        (
            3,
            vec![
                Parse,
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
                success,
                Repeat,
            ],
        ),
        (4, vec![IncludedParse(18), success]),
        (5, vec![IncludedParse(18), success, success, Repeat]),
        (
            6,
            vec![
                IncludedParse(18),
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
            ],
        ),
        (
            7,
            vec![
                IncludedParse(18),
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
                success,
                Repeat,
            ],
        ),
        (8, vec![Parse, cancelled, success]),
        (9, vec![Parse, cancelled, success, success, Repeat]),
        (
            10,
            vec![
                Parse,
                cancelled,
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
            ],
        ),
        (
            11,
            vec![
                Parse,
                cancelled,
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
                success,
                Repeat,
            ],
        ),
        (12, vec![Parse, success, ActiveWitness]),
        (13, vec![Parse, success, success, Repeat, ActiveWitness]),
        (
            14,
            vec![
                Parse,
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
                ActiveWitness,
            ],
        ),
        (
            15,
            vec![
                Parse,
                success,
                CloneRejected,
                ShiftRejected,
                EditRejected,
                success,
                Repeat,
                ActiveWitness,
            ],
        ),
    ];
    for (mode, expected) in cases {
        let bytes = [vec![0, mode, 0], payload.to_vec()].concat();
        let (result, events) = observation::capture(|| {
            exercise_with(&bytes, |index, _| {
                let index = index.unwrap();
                assert_eq!(text(index).text(), "audit");
                assert_eq!(
                    index.associations().next().unwrap().get().resolution,
                    Resolution::Resolved
                );
                Ok(())
            })
        });
        assert_eq!(result, Ok(Outcome::Checked), "mode {mode}");
        assert_eq!(operations(&events), expected, "mode {mode}");
        assert_eq!(
            exercise(b"\0\0\0///audit\nfn f() {}"),
            Ok(Outcome::Checked),
            "restoration after {mode}"
        );
    }
}

#[test]
fn observation_scope_restores_on_rejection_skip_and_unwind() {
    for bytes in [&[25, 0, 0][..], &[0, 4, 0], &[0, 4, 0, 0]] {
        let (result, events) = observation::capture(|| exercise(bytes));
        assert!(matches!(result, Ok(Outcome::Skipped(_))));
        assert_eq!(events, []);
    }
    let (result, events) = observation::capture(|| exercise(&[1, 0, 0]));
    let error = CollectionError::adapter_unavailable(LANG::Python, Profile::Python);
    assert_eq!(result, Ok(Outcome::Rejected(error)));
    assert_eq!(events, [Event::Parse, Event::Collection(Err(error))]);
    let ((), outer) = observation::capture(|| {
        assert!(
            std::panic::catch_unwind(|| observation::capture(|| panic!("recorder unwind control")))
                .is_err()
        );
        exercise(&[1, 0, 0]).unwrap();
    });
    assert_eq!(outer, events);
}

#[test]
fn evidence_validation_rejects_same_id_changed_context_and_wrong_document() {
    let parser = AnyParser::parse(
        LANG::Cpp,
        b"/**one*/\nvoid f();\n/**two*/\nvoid g();".to_vec(),
        None,
        None,
    )
    .unwrap();
    let index = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let documents: Vec<_> = index.documents().collect();
    assert_eq!(documents.len(), 2);
    let node = index
        .facts()
        .document_fragment(documents[0], 0)
        .unwrap()
        .get()
        .node()
        .unwrap();
    let mut references = identity::References::default();
    references.insert(documents[0], node);
    assert_eq!(references.validate(documents[0], node), Ok(()));
    assert_eq!(
        references.validate(documents[1], node),
        Err("qualification reference evidence is not a document fragment")
    );
    let mut edited = node.node();
    edited.edit(&tree_sitter::InputEdit {
        start_byte: 0,
        old_end_byte: 0,
        new_end_byte: 1,
        start_position: tree_sitter::Point::new(0, 0),
        old_end_position: tree_sitter::Point::new(0, 0),
        new_end_position: tree_sitter::Point::new(0, 1),
    });
    assert_eq!(node.node().id(), edited.id());
    let edited = SourceContext::new(parser.ts_tree(), parser.code())
        .node(edited)
        .unwrap();
    let owner = index
        .facts()
        .associated_owner(index.associations().next().unwrap(), 0)
        .unwrap();
    let Owner::Node(owner_node) = owner.get() else {
        panic!("real declaration owner")
    };
    assert_eq!(identity::candidate(owner.get(), *owner_node), Ok(()));
    assert_eq!(
        identity::candidate(owner.get(), edited),
        Err("qualification candidate evidence differs from owner")
    );
    let root = SourceContext::new(parser.ts_tree(), parser.code())
        .node(parser.ts_tree().root_node())
        .unwrap();
    assert_eq!(
        identity::candidate(owner.get(), root),
        Err("qualification candidate evidence differs from owner")
    );
    let file = index
        .facts()
        .owners()
        .find(|owner| matches!(owner.get(), Owner::File(_)))
        .unwrap();
    assert_eq!(identity::candidate(file.get(), root), Ok(()));
    assert_eq!(
        references.validate(documents[0], edited),
        Err("qualification reference evidence is not a document fragment")
    );
    assert_eq!(
        identity::evidence(Some(node), Some(edited)),
        Err("nondeterministic node evidence")
    );
    // The ID bucket must retain distinct contexts rather than overwrite one.
    references.insert(documents[0], edited);
    assert_eq!(references.validate(documents[0], node), Ok(()));
    assert_eq!(references.validate(documents[0], edited), Ok(()));
}

#[test]
fn inherited_512_construct_source_still_reaches_documentation() {
    use arbitrary::{Arbitrary, Unstructured};
    let nesting =
        crate::nested::Nesting::arbitrary(&mut Unstructured::new(&[0, 255, 1, 0])).unwrap();
    let source = nesting.render();
    assert_eq!(source.len(), 1050);
    let ast = Ast::parse(big_code_analysis::Source::from_bytes(LANG::Rust, source)).unwrap();
    assert!(!ast.as_tree_sitter().root_node().has_error());
    let (result, events) = observation::capture(|| check_ast(&ast));
    assert_eq!(result, Ok(Outcome::Checked));
    assert_eq!(operations(&events), [Event::Collection(Ok(()))]);
}

#[test]
fn producer_qualification_runtime_and_registry_controls() {
    let mutation = std::env::var("BCA_FUZZ_PRODUCER_CASE").unwrap_or_default();
    for (name, class) in [
        ("cpp-candidate-qualification", "candidate-qualification"),
        ("cpp-reference-qualification", "reference-qualification"),
    ] {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("corpus/documentation")
                .join(name),
        )
        .unwrap();
        let input = Input::decode(&bytes).unwrap();
        let parser = AnyParser::parse(input.language, input.source.to_vec(), None, None).unwrap();
        let index = parser
            .documentation(&DocumentationContext::default(), &input.options)
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(index.associations().len(), 1);
        let candidate = class == "candidate-qualification";
        assert_eq!(
            index.associations().next().unwrap().get().owner_count(),
            if candidate { 2 } else { 1 }
        );
        let affected = mutation.starts_with(if candidate {
            "candidate-"
        } else {
            "reference-"
        });
        let expected = if affected && mutation.ends_with("root") {
            Err(if candidate {
                "qualification candidate evidence differs from owner"
            } else {
                "qualification reference evidence is not a document fragment"
            })
        } else {
            Ok(Outcome::Checked)
        };
        assert_eq!(exercise(&bytes), expected, "runtime {name} {mutation}");
        let registry = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            edge(class, Some(&index), parser.ts_tree());
        }));
        assert_eq!(registry.is_err(), affected, "registry {name} {mutation}");
        if let Err(panic) = registry {
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("");
            let expected = if mutation.ends_with("resolution") {
                "seed association resolution"
            } else if candidate {
                "candidate source evidence"
            } else {
                "reference source evidence"
            };
            assert!(
                message.contains(expected),
                "wrong registry rejection: {message}"
            );
        }
    }
}

#[test]
fn producer_second_collection_repeat_control() {
    let mutation = std::env::var("BCA_FUZZ_PRODUCER_CASE").unwrap_or_default();
    let source = if mutation == "repeat-binding" || mutation == "repeat-scope" {
        b"/**audit*/\nexport function f() {}".as_slice()
    } else {
        b"/**audit*/\nint a, b;".as_slice()
    };
    let language = if mutation == "repeat-binding" || mutation == "repeat-scope" {
        LANG::Javascript
    } else {
        LANG::C
    };
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(!parser.ts_tree().root_node().has_error());
    let first = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    assert_eq!(
        properties::check(&first, parser.ts_tree(), parser.code()),
        Ok(())
    );
    assert_eq!(text(&first).text(), "audit");
    if language == LANG::C {
        edge("shared", Some(&first), parser.ts_tree());
    } else {
        let association = first.associations().next().unwrap();
        assert_eq!(
            association.get().binding.unwrap().node().kind(),
            "export_statement"
        );
        assert_eq!(association.get().lexical_scope.node().kind(), "program");
    }
    let second = parser
        .documentation(
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
    let expected = match mutation.as_str() {
        "repeat-slot" => Err("nondeterministic slot fields"),
        "repeat-binding" | "repeat-scope" => Err("nondeterministic node evidence"),
        _ => Ok(()),
    };
    assert_eq!(
        properties::repeat(&first, &second),
        expected,
        "second collection {mutation}"
    );
}

#[test]
fn repeat_checks_nondefault_context_and_options() {
    let parser = AnyParser::parse(LANG::Rust, b"///audit\nfn f() {}".to_vec(), None, None).unwrap();
    let context = DocumentationContext::default().with_logical_filename("named.rs");
    let options = DocumentationOptions::default()
        .with_profile(Profile::Rust)
        .with_unbound_evidence(true)
        .with_directive_evidence(true);
    let first = parser.documentation(&context, &options).unwrap();
    for (context, options) in [
        (DocumentationContext::default(), options),
        (context.clone(), options.with_profile(Profile::Default)),
        (context.clone(), options.with_unbound_evidence(false)),
        (context.clone(), options.with_directive_evidence(false)),
    ] {
        let second = parser.documentation(&context, &options).unwrap();
        assert_eq!(
            properties::repeat(&first, &second),
            Err("nondeterministic index context/options")
        );
    }
    let second = parser.documentation(&context, &options).unwrap();
    assert_eq!(properties::repeat(&first, &second), Ok(()));
}
