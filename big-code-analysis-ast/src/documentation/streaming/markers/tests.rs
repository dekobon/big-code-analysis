#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::{AnyParser, LANG, Search};

#[cfg(feature = "java")]
#[test]
fn java_source_selects_empty_new_block_before_legacy_selection() {
    use crate::JavaCode;
    let source = b"/** a\n * b\n * c */\n/**/\nclass C {}\n";
    let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut runs = Vec::new();
    let mut legacy = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if JavaCode::is_comment(node) {
            source_comment::<JavaCode>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: true,
                    sink: &mut |run| runs.push(run),
                },
            )
            .unwrap();
            super::super::marker_comment::<JavaCode>(node, source, ancestors, |event| {
                legacy.push(event);
            });
        }
    });
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].target.unwrap().node().byte_range(), 24..34);
    assert_eq!(
        runs[0]
            .members
            .iter()
            .map(|m| (m.node.node().byte_range(), m.selection))
            .collect::<Vec<_>>(),
        [
            (0..18, MarkerSelection::Superseded),
            (19..23, MarkerSelection::Selected),
        ]
    );
    assert_eq!(
        legacy
            .iter()
            .filter_map(|e| match e {
                super::super::LegacyEvent::BoundComment(n) => Some(n.as_tree_sitter().byte_range()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0..18]
    );
    assert!(matches!(
        legacy[0],
        super::super::LegacyEvent::Comment {
            marker: true,
            by_placement: false,
            ..
        }
    ));
    assert!(matches!(
        legacy[1],
        super::super::LegacyEvent::Comment {
            marker: false,
            by_placement: false,
            ..
        }
    ));
    assert!(matches!(
        legacy[2],
        super::super::LegacyEvent::BoundComment(_)
    ));
    assert_eq!(legacy.len(), 3);
}

#[cfg(feature = "java")]
#[test]
fn divergent_marker_queries_share_real_cache_in_both_orders() {
    use crate::{FoldCache, JavaCode, SiblingCache};
    for source_first in [false, true] {
        let source = b"/**/\nclass C {}";
        let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let root = parser.root_node();
        let comment = root.children().next().unwrap();
        let cache = FoldCache::default();
        let siblings = SiblingCache::default();
        let chain = [root];
        let ancestors = Ancestors::for_walk(&chain, &comment, &cache, &siblings);
        let order = if source_first {
            [CandidateView::Source(&context), CandidateView::Legacy]
        } else {
            [CandidateView::Legacy, CandidateView::Source(&context)]
        };
        let before = marker_reads::observed();
        for view in order.into_iter().cycle().take(6) {
            assert_eq!(
                view.marker::<JavaCode>(&comment, source, ancestors),
                matches!(view, CandidateView::Source(_))
            );
        }
        // expected: each distinct view computes once for this visited node.
        assert_eq!(marker_reads::observed() - before, 2);

        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let cache = FoldCache::default();
        let ancestors = Ancestors::for_walk(&chain, &comment, &cache, &siblings);
        let before = marker_reads::observed();
        let scans = stretches_collected::observed();
        let mut runs = Vec::new();
        let mut events = Vec::new();
        for view in order {
            match view {
                CandidateView::Source(_) => source_comment::<JavaCode>(
                    &comment,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound: true,
                        sink: &mut |run| runs.push(run),
                    },
                )
                .unwrap(),
                CandidateView::Legacy => {
                    super::super::marker_comment::<JavaCode>(
                        &comment,
                        source,
                        ancestors,
                        |event| {
                            events.push(event);
                        },
                    );
                }
            }
        }
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].members.len(), 1);
        assert_eq!(runs[0].members[0].node.node().byte_range(), 0..4);
        assert_eq!(runs[0].members[0].selection, MarkerSelection::Selected);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            super::super::LegacyEvent::Comment {
                marker: false,
                by_placement: false,
                ..
            }
        ));
        // expected: two distinct questions computed once; one final-member scan per view.
        assert_eq!(marker_reads::observed() - before, 2);
        assert_eq!(stretches_collected::observed() - scans, 2);
    }
}

