#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::{AnyParser, LANG, Search};
use std::ops::Range;

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn runs<T: Checker>(parser: &AnyParser, retain: bool, directives: bool) -> Vec<SourceRun<'_>> {
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut result = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                ancestors,
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: retain,
                    retain_directives: directives,
                    sink: &mut |run| result.push(run),
                },
            )
            .unwrap();
        }
    });
    result
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn parse(lang: LANG, source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(lang, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{}",
        parser.ts_tree().root_node().to_sexp()
    );
    parser
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn identity(node: NodeEvidence<'_>, kind: &str, range: Range<usize>) {
    assert_eq!(node.node().kind(), kind);
    assert_eq!(node.node().byte_range(), range);
    node.validate(node.node()).unwrap();
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn members(run: &SourceRun<'_>, expected: &[(Range<usize>, &[u8], bool)]) {
    assert_eq!(run.members.len(), expected.len());
    for (member, (range, bytes, directive)) in run.members.iter().zip(expected) {
        identity(member.node, "comment", range.clone());
        assert_eq!(member.source.pieces().len(), 1);
        assert_eq!(member.source.pieces()[0].range(), *range);
        assert_eq!(member.source.pieces()[0].bytes(), *bytes);
        assert_eq!(member.directive, *directive);
        assert_eq!(
            member.selected,
            !directive && run.selection != RunSelection::Unattached
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_order_directives_and_candidate_are_independent() {
    let parser = parse(
        LANG::Go,
        b"package p\n// audit\n//go:generate tool\n// more\nfunc F() {}",
    );
    let run = runs::<crate::GoCode>(&parser, false, true).pop().unwrap();
    members(
        &run,
        &[
            (10..18, b"// audit", false),
            (19..37, b"//go:generate tool", true),
            (38..45, b"// more", false),
        ],
    );
    identity(run.target.unwrap(), "function_declaration", 46..57);
    identity(run.following.unwrap(), "function_declaration", 46..57);
    identity(run.code_before.unwrap(), "package_clause", 0..9);
    identity(run.containing.unwrap(), "source_file", 0..57);
    assert_eq!(run.selection, RunSelection::FollowingTarget);
    assert_eq!(run.uncertainty, None);
    assert_eq!(run.end, RunEnd::Token);
    assert!(!run.prefix_gap);
    let before = evidence_built::observed();
    let filtered = runs::<crate::GoCode>(&parser, false, false);
    assert_eq!(evidence_built::observed() - before, 2);
    members(
        &filtered[0],
        &[(10..18, b"// audit", false), (38..45, b"// more", false)],
    );
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_wrappers_magic_and_virtual_opener_keep_distinct_evidence() {
    let parser = parse(
        LANG::Ruby,
        b"#!/ruby\n# frozen_string_literal: true\n# audit\nprivate def f; end",
    );
    let result = runs::<crate::RubyCode>(&parser, false, true);
    assert_eq!(result.len(), 1);
    members(
        &result[0],
        &[
            (0..7, b"#!/ruby", true),
            (8..37, b"# frozen_string_literal: true", true),
            (38..45, b"# audit", false),
        ],
    );
    identity(result[0].target.unwrap(), "call", 46..64);
    assert_eq!(result[0].selection, RunSelection::FollowingTarget);
    assert_eq!(result[0].uncertainty, None);
    let parser = parse(LANG::Ruby, b"# @!method ghost\n#   audit\ndef ghost; end");
    let result = runs::<crate::RubyCode>(&parser, false, false);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].selection,
        RunSelection::Opener(OpenerReason::Explicit)
    );
    identity(result[0].target.unwrap(), "method", 27..41);
    members(
        &result[0],
        &[
            (0..16, b"# @!method ghost", false),
            (17..26, b"#   audit", false),
        ],
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_plain_marked_first_file_and_later_contexts() {
    for (source, selection, target) in [
        (
            "---audit\nfunction f() end",
            RunSelection::Opener(OpenerReason::FirstFile),
            9..25,
        ),
        (
            "module(\"m\")\n---audit\nfunction f() end",
            RunSelection::Opener(OpenerReason::FollowingItem),
            21..37,
        ),
        (
            "module(\"m\")\n-- audit\nfunction f() end",
            RunSelection::Unattached,
            21..37,
        ),
        (
            "x = 1\nmodule(\"m\")\n---audit\nfunction f() end",
            RunSelection::Opener(OpenerReason::FollowingItem),
            27..43,
        ),
        ("---\nfunction f() end", RunSelection::Unattached, 4..20),
        (
            "module(\"m\")\n--- \nfunction f() end",
            RunSelection::Unattached,
            17..33,
        ),
    ] {
        let parser = parse(LANG::Lua, source.as_bytes());
        let result = runs::<crate::LuaCode>(&parser, true, false);
        assert_eq!(result.len(), 1, "{source}");
        assert_eq!(result[0].selection, selection, "{source}");
        identity(result[0].target.unwrap(), "function_declaration", target);
        assert_eq!(result[0].uncertainty, None);
        let selected = runs::<crate::LuaCode>(&parser, false, false);
        assert_eq!(
            selected.len(),
            usize::from(selection != RunSelection::Unattached)
        );
    }
    let parser = parse(LANG::Lua, b"module(\"m\")\n---audit\n-- @function ghost");
    let result = runs::<crate::LuaCode>(&parser, false, false);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].selection,
        RunSelection::Opener(OpenerReason::Tagged)
    );
    assert!(result[0].target.is_none());
    members(
        &result[0],
        &[
            (12..20, b"---audit", false),
            (21..39, b"-- @function ghost", false),
        ],
    );
    assert_eq!(result[0].end, RunEnd::EndOfScope);
    let parser = parse(LANG::Lua, b"---module\n\n---later\nfunction f() end");
    let result = runs::<crate::LuaCode>(&parser, false, false);
    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0].selection,
        RunSelection::Opener(OpenerReason::FirstFile)
    );
    assert!(result[0].target.is_none());
    assert_eq!(
        result[1].selection,
        RunSelection::Opener(OpenerReason::FollowingItem)
    );
    identity(result[1].target.unwrap(), "function_declaration", 20..36);
}

#[cfg(feature = "go")]
#[test]
fn go_source_blank_comment_blank_trailing_and_same_row() {
    for (source, counts, selected, end) in [
        (
            "package p\n// a\n\n// b\nfunc F() {}",
            vec![1, 1],
            vec![false, true],
            RunEnd::Token,
        ),
        (
            "package p\n// a\n//\n// b\nfunc F() {}",
            vec![3],
            vec![true],
            RunEnd::Token,
        ),
        (
            "package p\n/*a*/ /*b*/\nfunc F() {}",
            vec![2],
            vec![true],
            RunEnd::Token,
        ),
        (
            "package p\nfunc F() {} // a",
            vec![1],
            vec![false],
            RunEnd::EndOfScope,
        ),
        ("package p\n// a", vec![1], vec![false], RunEnd::EndOfScope),
        (
            "package p\n// a\nvar x = 1",
            vec![1],
            vec![true],
            RunEnd::Token,
        ),
    ] {
        let parser = parse(LANG::Go, source.as_bytes());
        let result = runs::<crate::GoCode>(&parser, true, true);
        assert_eq!(
            result.iter().map(|r| r.members.len()).collect::<Vec<_>>(),
            counts,
            "{source}"
        );
        assert_eq!(
            result
                .iter()
                .map(|r| r.selection != RunSelection::Unattached)
                .collect::<Vec<_>>(),
            selected,
            "{source}"
        );
        assert_eq!(result.last().unwrap().end, end);
        if result.len() == 2 {
            assert_eq!(result[0].end, RunEnd::BlankLine);
        }
    }
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_yard_scope_uses_actual_wrapper_context() {
    for (source, expected) in [
        (
            "class C\n# audit\nX = 1 unless false\nend",
            RunSelection::FollowingTarget,
        ),
        ("foo do\n# audit\ndef f; end\nend", RunSelection::Unattached),
        ("def outer\n# audit\nX = 1\nend", RunSelection::Unattached),
        (
            "if true\n# audit\ndef f; end\nend",
            RunSelection::FollowingTarget,
        ),
        ("# audit\nputs 1", RunSelection::Unattached),
        ("# audit", RunSelection::Unattached),
        ("##\n# audit", RunSelection::Opener(OpenerReason::Explicit)),
    ] {
        let parser = parse(LANG::Ruby, source.as_bytes());
        let result = runs::<crate::RubyCode>(&parser, true, true);
        assert_eq!(result.len(), 1, "{source}");
        assert_eq!(result[0].selection, expected, "{source}");
        assert_eq!(result[0].uncertainty, None);
        if source.starts_with("if") {
            identity(result[0].target.unwrap(), "then", 15..26);
        }
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_long_terminator_and_body_notes_do_not_merge() {
    let parser = parse(LANG::Lua, b"--[[-audit]]\n-- note\nfunction f() end");
    let result = runs::<crate::LuaCode>(&parser, true, false);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].end, RunEnd::Terminator);
    assert_eq!(
        result[0].selection,
        RunSelection::Opener(OpenerReason::FirstFile)
    );
    assert_eq!(result[1].selection, RunSelection::Unattached);
    let parser = parse(LANG::Lua, b"function f()\n---audit\nlocal x = 1\nend");
    let result = runs::<crate::LuaCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].selection, RunSelection::Unattached);
    assert_eq!(result[0].uncertainty, None);
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
#[allow(clippy::naive_bytecount)]
fn restricted(lang: LANG, source: &[u8], included: &[Range<usize>]) -> AnyParser {
    let point = |at| {
        tree_sitter::Point::new(
            source[..at].iter().filter(|&&b| b == b'\n').count(),
            source[..at]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(at, |b| at - b - 1),
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
    AnyParser::from_tree(lang, tree, source.to_vec()).unwrap()
}

#[cfg(feature = "go")]
#[test]
fn go_gaps_split_runs_before_selection_and_keep_composition_metadata() {
    let source = b"package p\n// old\nXXXX\n// new\nfunc F() {}";
    let parser = restricted(LANG::Go, source, &[0..17, 22..source.len()]);
    for retain in [false, true] {
        let result = runs::<crate::GoCode>(&parser, retain, true);
        assert_eq!(result.len(), if retain { 2 } else { 1 });
        let last = result.last().unwrap();
        assert!(last.prefix_gap);
        assert!(last.code_before.is_none());
        assert_eq!(last.selection, RunSelection::FollowingTarget);
        assert_eq!(last.uncertainty, None);
        identity(last.target.unwrap(), "function_declaration", 29..40);
        if retain {
            assert_eq!(result[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
        }
    }
    for (source, included) in [
        (b"XX// a\npackage p".as_slice(), vec![2..16]),
        (b"// a\npackage p".as_slice(), vec![0..2, 3..14]),
        (b"// a\npackage p".as_slice(), vec![0..4, 5..14]),
    ] {
        let parser = restricted(LANG::Go, source, &included);
        assert!(runs::<crate::GoCode>(&parser, false, false).is_empty());
        let result = runs::<crate::GoCode>(&parser, true, true);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].selection, RunSelection::Unattached);
        assert_eq!(result[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
    }
}

#[cfg(feature = "lua")]
#[test]
fn lua_gap_never_fabricates_first_file_context() {
    let source = b"XXXX\n---audit\nfunction f() end";
    let parser = restricted(LANG::Lua, source, &[5..source.len()]);
    assert!(runs::<crate::LuaCode>(&parser, false, false).is_empty());
    let result = runs::<crate::LuaCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert!(result[0].prefix_gap);
    assert!(result[0].code_before.is_none());
    assert_eq!(result[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn work<T: Checker + crate::traits::LanguageInfo>(prefix: &str, comment: &str, target: &str) {
    for width in [4, 64, 256] {
        let source = format!("{prefix}{}{target}", comment.repeat(width));
        let parser = parse(T::lang(), source.as_bytes());
        let before = positional_runs_collected::observed();
        let probes = positional_predecessors::observed();
        let result = runs::<T>(&parser, false, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].members.len(), width);
        // expected: one final-member scan, width-1 members and the one prefix token.
        assert_eq!(positional_runs_collected::observed() - before, 1);
        assert_eq!(positional_predecessors::observed() - probes, width);
        let before = evidence_built::observed();
        let scans = positional_runs_collected::observed();
        let probes = positional_predecessors::observed();
        let mut rows = Vec::new();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if T::is_comment(node) {
                let mut billed = false;
                super::super::positional_comment::<T>(
                    node,
                    source.as_bytes(),
                    ancestors,
                    |event| {
                        if let super::super::LegacyEvent::Comment { node: current, .. } = event {
                            assert_eq!(current.id(), node.id());
                            assert!(!billed, "each visit bills its comment once");
                            billed = true;
                        }
                        if let super::super::LegacyEvent::Coverage { kind, first, last } = event {
                            assert!(billed, "coverage follows the final comment's billing");
                            assert_eq!(
                                node.start_row(),
                                width,
                                "only the final visit credits the run"
                            );
                            assert_eq!(kind, super::super::CoverageKind::LeadingRun);
                            rows.push((first, last));
                        }
                    },
                );
                assert!(billed);
            }
        });
        assert_eq!(rows, (1..=width).map(|row| (row, row)).collect::<Vec<_>>());
        assert_eq!(evidence_built::observed() - before, 0);
        assert_eq!(positional_runs_collected::observed() - scans, 1);
        assert_eq!(positional_predecessors::observed() - probes, width);
    }
}

#[cfg(feature = "go")]
#[test]
fn go_completed_run_is_scanned_once() {
    work::<crate::GoCode>("package p\n", "// audit\n", "func F() {}");
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_completed_run_is_scanned_once() {
    work::<crate::RubyCode>("X = 1\n", "# audit\n", "def f; end");
}
#[cfg(feature = "lua")]
#[test]
fn lua_completed_run_is_scanned_once() {
    work::<crate::LuaCode>("module(\"m\")\n", "---audit\n", "function f() end");
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn raw<T: Checker + crate::traits::LanguageInfo>(source: &[u8], range: Range<usize>) {
    let parser = parse(T::lang(), source);
    let result = runs::<T>(&parser, false, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].members.len(), 1);
    assert_eq!(
        result[0].members[0].source.pieces()[0].range(),
        range.clone()
    );
    assert_eq!(
        result[0].members[0].source.pieces()[0].bytes(),
        &source[range]
    );
    assert_eq!(result[0].uncertainty, None);
}

#[cfg(feature = "go")]
#[test]
fn go_invalid_bytes_and_crlf_are_raw() {
    raw::<crate::GoCode>(b"// \xff\r\npackage p", 0..4);
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_invalid_bytes_and_crlf_are_raw() {
    raw::<crate::RubyCode>(b"# \xff\r\ndef f; end", 0..3);
}
#[cfg(feature = "lua")]
#[test]
fn lua_invalid_bytes_and_crlf_are_raw() {
    raw::<crate::LuaCode>(b"---\xff\r\nfunction f() end", 0..4);
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_gap_after_code_does_not_hide_incomplete_composition() {
    let source = b"X = 1\n# old\nXXXX\n# new\ndef f; end";
    let parser = restricted(LANG::Ruby, source, &[0..12, 17..source.len()]);
    let result = runs::<crate::RubyCode>(&parser, false, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].selection, RunSelection::FollowingTarget);
    assert!(result[0].prefix_gap);
    assert!(result[0].code_before.is_none());
    assert_eq!(result[0].uncertainty, None);
}

#[cfg(feature = "ruby")]
#[test]
fn ruby_source_and_legacy_scope_caches_disagree_in_both_orders() {
    // The conditional puts the call block above the immediate parent, so its
    // different scope states occupy cached fold levels rather than fresh steps.
    let source = b"foo do\nif true\n# audit\ndef f; end\nend\nend";
    let parser = parse(LANG::Ruby, source);
    for source_first in [false, true] {
        let context = SourceContext::new(parser.ts_tree(), source);
        let mut result = Vec::new();
        let mut credited = Vec::new();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if !crate::RubyCode::is_comment(node) {
                return;
            }
            for is_source in [source_first, !source_first] {
                if is_source {
                    source_comment::<crate::RubyCode>(
                        node,
                        ancestors,
                        &mut SourceRequest {
                            context: &context,
                            retain_unbound: true,
                            retain_directives: false,
                            sink: &mut |run| result.push(run),
                        },
                    )
                    .unwrap();
                } else {
                    super::super::positional_comment::<crate::RubyCode>(
                        node,
                        source,
                        ancestors,
                        |event| {
                            if let super::super::LegacyEvent::Coverage { first, last, .. } = event {
                                credited.push((first, last));
                            }
                        },
                    );
                }
            }
        });
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].selection, RunSelection::Unattached);
        assert!(result[0].target.is_none());
        assert_eq!(credited, [(2, 2)]);
    }
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn recovery<T: Checker + crate::traits::LanguageInfo>(source: &[u8]) {
    let parser = AnyParser::parse(T::lang(), source.to_vec(), None, None).unwrap();
    let mut errors = 0;
    parser.root_node().act_on_node(&mut |node, _| {
        if node.is_error() && node.is_extra() {
            errors += 1;
        }
    });
    assert!(errors > 0, "{}", parser.ts_tree().root_node().to_sexp());
    let result = runs::<T>(&parser, true, false);
    assert_eq!(result.len(), 2, "{result:?}");
    assert_eq!(result[0].selection, RunSelection::Unattached);
    assert_eq!(
        result[0].uncertainty,
        Some(UnresolvedReason::GrammarBoundary)
    );
    assert!(result[0].target.is_none());
    assert!(result[0].following.unwrap().node().is_error());
    assert_eq!(result[0].members.len(), 1);
    assert_eq!(result[1].members.len(), 1);
}

#[cfg(feature = "go")]
#[test]
fn go_extra_error_is_a_boundary() {
    recovery::<crate::GoCode>(b"package p\n// a\n@\n// b\nfunc F() {}");
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_extra_error_is_a_boundary() {
    recovery::<crate::RubyCode>(b"# a\n@\n# b\ndef f; end");
}
#[cfg(feature = "lua")]
#[test]
fn lua_extra_error_is_a_boundary() {
    recovery::<crate::LuaCode>(b"module(\"m\")\n-- a\n@\n---b\nfunction f() end");
}

#[cfg(feature = "go")]
#[test]
fn go_trailing_prefix_is_retained_without_seeding_documentation() {
    let parser = parse(
        LANG::Go,
        b"package p\nfunc A() {} /* t */ /* u */\n// b\nfunc B() {}",
    );
    let result = runs::<crate::GoCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]
            .members
            .iter()
            .map(|m| m.selected)
            .collect::<Vec<_>>(),
        [false, false, true]
    );
    let selected = runs::<crate::GoCode>(&parser, false, false);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].members.len(), 1);
    assert_eq!(selected[0].members[0].source.pieces()[0].bytes(), b"// b");
}

#[cfg(feature = "go")]
#[test]
fn go_missing_target_retains_uncertain_canonical_candidate() {
    let source = b"package p\n// audit\nfunc F( {}";
    let parser = AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().preorder().any(|n| n.is_missing()));
    let result = runs::<crate::GoCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].uncertainty,
        Some(UnresolvedReason::GrammarBoundary)
    );
    assert!(result[0].following.unwrap().node().has_error());
    assert_eq!(result[0].selection, RunSelection::FollowingTarget);
    identity(result[0].target.unwrap(), "function_declaration", 19..29);
    assert_eq!(result[0].members.len(), 1);
    identity(result[0].members[0].node, "comment", 10..18);
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn lazy<T: Checker + crate::traits::LanguageInfo>(source: &[u8]) {
    let parser = parse(T::lang(), source);
    let before = evidence_built::observed();
    assert!(runs::<T>(&parser, false, false).is_empty());
    assert_eq!(evidence_built::observed() - before, 0);
    let result = runs::<T>(&parser, true, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].members.len(), 1);
    assert_eq!(evidence_built::observed() - before, 1);
    assert_eq!(result[0].selection, RunSelection::Unattached);
}

#[cfg(feature = "go")]
#[test]
fn go_unattached_evidence_is_lazy() {
    lazy::<crate::GoCode>(b"package p\n// note");
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_unattached_evidence_is_lazy() {
    lazy::<crate::RubyCode>(b"# note");
}
#[cfg(feature = "lua")]
#[test]
fn lua_unattached_evidence_is_lazy() {
    lazy::<crate::LuaCode>(b"-- note");
}

#[cfg(feature = "go")]
#[test]
fn go_directive_only_evidence_is_lazy_and_not_prose() {
    let parser = parse(LANG::Go, b"//go:generate tool\npackage p");
    let before = evidence_built::observed();
    assert!(runs::<crate::GoCode>(&parser, false, false).is_empty());
    assert_eq!(evidence_built::observed() - before, 0);
    let result = runs::<crate::GoCode>(&parser, false, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].members.len(), 1);
    assert!(result[0].members[0].directive);
    assert!(!result[0].members[0].selected);
    identity(result[0].target.unwrap(), "package_clause", 19..28);
}
