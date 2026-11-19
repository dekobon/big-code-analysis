#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::documentation::SourcePiece;
use crate::{AnyParser, LANG, Search, documentation::UnresolvedReason};
use std::ops::Range;

#[cfg(any(
    feature = "java",
    feature = "rust",
    feature = "typescript",
    feature = "groovy",
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc",
    feature = "csharp",
    feature = "php"
))]
fn runs<T: Checker>(parser: &AnyParser, retain_unbound: bool) -> Vec<MarkerRun<'_>> {
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut runs = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound,
                    sink: &mut |run| runs.push(run),
                },
            )
            .unwrap();
        }
    });
    runs
}

#[cfg(any(
    feature = "java",
    feature = "rust",
    feature = "csharp",
    feature = "php",
    feature = "typescript",
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc"
))]
#[allow(clippy::naive_bytecount)]
fn restricted(lang: LANG, source: &[u8], included: &[Range<usize>]) -> AnyParser {
    let point = |at| {
        let prefix = &source[..at];
        tree_sitter::Point::new(
            prefix.iter().filter(|&&b| b == b'\n').count(),
            prefix
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(at, |pos| at - pos - 1),
        )
    };
    let ranges: Vec<_> = included
        .iter()
        .map(|r| tree_sitter::Range {
            start_byte: r.start,
            end_byte: r.end,
            start_point: point(r.start),
            end_point: point(r.end),
        })
        .collect();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    AnyParser::from_tree(lang, tree, source.to_vec()).unwrap()
}

#[cfg(feature = "java")]
#[test]
fn excluded_marker_cannot_supersede_genuine_documentation() {
    let source = b"/**old*/\n///fake\nclass C {}";
    let parser = restricted(LANG::Java, source, &[0..11, 12..source.len()]);
    assert!(runs::<crate::JavaCode>(&parser, false).is_empty());
    let runs = runs::<crate::JavaCode>(&parser, true);
    assert_eq!(runs.len(), 2);
    let members: Vec<_> = runs.iter().flat_map(|r| &r.members).collect();
    assert_eq!(members.len(), 2);
    assert!(runs.iter().all(|r| r.target.is_none()));
    assert_eq!(members[0].node.node().byte_range(), 0..8);
    assert_eq!(members[0].source.pieces()[0].bytes(), b"/**old*/");
    assert_eq!(
        members[1]
            .source
            .pieces()
            .iter()
            .map(SourcePiece::range)
            .collect::<Vec<_>>(),
        [9..11, 12..16]
    );
    assert!(matches!(members[1].placement, MarkerPlacement::Unknown));
    for member in members {
        assert_eq!(member.selection, MarkerSelection::Unattached);
        assert_eq!(member.uncertainty, Some(UnresolvedReason::ExcludedGap));
    }
    let mut bound = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if crate::JavaCode::is_comment(node) {
            super::super::marker_comment::<crate::JavaCode>(node, source, ancestors, |event| {
                if let super::super::LegacyEvent::BoundComment(n) = event {
                    bound.push(n.as_tree_sitter().byte_range());
                }
            });
        }
    });
    assert_eq!(bound, [9..16]);
}

#[cfg(feature = "java")]
#[test]
fn excluded_marker_byte_never_becomes_a_native_marker() {
    let source = b"///doc\nclass C {}";
    let parser = restricted(LANG::Java, source, &[0..2, 3..source.len()]);
    assert!(runs::<crate::JavaCode>(&parser, false).is_empty());
    let runs = runs::<crate::JavaCode>(&parser, true);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].members.len(), 1);
    let member = &runs[0].members[0];
    assert_eq!(member.selection, MarkerSelection::Unattached);
    assert!(matches!(member.placement, MarkerPlacement::Unknown));
    assert_eq!(member.uncertainty, Some(UnresolvedReason::ExcludedGap));
    assert_eq!(
        member
            .source
            .pieces()
            .iter()
            .map(SourcePiece::range)
            .collect::<Vec<_>>(),
        [0..2, 3..6]
    );
    assert!(member.source.has_excluded_gap());
}