#[cfg(feature = "java")]
#[test]
fn legacy_deferrals_rejections_and_eof_build_no_source_evidence() {
    use crate::JavaCode;
    for (source, expected_scans, expected_bound) in [
        ("/**a*/\n/**b*/\nclass C {}", 1, vec![7..13]),
        ("/**a*/\n/**b*/", 0, vec![]),
        (
            "class C { void f() { /**a*/\n/**b*/\nclass N {} } }",
            0,
            vec![],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
        let built = evidence_built::observed();
        let scans = stretches_collected::observed();
        let mut bound = Vec::new();
        let mut visits = 0;
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if JavaCode::is_comment(node) {
                visits += 1;
                super::super::marker_comment::<JavaCode>(node, parser.code(), ancestors, |event| {
                    if let super::super::LegacyEvent::BoundComment(n) = event {
                        assert_eq!(visits, 2);
                        bound.push(n.as_tree_sitter().byte_range());
                    }
                });
                if visits == 1 {
                    assert_eq!(stretches_collected::observed() - scans, 0);
                }
            }
        });
        assert_eq!(visits, 2);
        assert_eq!(bound, expected_bound);
        assert_eq!(stretches_collected::observed() - scans, expected_scans);
        assert_eq!(evidence_built::observed() - built, 0);
    }
}

#[cfg(any(
    feature = "rust",
    feature = "java",
    feature = "kotlin",
    feature = "groovy",
    feature = "csharp",
    feature = "php",
    feature = "javascript",
    feature = "mozjs",
    feature = "typescript",
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc"
))]
fn selections<T: Checker + crate::traits::LanguageInfo>(
    source: &str,
) -> (Vec<(usize, MarkerSelection)>, Vec<usize>) {
    let parser = AnyParser::parse(T::lang(), source.as_bytes().to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut selected = Vec::new();
    let mut billed = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: true,
                    sink: &mut |run| {
                        selected.extend(
                            run.members
                                .iter()
                                .map(|m| (m.node.node().start_position().row, m.selection)),
                        );
                    },
                },
            )
            .unwrap();
            super::super::marker_comment::<T>(
                node,
                parser.code(),
                ancestors,
                |event| match event {
                    super::super::LegacyEvent::Comment {
                        node,
                        by_placement: true,
                        ..
                    }
                    | super::super::LegacyEvent::BoundComment(node) => {
                        billed.push(node.start_row());
                    }
                    _ => {}
                },
            );
        }
    });
    (selected, billed)
}

#[cfg(feature = "rust")]
#[test]
fn rust_all_crosses_plain_comments_attributes_and_blank_rows() {
    assert_eq!(
        selections::<crate::RustCode>("///a\n// plain\n#[inline]\n\n///b\nfn f() {}"),
        (
            vec![
                (0, MarkerSelection::Selected),
                (4, MarkerSelection::Selected)
            ],
            vec![0, 4]
        )
    );
}

#[cfg(feature = "java")]
#[test]
fn java_last_joins_markdown_and_accepts_four_slashes() {
    assert_eq!(
        selections::<crate::JavaCode>("///old\n\n///new\n////\nclass C {}"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected),
                (3, MarkerSelection::Selected)
            ],
            vec![2]
        )
    );
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_last_reads_across_plain_comments() {
    assert_eq!(
        selections::<crate::KotlinCode>("/**old*/\n// plain\n/**new*/\nfun f() {}"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected)
            ],
            vec![2]
        )
    );
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_nearest_plain_comment_detaches() {
    assert_eq!(
        selections::<crate::GroovyCode>("/**old*/\n// plain\nclass C {}"),
        (vec![(0, MarkerSelection::Unattached)], vec![])
    );
    assert_eq!(
        selections::<crate::GroovyCode>("/**old*/\n/* hidden */ class C {}"),
        (vec![(0, MarkerSelection::Selected)], vec![0])
    );
    assert_eq!(
        selections::<crate::GroovyCode>("/**old*/\npackage p\nclass C {}"),
        (vec![(0, MarkerSelection::Selected)], vec![0])
    );
}

#[cfg(feature = "csharp")]
#[test]
fn csharp_leading_joins_but_hidden_plain_breaks_composition() {
    assert_eq!(
        selections::<crate::CsharpCode>("///old\n// plain\n///a\n///b\nclass C {}"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected),
                (3, MarkerSelection::Selected)
            ],
            vec![2, 3]
        )
    );
    assert_eq!(
        selections::<crate::CsharpCode>("///a\n#region R\n///b\nclass C {}\n#endregion"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected)
            ],
            vec![2]
        )
    );
}

#[cfg(feature = "php")]
#[test]
fn php_leading_keeps_last_and_qualified_define_grouping() {
    assert_eq!(
        selections::<crate::PhpCode>(
            "<?php\n$x = 0;\n/** old */\n// plain\n/** new */\n((define('X', 1)));"
        ),
        (
            vec![
                (2, MarkerSelection::Superseded),
                (4, MarkerSelection::Selected)
            ],
            vec![4]
        )
    );
}

