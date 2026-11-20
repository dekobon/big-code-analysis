use super::*;
use big_code_analysis_ast::LANG;
use big_code_analysis_ast::documentation::*;

#[test]
fn recovery_and_conditional_semantics_reject_shape_preserving_substitutions() {
    for key in ["recovery-depth", "conditional-depth"] {
        let probe = REGISTRY.iter().find(|p| p.key == key).unwrap();
        for size in [4, 32] {
            let source = String::from_utf8(probe.source(size).unwrap()).unwrap();
            for replacement in ["wrong", "@file", "@fn absent"] {
                let changed = source.replace("audit", replacement);
                let parsed = Parsed::Ordinary(
                    big_code_analysis::Ast::parse(big_code_analysis::Source::new(
                        probe.language,
                        changed.as_bytes(),
                    ))
                    .unwrap(),
                );
                let index = parsed.collect(probe).unwrap();
                let nodes = parsed.nodes();
                let count = |kind| nodes.iter().filter(|(n, _)| n.kind() == kind).count();
                // Pin the original weaker conditions before requiring the new
                // text/attachment oracle to reject each counterexample.
                assert_eq!(
                    parsed.tree().root_node().has_error(),
                    key == "recovery-depth"
                );
                assert_eq!(count("struct_specifier"), size);
                if key == "recovery-depth" {
                    assert_eq!(count("field_declaration"), size + 31);
                    assert!(
                        nodes
                            .iter()
                            .any(|(n, depth)| n.kind() == "field_declaration_list"
                                && n.has_error()
                                && n.child_count() >= 64
                                && *depth >= size)
                    );
                    assert_eq!(index.documents().len(), 32);
                } else {
                    assert_eq!(count("preproc_if"), size);
                    assert_eq!(index.documents().len(), 1);
                }
                let Err(error) = Prepared::new(probe, size, &parsed, &index) else {
                    panic!("accepted {key}/{size}/{replacement}")
                };
                let expected = match replacement {
                    "wrong" => "exact source piece range/bytes",
                    "@file" if key == "recovery-depth" => "combined fixture binding/scope identity",
                    "@file" => "combined fixture resolution/owner count",
                    "@fn absent" if key == "conditional-depth" => "direct attachment must have Q=0",
                    _ => "combined fixture resolution/owner count",
                };
                assert_eq!(error.0, expected, "{key}/{size}/{replacement}");
            }
        }
    }
}

#[test]
fn original_registry_ids_sizes_and_options_are_preserved() {
    assert_eq!(
        crate::shapes::PROBES.len() + crate::vcs::VCS_PROBES.len(),
        140
    );
    let expected = [
        "documentation/cold/depth",
        "documentation/cold/width",
        "documentation/cold/fragments",
        "documentation/cold/single-line-lua",
        "documentation/cold/single-line-jsdoc",
        "documentation/cold/c-slots",
        "documentation/cold/wide-error",
        "documentation/cold/missing",
        "documentation/cold/names",
        "documentation/secondary/single-line-php",
        "documentation/secondary/single-line-ruby",
        "documentation/secondary/single-line-doxygen",
        "documentation/secondary/text-fixed-fanout",
        "documentation/secondary/fanout-fixed-text",
        "documentation/secondary/visible-declarator-depth",
        "documentation/secondary/ruby-modifiers",
        "documentation/secondary/java-undocumented",
        "documentation/secondary/pending-scope",
        "documentation/prepared/owner",
        "documentation/prepared/late-map",
        "documentation/prepared/retained",
        "documentation/prepared/late-owner",
        "documentation/ranges/cold",
        "documentation/remediation/inline-spans",
        "documentation/remediation/gathered-disposition",
        "documentation/remediation/complex-signatures",
        "documentation/remediation/order-multiple-chunks",
        "documentation/remediation/text-budget-boundary",
        "documentation/remediation/go-fold",
        "documentation/remediation/ruby-fold",
        "documentation/remediation/multiline-adjacent-ranges",
    ];
    assert_eq!(
        REGISTRY[..31].iter().map(|p| p.id).collect::<Vec<_>>(),
        expected
    );
    for (at, probe) in REGISTRY[..31].iter().enumerate() {
        assert_eq!(
            probe.sizes,
            if at < 23 {
                [256, 512, 1024]
            } else {
                [1024, 2048, 4096]
            }
        );
        let options = probe.options();
        assert_eq!(options.profile, Profile::Default);
        assert!(!options.retain_unbound && !options.retain_directives);
    }
    assert_eq!(REGISTRY.len() - 31, 15);
}