#[cfg(feature = "java")]
#[test]
fn excluded_attachment_and_target_bytes_are_barriers() {
    for (source, included) in [
        (b"/**doc*/\nXX\nclass C {}".as_slice(), vec![0..9, 12..22]),
        (b"/**doc*/\nclass C {}".as_slice(), vec![0..16, 17..19]),
    ] {
        let parser = restricted(LANG::Java, source, &included);
        assert!(runs::<crate::JavaCode>(&parser, false).is_empty());
        let runs = runs::<crate::JavaCode>(&parser, true);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].target.is_none());
        assert_eq!(runs[0].members.len(), 1);
        assert_eq!(runs[0].members[0].selection, MarkerSelection::Unattached);
        assert_eq!(
            runs[0].members[0].uncertainty,
            Some(UnresolvedReason::ExcludedGap)
        );
        assert_eq!(runs[0].members[0].source.pieces()[0].range(), 0..8);
        assert!(!runs[0].members[0].source.has_excluded_gap());
    }
}

#[cfg(feature = "java")]
#[test]
fn excluded_code_keeps_unrelated_later_docs_and_splits_comment_runs() {
    let source = b"/**old*/\nclass X {}\n/**new*/\nclass C {}";
    let later = source.windows(8).position(|b| b == b"/**new*/").unwrap();
    let parser = restricted(LANG::Java, source, &[0..9, later..source.len()]);
    for retain in [false, true] {
        let runs = runs::<crate::JavaCode>(&parser, retain);
        assert_eq!(runs.len(), if retain { 2 } else { 1 });
        let last = runs.last().unwrap();
        assert_eq!(last.members.len(), 1);
        assert!(last.prefix_gap);
        assert!(last.code_before.is_none());
        assert_eq!(last.members[0].source.pieces()[0].bytes(), b"/**new*/");
        assert_eq!(last.members[0].selection, MarkerSelection::Selected);
        assert_eq!(last.members[0].uncertainty, None);
        assert_eq!(
            last.target.unwrap().node().byte_range(),
            later + 9..source.len()
        );
        if retain {
            assert_eq!(runs[0].members[0].selection, MarkerSelection::Unattached);
            assert_eq!(
                runs[0].members[0].uncertainty,
                Some(UnresolvedReason::ExcludedGap)
            );
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn selected_join_all_run_reports_missing_prefix_for_composition() {
    let source = b"///old\nXXXX\n///new\nfn f() {}";
    let parser = restricted(LANG::Rust, source, &[0..7, 12..source.len()]);
    for retain in [false, true] {
        let result = runs::<crate::RustCode>(&parser, retain);
        assert_eq!(result.len(), if retain { 2 } else { 1 });
        let last = result.last().unwrap();
        assert!(last.prefix_gap);
        assert!(last.code_before.is_none());
        assert_eq!(last.members.len(), 1);
        assert_eq!(last.members[0].selection, MarkerSelection::Selected);
        assert_eq!(last.members[0].source.pieces()[0].bytes(), b"///new");
        assert_eq!(last.target.unwrap().node().kind(), "function_item");
        assert_eq!(last.target.unwrap().node().byte_range(), 19..28);
    }
}

#[cfg(feature = "php")]
#[test]
fn php_file_scan_cannot_read_a_gap_spanning_marker() {
    let source = b"<?php\n/** old */\n/** fake */\nclass C {}\n/** later */\nfunction f() {}";
    let parser = restricted(LANG::Php, source, &[0..20, 21..source.len()]);
    for retain in [false, true] {
        let runs = runs::<crate::PhpCode>(&parser, retain);
        let members: Vec<_> = runs.iter().flat_map(|r| &r.members).collect();
        assert_eq!(
            members.len(),
            if retain { 3 } else { 1 },
            "{} {runs:?}",
            parser.ts_tree().root_node().to_sexp()
        );
        let last = members.last().unwrap();
        assert_eq!(last.source.pieces()[0].bytes(), b"/** later */");
        assert_eq!(last.selection, MarkerSelection::Selected);
        assert!(matches!(last.placement, MarkerPlacement::Forward));
        if retain {
            assert_eq!(members[0].selection, MarkerSelection::Unattached);
            assert_eq!(members[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
            assert!(matches!(members[0].placement, MarkerPlacement::Unknown));
            assert!(matches!(members[1].placement, MarkerPlacement::Unknown));
        }
    }
}

#[cfg(feature = "java")]
#[test]
fn many_included_ranges_do_not_rescan_comment_prefixes() {
    for width in [4, 64, 256] {
        let source = "/**doc*/X\n".repeat(width);
        let included: Vec<_> = (0..width).map(|n| n * 10..n * 10 + 8).collect();
        let parser = restricted(LANG::Java, source.as_bytes(), &included);
        let before = stretches_collected::observed();
        let built = evidence_built::observed();
        let runs = runs::<crate::JavaCode>(&parser, true);
        assert_eq!(runs.len(), width);
        // expected: each gap-delimited comment is its own final member.
        assert_eq!(stretches_collected::observed() - before, width);
        assert_eq!(evidence_built::observed() - built, width);
        assert!(
            runs.iter()
                .all(|r| r.members.len() == 1 && r.target.is_none())
        );
    }
}

#[cfg(feature = "java")]
#[test]
fn foreign_source_context_fails_without_partial_emission() {
    let source = b"/**a*/\n/**b*/\nclass C {}";
    let parser = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
    let other = AnyParser::parse(LANG::Java, source.to_vec(), None, None).unwrap();
    let context = SourceContext::new(other.ts_tree(), source);
    let mut visits = 0;
    let mut emitted = 0;
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if crate::JavaCode::is_comment(node) {
            visits += 1;
            assert_eq!(
                source_comment::<crate::JavaCode>(
                    node,
                    ancestors,
                    &mut SourceRequest {
                        context: &context,
                        retain_unbound: true,
                        sink: &mut |_| emitted += 1,
                    }
                ),
                Err(SourceError::ForeignTree)
            );
        }
    });
    assert_eq!(visits, 2);
    assert_eq!(emitted, 0);
}

#[cfg(feature = "typescript")]
fn mixed_rejections<T: Checker + crate::traits::LanguageInfo>() {
    for (source, expected) in [
        (
            "const x = 1; /**a*/\n/**b*/\nfunction f() {}",
            [MarkerSelection::Unattached, MarkerSelection::Selected],
        ),
        (
            "class C {\n/**a*/\n@dec\n/**b*/\nm() {}\n}",
            [MarkerSelection::Selected, MarkerSelection::Unattached],
        ),
    ] {
        let parser = AnyParser::parse(T::lang(), source.as_bytes().to_vec(), None, None).unwrap();
        assert!(!parser.ts_tree().root_node().has_error());
        for retain in [false, true] {
            let runs = runs::<T>(&parser, retain);
            let members: Vec<_> = runs.iter().flat_map(|r| &r.members).collect();
            assert_eq!(
                members.iter().map(|m| m.selection).collect::<Vec<_>>(),
                expected
                    .into_iter()
                    .filter(|s| retain || *s == MarkerSelection::Selected)
                    .collect::<Vec<_>>(),
                "{source}"
            );
        }
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_mixed_rejections_are_not_superseded() {
    mixed_rejections::<crate::TypescriptCode>();
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_mixed_rejections_are_not_superseded() {
    mixed_rejections::<crate::TsxCode>();
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_hidden_doc_is_unattached_beside_selected_doc() {
    let parser = AnyParser::parse(
        LANG::Groovy,
        b"/**a*/\n/**b*/ class C {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let runs = runs::<crate::GroovyCode>(&parser, true);
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0]
            .members
            .iter()
            .map(|m| m.selection)
            .collect::<Vec<_>>(),
        [MarkerSelection::Selected, MarkerSelection::Unattached]
    );
}

#[cfg(any(
    feature = "rust",
    feature = "java",
    feature = "csharp",
    feature = "c",
    feature = "cpp",
    feature = "mozcpp",
    feature = "objc"
))]
fn terminators<T: Checker + crate::traits::LanguageInfo>() {
    for (source, bytes) in [
        (b"///audit\n".as_slice(), b"///audit".as_slice()),
        (b"///audit\r\n", b"///audit"),
        (b"///audit\r", b"///audit\r"),
    ] {
        let parser = AnyParser::parse(T::lang(), source.to_vec(), None, None).unwrap();
        let runs = runs::<T>(&parser, true);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].members.len(), 1);
        assert_eq!(runs[0].members[0].source.pieces().len(), 1);
        assert_eq!(runs[0].members[0].source.pieces()[0].bytes(), bytes);
    }
    let source = b"///audit\r\n";
    for included in [vec![0..9], vec![0..9, 9..10], vec![0..8, 9..10]] {
        let parser = restricted(T::lang(), source, &included);
        let runs = runs::<T>(&parser, true);
        assert_eq!(runs.len(), 1);
        let member = &runs[0].members[0];
        let bytes: Vec<_> = member
            .source
            .pieces()
            .iter()
            .flat_map(SourcePiece::bytes)
            .copied()
            .collect();
        assert_eq!(
            bytes,
            if included == [0..9] {
                b"///audit\r".as_slice()
            } else {
                b"///audit"
            }
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_verbatim_line_terminators() {
    terminators::<crate::RustCode>();
    for (source, expected) in [
        (b"//!audit\r".as_slice(), b"//!audit\r".as_slice()),
        (b"//!audit\r\n", b"//!audit"),
        (b"//!audit\n", b"//!audit"),
    ] {
        let parser = AnyParser::parse(LANG::Rust, source.to_vec(), None, None).unwrap();
        let runs = runs::<crate::RustCode>(&parser, false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].members.len(), 1);
        assert_eq!(runs[0].members[0].selection, MarkerSelection::Selected);
        assert_eq!(runs[0].members[0].source.pieces().len(), 1);
        assert_eq!(runs[0].members[0].source.pieces()[0].bytes(), expected);
    }
}
#[cfg(feature = "java")]
#[test]
fn java_verbatim_line_terminators() {
    terminators::<crate::JavaCode>();
}
#[cfg(feature = "csharp")]
#[test]
fn csharp_verbatim_line_terminators() {
    terminators::<crate::CsharpCode>();
}

#[cfg(feature = "c")]
#[test]
fn c_verbatim_line_terminators() {
    terminators::<crate::CCode>();
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_verbatim_line_terminators() {
    terminators::<crate::CppCode>();
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_verbatim_line_terminators() {
    terminators::<crate::MozcppCode>();
}
#[cfg(feature = "objc")]
#[test]
fn objc_verbatim_line_terminators() {
    terminators::<crate::ObjcCode>();
}

#[cfg(any(feature = "rust", feature = "java"))]
fn recovery<T: Checker + crate::traits::LanguageInfo>(source: &[u8]) {
    let parser = AnyParser::parse(T::lang(), source.to_vec(), None, None).unwrap();
    let runs = runs::<T>(&parser, true);
    assert_eq!(runs.len(), 1, "{}", parser.ts_tree().root_node().to_sexp());
    assert!(runs[0].target.unwrap().node().is_error());
    assert_eq!(
        runs[0].target_uncertainty,
        Some(UnresolvedReason::GrammarBoundary)
    );
    assert_eq!(runs[0].members.len(), 1);
    assert_eq!(runs[0].members[0].selection, MarkerSelection::Selected);
}

#[cfg(feature = "rust")]
#[test]
fn rust_recovery_target_retains_grammar_uncertainty() {
    recovery::<crate::RustCode>(b"///audit\n{ fn f() {}");
    recovery::<crate::RustCode>(b"/// d\n1 fn f() {} 2\n");
}

#[cfg(feature = "java")]
#[test]
fn java_recovery_target_retains_default_uncertainty() {
    recovery::<crate::JavaCode>(b"/**doc*/\nclass {\npackage a;\n@ ");
}

#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
fn gatherer_search<T: Checker + crate::traits::LanguageInfo>() {
    for width in [4, 64, 256] {
        let source = format!("/**lead*/\n/**@file*/\n{}", "/**dangling*/\n".repeat(width));
        let parser = AnyParser::parse(T::lang(), source.as_bytes().to_vec(), None, None).unwrap();
        let before = doc_comment::gatherer_probes::observed();
        let runs = runs::<T>(&parser, true);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].members.len(), width + 2);
        assert!(matches!(
            runs[0].members[0].placement,
            MarkerPlacement::Gathered(_)
        ));
        // expected: the dangling suffix plus its one gatherer, searched once.
        assert_eq!(doc_comment::gatherer_probes::observed() - before, width + 1);
    }
}

#[cfg(feature = "c")]
#[test]
fn c_gatherer_search_is_once_per_stretch() {
    gatherer_search::<crate::CCode>();
}
#[cfg(feature = "cpp")]
#[test]
fn cpp_gatherer_search_is_once_per_stretch() {
    gatherer_search::<crate::CppCode>();
}
#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_gatherer_search_is_once_per_stretch() {
    gatherer_search::<crate::MozcppCode>();
}
#[cfg(feature = "objc")]
#[test]
fn objc_gatherer_search_is_once_per_stretch() {
    gatherer_search::<crate::ObjcCode>();
}

#[cfg(all(test, feature = "typescript"))]
fn source_prefix_gap<T: Checker + crate::traits::LanguageInfo>() {
    for (source, included, expected_runs, span, target) in [
        (
            b"/**a*/ /**b*/\nfunction f() {}".as_slice(),
            vec![0..6, 7..29],
            2,
            7..13,
            14..29,
        ),
        (
            b"XXXXXX /**b*/\nfunction f() {}".as_slice(),
            vec![7..29],
            1,
            7..13,
            14..29,
        ),
    ] {
        let parser = restricted(T::lang(), source, &included);
        for retain in [false, true] {
            let runs = runs::<T>(&parser, retain);
            assert_eq!(runs.len(), if retain { expected_runs } else { 1 });
            if retain && expected_runs == 2 {
                assert!(runs[0].code_before.is_none());
                assert!(runs[0].target.is_none());
                assert_eq!(runs[0].members.len(), 1);
                assert_eq!(runs[0].members[0].source.pieces()[0].range(), 0..6);
                assert_eq!(runs[0].members[0].selection, MarkerSelection::Unattached);
                assert_eq!(
                    runs[0].members[0].uncertainty,
                    Some(UnresolvedReason::ExcludedGap)
                );
            }
            let run = runs.last().unwrap();
            assert!(run.code_before.is_none());
            assert_eq!(run.target.unwrap().node().byte_range(), target);
            assert_eq!(run.members.len(), 1);
            let member = &run.members[0];
            assert_eq!(member.node.node().byte_range(), span);
            assert_eq!(member.source.pieces().len(), 1);
            assert_eq!(member.source.pieces()[0].range(), span);
            assert_eq!(member.source.pieces()[0].bytes(), b"/**b*/");
            assert_eq!(member.selection, MarkerSelection::Selected);
            assert!(matches!(member.placement, MarkerPlacement::Forward));
            assert_eq!(member.uncertainty, Some(UnresolvedReason::ExcludedGap));
        }
    }
    let parser = AnyParser::parse(
        T::lang(),
        b"/**a*/ /**b*/\nfunction f() {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let runs = runs::<T>(&parser, false);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].members.len(), 1);
    assert_eq!(runs[0].members[0].node.node().byte_range(), 7..13);
    assert_eq!(runs[0].members[0].selection, MarkerSelection::Selected);
    assert_eq!(runs[0].members[0].uncertainty, None);
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_source_prefix_gap_is_not_preceding_code() {
    source_prefix_gap::<crate::TypescriptCode>();
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_source_prefix_gap_is_not_preceding_code() {
    source_prefix_gap::<crate::TsxCode>();
}

#[cfg(feature = "php")]
#[test]
fn php_source_prefix_gap_keeps_unknown_direction_out_of_selection() {
    for (source, included) in [
        (
            b"<?php\nXX\n/** a */\n/** b */\nclass C {}".as_slice(),
            vec![0..6, 9..37],
        ),
        (
            b"XX\n<?php\n/** a */\n/** b */\nclass C {}".as_slice(),
            vec![3..37],
        ),
    ] {
        let parser = restricted(LANG::Php, source, &included);
        for retain in [false, true] {
            let runs = runs::<crate::PhpCode>(&parser, retain);
            assert_eq!(runs.len(), 1);
            let run = &runs[0];
            assert_eq!(
                run.code_before.map(|n| n.node().byte_range()),
                if included == [3..37] {
                    Some(3..8)
                } else {
                    None
                }
            );
            assert_eq!(run.target.unwrap().node().byte_range(), 27..37);
            assert_eq!(run.members.len(), if retain { 2 } else { 1 });
            let b = run.members.last().unwrap();
            assert_eq!(b.source.pieces().len(), 1);
            assert_eq!(b.source.pieces()[0].range(), 18..26);
            assert_eq!(b.source.pieces()[0].bytes(), b"/** b */");
            assert_eq!(b.selection, MarkerSelection::Selected);
            assert!(matches!(b.placement, MarkerPlacement::Forward));
            assert_eq!(b.uncertainty, None);
            if retain {
                let a = &run.members[0];
                assert_eq!(a.source.pieces().len(), 1);
                assert_eq!(a.source.pieces()[0].range(), 9..17);
                assert_eq!(a.source.pieces()[0].bytes(), b"/** a */");
                assert_eq!(a.selection, MarkerSelection::Unattached);
                assert!(matches!(a.placement, MarkerPlacement::Unknown));
                assert_eq!(a.uncertainty, Some(UnresolvedReason::ExcludedGap));
            }
        }
    }
    let parser = AnyParser::parse(
        LANG::Php,
        b"<?php\n/** a */\n/** b */\nclass C {}".to_vec(),
        None,
        None,
    )
    .unwrap();
    let runs = runs::<crate::PhpCode>(&parser, false);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].members.len(), 2);
    assert!(matches!(
        runs[0].members[0].placement,
        MarkerPlacement::Local
    ));
    assert!(matches!(
        runs[0].members[1].placement,
        MarkerPlacement::Forward
    ));
    assert!(
        runs[0]
            .members
            .iter()
            .all(|m| m.selection == MarkerSelection::Selected && m.uncertainty.is_none())
    );
}

#[cfg(feature = "php")]
#[test]
fn php_source_prefix_gap_cannot_select_unknown_define_documentation() {
    let source = b"<?php\nXX\n/** a */\n((define('X',1)));";
    let parser = restricted(LANG::Php, source, &[0..6, 9..source.len()]);
    assert!(runs::<crate::PhpCode>(&parser, false).is_empty());
    let runs = runs::<crate::PhpCode>(&parser, true);
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert!(run.code_before.is_none());
    assert_eq!(run.target.unwrap().node().byte_range(), 18..source.len());
    assert_eq!(run.members.len(), 1);
    let member = &run.members[0];
    assert_eq!(member.source.pieces()[0].range(), 9..17);
    assert_eq!(member.source.pieces().len(), 1);
    assert_eq!(member.selection, MarkerSelection::Unattached);
    assert!(matches!(member.placement, MarkerPlacement::Unknown));
    assert_eq!(member.uncertainty, Some(UnresolvedReason::ExcludedGap));
}