#[cfg(feature = "javascript")]
#[test]
fn javascript_leading_keeps_last() {
    assert_eq!(
        selections::<crate::JavascriptCode>("/**old*/\n// plain\n/**new*/\nfunction f() {}"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected)
            ],
            vec![2]
        )
    );
    assert_eq!(
        selections::<crate::JavascriptCode>(
            "function outer() {\n/**inner*/\nfunction inner() {}\n/**note*/\nreturn 1;\n}"
        ),
        (
            vec![
                (1, MarkerSelection::Selected),
                (3, MarkerSelection::Unattached)
            ],
            vec![1, 3]
        )
    );
}

#[cfg(feature = "mozjs")]
#[test]
fn mozjs_leading_keeps_last() {
    assert_eq!(
        selections::<crate::MozjsCode>("/**old*/\n// plain\n/**new*/\nfunction f() {}"),
        (
            vec![
                (0, MarkerSelection::Superseded),
                (2, MarkerSelection::Selected)
            ],
            vec![2]
        )
    );
    assert_eq!(
        selections::<crate::MozjsCode>(
            "function outer() {\n/**inner*/\nfunction inner() {}\n/**note*/\nreturn 1;\n}"
        ),
        (
            vec![
                (1, MarkerSelection::Selected),
                (3, MarkerSelection::Unattached)
            ],
            vec![1, 3]
        )
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_after_code_is_unattached() {
    assert_eq!(
        selections::<crate::TypescriptCode>("const x = 1; /**doc*/\nfunction f() {}"),
        (vec![(0, MarkerSelection::Unattached)], vec![])
    );
    assert_eq!(
        selections::<crate::TypescriptCode>(
            "function outer() {\n/**inner*/\nfunction inner() {}\n/**note*/\nreturn 1;\n}"
        ),
        (
            vec![
                (1, MarkerSelection::Selected),
                (3, MarkerSelection::Unattached)
            ],
            vec![1, 3]
        )
    );
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_after_code_is_unattached() {
    assert_eq!(
        selections::<crate::TsxCode>("const x = 1; /**doc*/\nfunction f() {}"),
        (vec![(0, MarkerSelection::Unattached)], vec![])
    );
    assert_eq!(
        selections::<crate::TsxCode>(
            "function outer() {\n/**inner*/\nfunction inner() {}\n/**note*/\nreturn 1;\n}"
        ),
        (
            vec![
                (1, MarkerSelection::Selected),
                (3, MarkerSelection::Unattached)
            ],
            vec![1, 3]
        )
    );
}

#[cfg(feature = "java")]
#[test]
fn java_source_context_keeps_nested_and_package_candidates() {
    assert_eq!(
        selections::<crate::JavaCode>("class C { void f() {\n/**nested*/\nclass N {}\n} }"),
        (vec![(1, MarkerSelection::Selected)], vec![])
    );
    assert_eq!(
        selections::<crate::JavaCode>("/**pkg*/\npackage p;\nclass C {}"),
        (vec![(0, MarkerSelection::Selected)], vec![])
    );
}

#[cfg(feature = "java")]
#[test]
fn unbound_retention_is_lazy_on_the_real_source_path() {
    use crate::JavaCode;
    for (source, retain_unbound, expected) in [
        ("/**old*/\n/**new*/\nclass C {}", false, 1),
        ("/**old*/\n/**new*/\nclass C {}", true, 2),
        ("/**dangling*/", false, 0),
        ("/**dangling*/", true, 1),
    ] {
        let parser = AnyParser::parse(LANG::Java, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let before = evidence_built::observed();
        let scans = stretches_collected::observed();
        let mut members = 0;
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if JavaCode::is_comment(node) {
                source_comment::<JavaCode>(
                    node,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound,
                        sink: &mut |run| members += run.members.len(),
                    },
                )
                .unwrap();
            }
        });
        assert_eq!(members, expected);
        assert_eq!(evidence_built::observed() - before, expected);
        assert_eq!(
            stretches_collected::observed() - scans,
            usize::from(expected != 0)
        );
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn gathering<T: Checker + crate::traits::LanguageInfo>() {
    assert_eq!(
        selections::<T>("/**a*/\n/**b*/\n/**@file*/\n/**dangling*/"),
        (
            vec![
                (0, MarkerSelection::Selected),
                (1, MarkerSelection::Selected),
                (2, MarkerSelection::Selected),
                (3, MarkerSelection::Unattached)
            ],
            vec![2, 0, 1]
        )
    );
    assert_eq!(
        selections::<T>("/**a*/\n/** text \"@file\" */\nvoid f();"),
        (
            vec![
                (0, MarkerSelection::Selected),
                (1, MarkerSelection::Selected)
            ],
            vec![0, 1]
        )
    );
}

#[cfg(feature = "rust")]
#[test]
fn rust_nested_inner_and_macro_evidence_keeps_exact_source_spelling() {
    use crate::{RustCode, documentation::UnresolvedReason};
    for (source, range, target, local, uncertainty, billed) in [
        ("/*!*/\nfn f() {}\n", 0..5, Some(6..15), true, None, vec![0]),
        (
            "fn outer() {\n///inner\nfn inner() {}\n}",
            13..21,
            Some(22..35),
            false,
            None,
            vec![],
        ),
        (
            "m! { ///audit\nfn f() {} }\n",
            5..13,
            None,
            true,
            Some(UnresolvedReason::Expansion),
            vec![0],
        ),
        (
            "///audit\r\nfn f() {}",
            0..8,
            Some(10..19),
            false,
            None,
            vec![0],
        ),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let mut runs = Vec::new();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if RustCode::is_comment(node) {
                source_comment::<RustCode>(
                    node,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound: false,
                        sink: &mut |run| runs.push(run),
                    },
                )
                .unwrap();
            }
        });
        assert_eq!(runs.len(), 1, "{source}");
        let run = &runs[0];
        assert_eq!(run.members.len(), 1);
        let member = &run.members[0];
        assert_eq!(member.source.pieces()[0].range(), range, "{source}");
        assert_eq!(member.source.pieces().len(), 1);
        assert_eq!(member.selection, MarkerSelection::Selected);
        assert_eq!(matches!(member.placement, MarkerPlacement::Local), local);
        assert_eq!(member.uncertainty, uncertainty);
        assert_eq!(
            run.target.map(|n| n.node().byte_range()),
            target,
            "{source}"
        );
        assert_eq!(selections::<RustCode>(source).1, billed);
    }
}