#[test]
fn declared_growth_axes_match_full_size_input_and_output_counts() {
    for probe in REGISTRY {
        let counts: Vec<_> = probe
            .sizes
            .into_iter()
            .map(|size| {
                let parsed = Parsed::new(probe, size).unwrap();
                let index = parsed.collect(probe).unwrap();
                Prepared::new(probe, size, &parsed, &index)
                    .unwrap_or_else(|e| panic!("{} size {size}: {e}", probe.id))
                    .counts()
            })
            .collect();
        for (pair, sizes) in counts.windows(2).zip(probe.sizes.windows(2)) {
            if probe.class == StatisticClass::FixedOutputRatio
                || probe.class == StatisticClass::LogMapRatio
            {
                assert!(pair[0].query_output > 0, "{}", probe.id);
                assert_eq!(pair[0].query_output, pair[1].query_output, "{}", probe.id);
            }
            if matches!(
                probe.key,
                "width" | "qualified-unrelated" | "qualified-candidates" | "qualified-references"
            ) {
                assert_eq!(
                    pair[0].depth, pair[1].depth,
                    "{}: fixed lexical depth",
                    probe.id
                );
            }
            if matches!(
                probe.key,
                "wide-depth"
                    | "pending-ancestry"
                    | "pending-late"
                    | "conditional-depth"
                    | "recovery-depth"
            ) {
                assert!(
                    pair[1].depth >= pair[0].depth + sizes[0],
                    "{}: growing lexical depth",
                    probe.id
                );
            }
            match probe.key {
                "qualified-candidates" | "qualified-unrelated" => {
                    assert_eq!(pair[1].qualifications, pair[0].qualifications * 2);
                    assert_eq!(pair[1].edges, pair[0].edges * 2);
                }
                "qualified-references" => {
                    assert_eq!(pair[1].qualifications, pair[1].documents * 2);
                    assert_eq!(pair[1].edges, pair[1].documents * 2);
                }
                "wide-depth" => {
                    assert_eq!(pair[0].width, pair[1].width);
                    assert_eq!(pair[0].documents, 32);
                    assert_eq!(pair[1].documents, 32);
                }
                _ => {}
            }
        }
    }
}

