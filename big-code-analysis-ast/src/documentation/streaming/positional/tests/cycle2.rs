use super::*;

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn collect<T: Checker>(parser: &AnyParser, retain: bool, chainless: bool) -> Vec<SourceRun<'_>> {
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut result = Vec::new();
    parser.root_node().act_on_node(&mut |node, ancestors| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                if chainless {
                    Ancestors::unknown()
                } else {
                    ancestors
                },
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: retain,
                    retain_directives: true,
                    sink: &mut |run| result.push(run),
                },
            )
            .unwrap();
        }
    });
    result
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn suffix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    end: usize,
    selection: RunSelection,
) {
    let parser = restricted(T::lang(), source, &[0..end]);
    assert!(!parser.ts_tree().root_node().has_error());
    for chainless in [false, true] {
        for retain in [false, true] {
            let result = collect::<T>(&parser, retain, chainless);
            assert_eq!(
                result.len(),
                usize::from(retain || selection != RunSelection::Unattached)
            );
            if result.is_empty() {
                continue;
            }
            let run = &result[0];
            assert!(run.suffix_gap);
            assert!(!run.prefix_gap);
            assert_eq!(run.selection, selection);
            assert_eq!(run.uncertainty, Some(UnresolvedReason::ExcludedGap));
            assert!(run.target.is_none());
            assert!(run.following.is_none());
            assert_eq!(run.members.len(), 1);
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_chainless_exhaustion_keeps_full_buffer_suffix() {
    suffix::<crate::GoCode>(
        b"package p\n// audit\nfunc f() {}",
        19,
        RunSelection::Unattached,
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_chainless_exhaustion_keeps_full_buffer_suffix() {
    suffix::<crate::RubyCode>(b"# audit\ndef f; end", 8, RunSelection::Unattached);
}
#[cfg(feature = "lua")]
#[test]
fn lua_chainless_exhaustion_keeps_full_buffer_suffix() {
    suffix::<crate::LuaCode>(
        b"--- audit\nfunction f() end",
        10,
        RunSelection::Opener(OpenerReason::FirstFile),
    );
}

#[cfg(any(feature = "go", feature = "ruby"))]
fn prefix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    spans: &[Range<usize>],
    target: &(&str, Range<usize>),
) {
    let parser = restricted(T::lang(), source, &[2..source.len()]);
    assert!(!parser.ts_tree().root_node().has_error());
    for chainless in [false, true] {
        for retain in [false, true] {
            let result = collect::<T>(&parser, retain, chainless);
            assert_eq!(result.len(), 1);
            let run = &result[0];
            assert!(run.prefix_gap);
            assert!(!run.suffix_gap);
            assert!(run.code_before.is_none());
            assert_eq!(run.uncertainty, Some(UnresolvedReason::ExcludedGap));
            assert_eq!(run.selection, RunSelection::FollowingTarget);
            identity(run.target.unwrap(), target.0, target.1.clone());
            let expected = if retain { spans } else { &spans[1..] };
            assert_eq!(run.members.len(), expected.len());
            for (member, span) in run.members.iter().zip(expected) {
                identity(member.node, "comment", span.clone());
                assert_eq!(member.selected, span != &spans[0]);
                assert_eq!(member.source.pieces()[0].range(), *span);
            }
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_chainless_prefix_keeps_unknown_first_row() {
    prefix::<crate::GoCode>(
        b"XX// a\n// b\npackage p",
        &[2..6, 7..11],
        &("package_clause", 12..21),
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_chainless_prefix_keeps_unknown_first_row() {
    prefix::<crate::RubyCode>(
        b"XX# a\n# b\ndef f; end",
        &[2..5, 6..9],
        &("method", 10..20),
    );
}

#[cfg(feature = "go")]
#[test]
fn uncertain_multiline_prefix_excludes_all_row_connected_members() {
    for (source, spans, target) in [
        (
            b"XX/*a\nb*/ /*c*/\n// d\npackage p".as_slice(),
            vec![2..9, 10..15, 16..20],
            21..30,
        ),
        (
            b"XX/*a\nb*/ /*c\nd*/ /*e*/\n// f\npackage p".as_slice(),
            vec![2..9, 10..17, 18..23, 24..28],
            29..38,
        ),
    ] {
        let parser = restricted(LANG::Go, source, &[2..source.len()]);
        assert!(!parser.ts_tree().root_node().has_error());
        for chainless in [false, true] {
            for retain in [false, true] {
                let result = collect::<crate::GoCode>(&parser, retain, chainless);
                assert_eq!(result.len(), 1);
                let run = &result[0];
                assert_eq!(run.selection, RunSelection::FollowingTarget);
                assert_eq!(run.uncertainty, Some(UnresolvedReason::ExcludedGap));
                assert!(run.prefix_gap);
                identity(run.target.unwrap(), "package_clause", target.clone());
                let expected = if retain {
                    &spans[..]
                } else {
                    &spans[spans.len() - 1..]
                };
                assert_eq!(run.members.len(), expected.len());
                for (member, span) in run.members.iter().zip(expected) {
                    identity(member.node, "comment", span.clone());
                    assert_eq!(member.selected, span == spans.last().unwrap());
                    assert_eq!(member.source.pieces()[0].bytes(), &source[span.clone()]);
                }
            }
        }
        let twin = parse(LANG::Go, &source[2..]);
        let result = collect::<crate::GoCode>(&twin, false, false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].members.len(), spans.len());
        assert!(result[0].members.iter().all(|member| member.selected));
        assert_eq!(result[0].uncertainty, None);
    }
    let source = b"XX/*a\nb*/ /*c*/\npackage p";
    let parser = restricted(LANG::Go, source, &[2..source.len()]);
    for chainless in [false, true] {
        assert!(collect::<crate::GoCode>(&parser, false, chainless).is_empty());
        let result = collect::<crate::GoCode>(&parser, true, chainless);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].selection, RunSelection::Unattached);
        assert!(result[0].members.iter().all(|member| !member.selected));
        identity(result[0].target.unwrap(), "package_clause", 16..25);
        assert_eq!(result[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
    }
    let parser = parse(
        LANG::Go,
        b"package p\nvar x = 0 /*a\nb*/ /*c*/\n// d\nvar y = 1",
    );
    let result = collect::<crate::GoCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0]
            .members
            .iter()
            .map(|member| member.selected)
            .collect::<Vec<_>>(),
        [false, false, true]
    );
    assert_eq!(result[0].uncertainty, None);
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn trailing<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    span: Range<usize>,
    target: &(&str, Range<usize>),
) {
    let parser = parse(T::lang(), source);
    for chainless in [false, true] {
        assert!(collect::<T>(&parser, false, chainless).is_empty());
        let result = collect::<T>(&parser, true, chainless);
        assert_eq!(result.len(), 1);
        let run = &result[0];
        assert_eq!(run.selection, RunSelection::Unattached);
        assert_eq!(run.uncertainty, None);
        assert_eq!(run.members.len(), 1);
        assert!(!run.members[0].selected);
        identity(run.members[0].node, "comment", span.clone());
        identity(run.target.unwrap(), target.0, target.1.clone());
        identity(run.following.unwrap(), target.0, target.1.clone());
    }
    let parser = restricted(
        T::lang(),
        source,
        &[0..target.1.start + 1, target.1.start + 2..source.len()],
    );
    let result = collect::<T>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].selection, RunSelection::Unattached);
    assert!(result[0].target.is_none());
    assert_eq!(result[0].uncertainty, Some(UnresolvedReason::ExcludedGap));
}

#[cfg(feature = "go")]
#[test]
fn go_trailing_only_keeps_candidate_without_selection() {
    trailing::<crate::GoCode>(
        b"package p\nvar x = 1 // note\nfunc F() {}",
        20..27,
        &("function_declaration", 28..39),
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_trailing_only_keeps_candidate_without_selection() {
    trailing::<crate::RubyCode>(b"X = 1 # note\ndef f; end", 6..12, &("method", 13..23));
}
#[cfg(feature = "lua")]
#[test]
fn lua_trailing_only_keeps_candidate_without_selection() {
    trailing::<crate::LuaCode>(
        b"x = 1 -- note\nfunction f() end",
        6..13,
        &("function_declaration", 14..30),
    );
}

#[cfg(feature = "go")]
#[test]
fn recovered_trailing_candidate_keeps_grammar_uncertainty() {
    let source = b"package p\nvar x = 1 // note\nfunc F( {}";
    let parser = AnyParser::parse(LANG::Go, source.to_vec(), None, None).unwrap();
    assert!(parser.root_node().preorder().any(|node| node.is_missing()));
    let result = collect::<crate::GoCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].selection, RunSelection::Unattached);
    assert_eq!(
        result[0].uncertainty,
        Some(UnresolvedReason::GrammarBoundary)
    );
    identity(result[0].target.unwrap(), "function_declaration", 28..38);
    assert_eq!(result[0].members.len(), 1);
    assert!(!result[0].members[0].selected);
}