#[cfg(feature = "rust")]
#[test]
fn source_run_retains_containing_and_preceding_nodes() {
    use crate::RustCode;
    let source = b"fn outer() {\nlet x = 0;\n///doc\nfn inner() {}\n}";
    let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut runs = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if RustCode::is_comment(node) {
            source_comment::<RustCode>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: false,
                    sink: &mut |run| runs.push(run),
                },
            )
            .unwrap();
        }
    });
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    let containing = run.containing.unwrap().node();
    assert_eq!(
        (containing.kind(), containing.byte_range()),
        ("block", 11..46)
    );
    let before = run.code_before.unwrap().node();
    assert_eq!(
        (before.kind(), before.byte_range()),
        ("let_declaration", 13..23)
    );
    let target = run.target.unwrap().node();
    assert_eq!(
        (target.kind(), target.byte_range()),
        ("function_item", 31..44)
    );
    assert_eq!(run.members.len(), 1);
    assert_eq!(run.members[0].source.pieces()[0].range(), 24..30);
    assert_eq!(run.members[0].selection, MarkerSelection::Selected);
    assert!(matches!(run.members[0].placement, MarkerPlacement::Forward));
}

#[cfg(feature = "rust")]
#[test]
fn rust_macro_context_is_not_recovered_from_a_known_empty_chain() {
    use crate::{RustCode, documentation::UnresolvedReason};
    let parser = AnyParser::parse(
        LANG::Rust,
        b"m! { ///audit\nfn f() {} }".to_vec(),
        None,
        None,
    )
    .unwrap();
    let mut comments = 0;
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if RustCode::is_comment(node) {
            comments += 1;
            assert_eq!(
                RustCode::source_doc_uncertainty(node, parser.code(), ancestors),
                Some(UnresolvedReason::Expansion)
            );
            assert_eq!(
                RustCode::source_doc_uncertainty(node, parser.code(), Ancestors::known(&[])),
                None
            );
        }
    });
    assert_eq!(comments, 1);
}