#[test]
fn prepared_query_counters_charge_only_declared_output() {
    use fixtures::Operation;
    for size in [32, 64, 128] {
        for probe in REGISTRY.iter().filter(|p| {
            !matches!(
                p.operation,
                Operation::Collect | Operation::Render | Operation::EvidencePair
            )
        }) {
            let parsed = Parsed::new(probe, size).unwrap();
            let index = parsed.collect(probe).unwrap();
            let prepared = Prepared::new(probe, size, &parsed, &index).unwrap();
            let before = observations();
            prepared.execute().unwrap();
            let after = observations();
            assert_eq!(after.renders - before.renders, 0, "{}", probe.id);
            assert_eq!(after.copy_chunks - before.copy_chunks, 0, "{}", probe.id);
            assert_eq!(after.visits - before.visits, 0, "{}", probe.id);
            assert_eq!(after.memo_probes - before.memo_probes, 0, "{}", probe.id);
            assert_eq!(
                after.fold_slot_probes - before.fold_slot_probes,
                0,
                "{}",
                probe.id
            );
            assert_eq!(
                after.sibling_slot_probes - before.sibling_slot_probes,
                0,
                "{}",
                probe.id
            );
            match probe.operation {
                Operation::Owner => {
                    assert_eq!(after.association_reads - before.association_reads, 1);
                    assert_eq!(after.identity_probes - before.identity_probes, 1);
                }
                Operation::OwnerQualifications | Operation::DocumentQualifications => {
                    assert_eq!(
                        after.qualification_reads - before.qualification_reads,
                        1,
                        "{}",
                        probe.id
                    );
                    assert_eq!(after.association_reads - before.association_reads, 0);
                }
                Operation::Pending => {
                    // Two records plus one exhausted-scope read per populated
                    // ancestor, and the final None read; the query starts at leaf.
                    let expected = if probe.key == "pending-ancestry" {
                        3 * size + 1
                    } else {
                        4
                    };
                    assert_eq!(
                        after.assessment_scope_reads - before.assessment_scope_reads,
                        expected,
                        "{}",
                        probe.id
                    );
                }
                Operation::Map => {
                    assert_eq!(after.map_runs_visited - before.map_runs_visited, 1);
                    // Rust's slice partition_point uses floor(log2(S)) + 2
                    // comparisons for this non-power-of-two S=2n-1 ladder.
                    assert_eq!(
                        after.map_searches - before.map_searches,
                        (2 * size - 1).ilog2() as usize + 2
                    );
                }
                Operation::Retained => {
                    assert_eq!(after.association_reads - before.association_reads, 0);
                    assert_eq!(after.map_runs_visited - before.map_runs_visited, 0);
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn added_collection_axes_charge_actual_visits_and_qualification_memberships() {
    for size in [32, 64, 128] {
        for probe in &REGISTRY[31..] {
            let parsed = Parsed::new(probe, size).unwrap();
            let nodes = parsed.nodes().len();
            let before = observations();
            let index = parsed.collect(probe).unwrap();
            let after = observations();
            assert_eq!(after.visits - before.visits, nodes, "{}", probe.id);
            assert_eq!(
                after.child_batches - before.child_batches,
                nodes,
                "{}",
                probe.id
            );
            assert_eq!(
                after.target_classifications - before.target_classifications,
                nodes - 1,
                "{}",
                probe.id
            );
            assert_eq!(
                after.target_computations - before.target_computations,
                nodes - 1,
                "{}",
                probe.id
            );
            if probe.key.starts_with("qualified-") {
                let q = if probe.key == "qualified-references" {
                    2 * size
                } else {
                    size
                };
                // Each candidate fact is indexed by document, owner and node.
                assert_eq!(
                    after.qualification_memberships - before.qualification_memberships,
                    3 * q
                );
                assert_eq!(index.qualifications().len(), q);
                if probe.key == "qualified-candidates" {
                    // Per candidate: name key 3, two signature scans 24,
                    // signature hash/copy 30. Repeated keys compare 16 bytes;
                    // the single `f` reference scans/hashes/compares 5 bytes.
                    assert_eq!(after.name_bytes - before.name_bytes, 73 * size - 11);
                    assert_eq!(after.key_bytes_stored - before.key_bytes_stored, 16);
                    assert_eq!(after.name_candidates - before.name_candidates, 2 * size - 1);
                }
            }
        }
    }
}

#[test]
fn preserved_scope_folds_and_field_axes_charge_search_work() {
    for size in [32, 64, 128] {
        for (key, slots) in [
            ("go-fold", 4 * (size / 16) + 5),
            ("ruby-fold", 4 * (size / 16) - 1),
        ] {
            let probe = REGISTRY.iter().find(|p| p.key == key).unwrap();
            let parsed = Parsed::new(probe, size).unwrap();
            let before = observations();
            let index = parsed.collect(probe).unwrap();
            assert_eq!(
                observations().fold_slot_probes - before.fold_slot_probes,
                slots,
                "{key}"
            );
            assert_eq!(index.documents().len(), usize::from(key == "ruby-fold"));
        }
        let probe = REGISTRY
            .iter()
            .find(|p| p.key == "java-undocumented")
            .unwrap();
        let parsed = Parsed::new(probe, size).unwrap();
        let before = observations();
        let index = parsed.collect(probe).unwrap();
        assert_eq!(
            observations().memo_probes - before.memo_probes,
            parsed.nodes().len() - 1
        );
        assert_eq!(observations().field_lookups - before.field_lookups, 0);
        assert_eq!(index.facts().owners().len(), size + 2);
        let probe = REGISTRY
            .iter()
            .find(|p| p.key == "visible-declarator-depth")
            .unwrap();
        let parsed = Parsed::new(probe, size).unwrap();
        let before = observations();
        let index = parsed.collect(probe).unwrap();
        // Four children per visible array declarator, three on the declaration.
        assert_eq!(
            observations().field_requests - before.field_requests,
            4 * size + 3
        );
        assert_eq!(index.documents().len(), 1);
    }
}

#[test]
fn documented_wide_batch_slot_search_is_independent_of_lexical_depth() {
    let probe = REGISTRY.iter().find(|p| p.key == "wide-depth").unwrap();
    let mut expected = None;
    for depth in [32, 64, 128] {
        let parsed = Parsed::new(probe, depth).unwrap();
        let before = observations();
        let index = parsed.collect(probe).unwrap();
        let reads = observations().sibling_slot_probes - before.sibling_slot_probes;
        assert_eq!(index.documents().len(), 32);
        assert!(
            reads > 0,
            "wide documented parent must exercise recorded sibling slots"
        );
        if let Some(expected) = expected {
            assert_eq!(reads, expected);
        } else {
            expected = Some(reads);
        }
    }
}

#[test]
fn fresh_render_axes_build_once_and_fanout_keeps_one_text() {
    for size in [32, 64, 128] {
        for probe in REGISTRY
            .iter()
            .filter(|p| p.operation == fixtures::Operation::Render)
        {
            let parsed = Parsed::new(probe, size).unwrap();
            let index = parsed.collect(probe).unwrap();
            let prepared = Prepared::new(probe, size, &parsed, &index).unwrap();
            let before = observations();
            prepared.execute().unwrap();
            let after = observations();
            assert_eq!(after.renders - before.renders, 1);
            assert_eq!(
                after.copy_chunks - before.copy_chunks,
                if probe.key == "prepared" {
                    2 * size - 1
                } else {
                    1
                }
            );
            assert_eq!(
                after.intersection_searches - before.intersection_searches,
                0
            );
            assert_eq!(after.visits - before.visits, 0);
            assert_eq!(index.documents().len(), 1);
            assert_eq!(
                index.associations().next().unwrap().get().owner_count(),
                if probe.language == LANG::C { 2 } else { 1 }
            );
        }
    }
}

#[test]
fn large_fresh_render_preserves_bounded_copy_work() {
    let probe = REGISTRY
        .iter()
        .find(|p| p.id == "documentation/render/bytes")
        .unwrap();
    for size in probe.sizes {
        let parsed = Parsed::new(probe, size).unwrap();
        let index = parsed.collect(probe).unwrap();
        let prepared = Prepared::new(probe, size, &parsed, &index).unwrap();
        let before = observations();
        prepared.execute().unwrap();
        let after = observations();
        assert_eq!(after.renders - before.renders, 1);
        assert_eq!(
            after.copy_chunks - before.copy_chunks,
            (6 * size - 1).div_ceil(16 * 1024)
        );
        assert_eq!(prepared.counts().rendered_bytes, 6 * size - 1);
        assert_eq!(prepared.counts().map_segments, 1);
        assert_eq!(prepared.counts().edges, 2);
    }
}

#[test]
fn evidence_pair_executes_both_off_and_on_collections() {
    use big_code_analysis_ast::documentation::streaming::{markers, positional};

    for probe in REGISTRY
        .iter()
        .filter(|p| p.operation == fixtures::Operation::EvidencePair)
    {
        let parsed = Parsed::new(probe, 32).unwrap();
        let index = parsed.collect(probe).unwrap();
        let prepared = Prepared::new(probe, 32, &parsed, &index).unwrap();
        let before = observations();
        let marker_evidence = markers::evidence_built::observed();
        let positional_evidence = positional::evidence_built::observed();
        prepared.execute().unwrap();
        let after = observations();
        assert_eq!(after.visits - before.visits, 2 * parsed.nodes().len());
        assert_eq!(
            after.child_batches - before.child_batches,
            2 * parsed.nodes().len()
        );
        assert_eq!(
            after.renders - before.renders,
            if probe.key == "rejected" { 2 } else { 0 }
        );
        // Off/on materializes one selected Java member plus all 32 members,
        // or zero Go directives plus all 32 directives.
        assert_eq!(
            markers::evidence_built::observed() - marker_evidence,
            if probe.key == "rejected" { 33 } else { 0 }
        );
        assert_eq!(
            positional::evidence_built::observed() - positional_evidence,
            if probe.key == "directives" { 32 } else { 0 }
        );
    }
}

#[test]
fn new_generated_names_keep_affine_bytes() {
    for key in ["qualified-unrelated", "missing-name"] {
        let probe = REGISTRY.iter().find(|p| p.key == key).unwrap();
        let [a, b, c] = probe.sizes.map(|size| probe.source(size).unwrap().len());
        assert_eq!(c - b, 2 * (b - a), "{key}");
    }
}

#[test]
fn missing_name_source_retains_distinct_keys_without_owner_edges() {
    let probe = REGISTRY.iter().find(|p| p.key == "missing-name").unwrap();
    for size in [32, 64, 128] {
        let parsed = Parsed::new(probe, size).unwrap();
        let before = observations();
        let index = parsed.collect(probe).unwrap();
        let after = observations();
        // Each fixed-width f00000000 name stores nine bytes; its normalized
        // void\0f00000000\0(\0void\0) signature stores 23, regardless of bucket collisions.
        assert_eq!(after.key_bytes_stored - before.key_bytes_stored, 32 * size);
        assert_eq!(index.associations().len(), 1);
        let association = index.associations().next().unwrap();
        assert_eq!(
            association.get().resolution,
            Resolution::Unresolved(UnresolvedReason::MissingName)
        );
        assert_eq!(association.get().owner_count(), 0);
    }
}

#[test]
fn candidate_reference_overlap_and_unknown_signature_controls_are_distinct() {
    for (source, subject, reasons) in [
        (
            "#if FLAG\nvoid f(int x = );\n#endif\nvoid f(void);\n/**@fn f*/",
            QualificationSubject::Candidate,
            vec![
                UnresolvedReason::Conditional,
                UnresolvedReason::GrammarBoundary,
            ],
        ),
        (
            "void f(void);\n#if FLAG\n/**@fn f*/\n#endif",
            QualificationSubject::Reference,
            vec![UnresolvedReason::Conditional],
        ),
    ] {
        let ast = big_code_analysis::Ast::parse(big_code_analysis::Source::new(
            LANG::Cpp,
            source.as_bytes(),
        ))
        .unwrap();
        let index = collect(
            ast.language(),
            ast.as_tree_sitter(),
            ast.source(),
            &DocumentationContext::default(),
            &DocumentationOptions::default(),
        )
        .unwrap();
        let qualifications: Vec<_> = index.qualifications().collect();
        assert_eq!(
            qualifications
                .iter()
                .map(|q| q.get().reason)
                .collect::<Vec<_>>(),
            reasons
        );
        for q in qualifications {
            assert_eq!(q.get().subject, subject);
            assert_eq!(
                index.qualified_owner(q).unwrap().is_some(),
                subject == QualificationSubject::Candidate
            );
            assert_eq!(
                index.qualified_document(q).unwrap(),
                index.documents().next().unwrap()
            );
        }
    }
    let ast = big_code_analysis::Ast::parse(big_code_analysis::Source::new(
        LANG::Cpp,
        b"void f(int x[2]);\n/**@fn void absent(int x[2])*/",
    ))
    .unwrap();
    let index = collect(
        ast.language(),
        ast.as_tree_sitter(),
        ast.source(),
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 0);
    assert!(
        index
            .assessments()
            .any(|a| a.get().axis == AssessmentAxis::Owner)
    );
}

#[test]
fn semantic_validation_rejects_a_fixture_with_its_measured_axis_removed() {
    let probe = REGISTRY.iter().find(|p| p.key == "width").unwrap();
    let parsed = Parsed::new(probe, 32).unwrap();
    let index = parsed.collect(probe).unwrap();
    assert!(Prepared::new(probe, 64, &parsed, &index).is_err());
    let probe = REGISTRY
        .iter()
        .find(|p| p.key == "qualified-candidates")
        .unwrap();
    let parsed = Parsed::new(probe, 32).unwrap();
    let index = parsed.collect(probe).unwrap();
    assert!(Prepared::new(probe, 64, &parsed, &index).is_err());
}