#[cfg(feature = "rust")]
#[test]
fn source_rust_pruned_runs_remain_available_and_defer_once() {
    use crate::RustCode;
    let source = "///outer\n#[test]\nfn test_f() {}\n///kept\nfn f() {}";
    assert_eq!(
        selections::<RustCode>(source),
        (
            vec![
                (0, MarkerSelection::Selected),
                (3, MarkerSelection::Selected)
            ],
            vec![0, 3]
        )
    );
    for width in [4, 64, 256] {
        let source = format!(
            "{}fn f() {{}}",
            "///doc\n// plain\n#[inline]\n".repeat(width)
        );
        let parser = AnyParser::parse(LANG::Rust, source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let scans = stretches_collected::observed();
        let built = evidence_built::observed();
        let derived = crate::node::child_chains_derived::observed();
        let mut visits = 0;
        let mut emitted = 0;
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if RustCode::is_comment(node) {
                visits += 1;
                source_comment::<RustCode>(
                    node,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound: false,
                        sink: &mut |run| {
                            assert_eq!(visits, 2 * width);
                            assert_eq!(run.members.len(), width);
                            emitted += 1;
                        },
                    },
                )
                .unwrap();
                assert_eq!(
                    stretches_collected::observed() - scans,
                    usize::from(visits == 2 * width)
                );
            }
        });
        assert_eq!((visits, emitted), (2 * width, 1));
        // expected: one final-member scan, one source record per doc, no derived chains.
        assert_eq!(stretches_collected::observed() - scans, 1);
        assert_eq!(evidence_built::observed() - built, width);
        assert_eq!(crate::node::child_chains_derived::observed() - derived, 0);
    }
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn conditional_and_gathered_evidence<T: Checker + crate::traits::LanguageInfo>() {
    // bca: suppress(halstead) -- independent gathering and conditional fixtures share the four grammar adapters.
    use crate::documentation::UnresolvedReason;
    let source = b"/**a*/\n/**b*/\n/**@file*/\n/**dangling*/";
    let parser = AnyParser::parse(T::lang(), source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut runs = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: true,
                    sink: &mut |run| runs.push(run),
                },
            )
            .unwrap();
        }
    });
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].members.len(), 4);
    for member in &runs[0].members[..2] {
        let MarkerPlacement::Gathered(gatherer) = member.placement else {
            panic!("gathered predecessor")
        };
        assert_eq!(gatherer.node().byte_range(), 14..24);
    }
    assert!(matches!(
        runs[0].members[2].placement,
        MarkerPlacement::Gatherer
    ));
    assert_eq!(runs[0].members[3].selection, MarkerSelection::Unattached);
    for declares in [false, true] {
        const DEPTH: usize = 12;
        let source = format!(
            "{}{}{}",
            "/**doc*/\n#if A\n".repeat(DEPTH),
            if declares {
                "void f();\n"
            } else {
                "#include <x.h>\n"
            },
            "#endif\n".repeat(DEPTH)
        );
        let parser = AnyParser::parse(T::lang(), source.as_bytes().to_vec(), None, None).unwrap();
        let context = SourceContext::new(parser.ts_tree(), parser.code());
        let mut runs = Vec::new();
        let before = crate::lang_helpers::doxygen::conditionals_read::observed();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if T::is_comment(node) {
                super::super::marker_comment::<T>(node, parser.code(), ancestors, |_| {});
                source_comment::<T>(
                    node,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound: true,
                        sink: &mut |run| runs.push(run),
                    },
                )
                .unwrap();
            }
        });
        assert_eq!(runs.len(), DEPTH);
        assert_eq!(
            crate::lang_helpers::doxygen::conditionals_read::observed() - before,
            DEPTH
        );
        for run in runs {
            assert_eq!(run.members.len(), 1);
            assert_eq!(run.target.is_some(), declares);
            assert_eq!(
                run.target_uncertainty,
                declares.then_some(UnresolvedReason::Conditional)
            );
            assert_eq!(
                run.members[0].selection,
                if declares {
                    MarkerSelection::Selected
                } else {
                    MarkerSelection::Unattached
                }
            );
        }
    }
}

#[cfg(feature = "c")]
#[test]
fn c_conditional_and_gathering_context() {
    conditional_and_gathered_evidence::<crate::CCode>();
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_conditional_and_gathering_context() {
    conditional_and_gathered_evidence::<crate::CppCode>();
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_conditional_and_gathering_context() {
    conditional_and_gathered_evidence::<crate::MozcppCode>();
}
#[cfg(feature = "objc")]
#[test]
fn objc_conditional_and_gathering_context() {
    conditional_and_gathered_evidence::<crate::ObjcCode>();
}

#[cfg(feature = "c")]
#[test]
fn c_gatherer_and_predecessors_once() {
    gathering::<crate::CCode>();
}

#[cfg(feature = "cpp")]
#[test]
fn cpp_gatherer_and_predecessors_once() {
    gathering::<crate::CppCode>();
}

#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_gatherer_and_predecessors_once() {
    gathering::<crate::MozcppCode>();
}

#[cfg(feature = "objc")]
#[test]
fn objc_gatherer_and_predecessors_once() {
    gathering::<crate::ObjcCode>();
}
