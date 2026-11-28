use super::*;

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn allocation_free<T: Checker + crate::traits::LanguageInfo>(
    prefix: &str,
    comment: &str,
    target: &str,
) {
    for source in [
        format!("{prefix}{comment}{target}"),
        format!("{prefix}{comment}"),
        format!("{prefix}{target} {comment}"),
        format!("{prefix}{}{target}", format!("{comment}\n").repeat(64)),
        format!("{prefix}{}{target}", comment.repeat(256)),
    ] {
        let parser = parse(T::lang(), source.as_bytes());
        let before = members_materialized::observed();
        let scope_reads = source_scope_reads::observed();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if T::is_comment(node) {
                super::super::super::positional_comment::<T>(
                    node,
                    source.as_bytes(),
                    ancestors,
                    |_| {},
                );
            }
        });
        assert_eq!(members_materialized::observed() - before, 0, "{source}");
        assert_eq!(
            source_scope_reads::observed() - scope_reads,
            0,
            "legacy does not ask source scope bounds"
        );
        let expected = parser.root_node().preorder().filter(T::is_comment).count();
        assert!(expected > 0);
        let source_runs = runs::<T>(&parser, true, true);
        assert!(!source_runs.is_empty());
        // Every actual source node-buffer push is counted once, including rejected runs.
        assert_eq!(
            members_materialized::observed() - before,
            expected,
            "{source}"
        );
    }
}

#[cfg(feature = "go")]
#[test]
fn go_legacy_materializes_no_members() {
    allocation_free::<crate::GoCode>("package p\n", "// a\n", "func f() {}");
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_legacy_materializes_no_members() {
    allocation_free::<crate::RubyCode>("X = 1\n", "# a\n", "def f; end");
}
#[cfg(feature = "lua")]
#[test]
fn lua_legacy_materializes_no_members() {
    allocation_free::<crate::LuaCode>("module(\"m\")\n", "---a\n", "function f() end");
}

#[cfg(any(feature = "go", feature = "ruby", feature = "lua"))]
fn suffix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    end: usize,
    selected: RunSelection,
) {
    for retained_end in [end - 1, end] {
        let restricted = restricted(T::lang(), source, &[0..retained_end]);
        let truncated = parse(T::lang(), &source[..retained_end]);
        for retain in [false, true] {
            let restricted_runs = runs::<T>(&restricted, retain, true);
            let truncated_runs = runs::<T>(&truncated, retain, true);
            let count = usize::from(retain || selected != RunSelection::Unattached);
            assert_eq!(restricted_runs.len(), count);
            assert_eq!(truncated_runs.len(), count);
            if count == 0 {
                continue;
            }
            let unknown = &restricted_runs[0];
            let known = &truncated_runs[0];
            assert!(unknown.suffix_gap);
            assert!(!known.suffix_gap);
            assert_eq!(unknown.selection, selected);
            assert_eq!(known.selection, selected);
            assert_eq!(unknown.uncertainty, Some(UnresolvedReason::ExcludedGap));
            assert_eq!(known.uncertainty, None);
            assert_eq!(unknown.end, RunEnd::EndOfScope);
            assert!(unknown.following.is_none());
            assert!(unknown.target.is_none());
            assert_eq!(unknown.members.len(), 1);
            let start = source[..end - 1]
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(0, |at| at + 1);
            identity(unknown.members[0].node, "comment", start..end - 1);
            assert_eq!(
                unknown.members[0].source.pieces()[0].range(),
                start..end - 1
            );
            assert_eq!(
                unknown.members[0].source.pieces()[0].bytes(),
                known.members[0].source.pieces()[0].bytes()
            );
        }
    }
}

#[cfg(feature = "go")]
#[test]
fn go_exhaustion_reports_unknown_suffix() {
    suffix::<crate::GoCode>(
        b"package p\n// audit\nfunc f() {}",
        19,
        RunSelection::Unattached,
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_exhaustion_preserves_explicit_selection() {
    suffix::<crate::RubyCode>(b"# audit\ndef f; end", 8, RunSelection::Unattached);
    suffix::<crate::RubyCode>(
        b"# @!method ghost\ndef f; end",
        17,
        RunSelection::Opener(OpenerReason::Explicit),
    );
}
#[cfg(feature = "lua")]
#[test]
fn lua_exhaustion_preserves_known_file_and_tagged_selection() {
    suffix::<crate::LuaCode>(
        b"--- audit\nfunction f() end",
        10,
        RunSelection::Opener(OpenerReason::FirstFile),
    );
    suffix::<crate::LuaCode>(
        b"module(\"m\")\n---@function ghost\nfunction f() end",
        31,
        RunSelection::Opener(OpenerReason::Tagged),
    );
}

#[cfg(any(feature = "go", feature = "ruby"))]
fn partial_prefix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    expected_members: &[Range<usize>],
    target: &(&str, Range<usize>),
) {
    let parser = restricted(T::lang(), source, &[2..source.len()]);
    assert!(!parser.ts_tree().root_node().has_error());
    for retain in [false, true] {
        let result = runs::<T>(&parser, retain, true);
        assert_eq!(result.len(), 1);
        let run = &result[0];
        assert_eq!(run.selection, RunSelection::FollowingTarget);
        assert!(run.prefix_gap);
        assert!(run.code_before.is_none());
        assert_eq!(run.uncertainty, Some(UnresolvedReason::ExcludedGap));
        identity(run.target.unwrap(), target.0, target.1.clone());
        let expected = if retain {
            expected_members
        } else {
            &expected_members[expected_members.len() - 1..]
        };
        assert_eq!(run.members.len(), expected.len());
        for (index, (member, range)) in run.members.iter().zip(expected).enumerate() {
            identity(member.node, "comment", range.clone());
            assert_eq!(member.selected, index == expected.len() - 1);
            assert_eq!(member.source.pieces()[0].bytes(), &source[range.clone()]);
        }
    }
    let twin = parse(T::lang(), &source[2..]);
    let result = runs::<T>(&twin, false, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].members.len(), expected_members.len());
    assert!(result[0].members.iter().all(|member| member.selected));
    assert_eq!(result[0].uncertainty, None);
}

#[cfg(feature = "go")]
#[test]
fn go_unknown_first_row_keeps_clean_suffix() {
    partial_prefix::<crate::GoCode>(
        b"XX// a\n// b\npackage p",
        &[2..6, 7..11],
        &("package_clause", 12..21),
    );
    partial_prefix::<crate::GoCode>(
        b"XX/*a*/ /*b*/\n// c\npackage p",
        &[2..7, 8..13, 14..18],
        &("package_clause", 19..28),
    );
}
#[cfg(feature = "ruby")]
#[test]
fn ruby_unknown_first_row_keeps_clean_suffix() {
    partial_prefix::<crate::RubyCode>(
        b"XX# a\n# b\ndef f; end",
        &[2..5, 6..9],
        &("method", 10..20),
    );
}

#[cfg(feature = "ruby")]
#[test]
fn yard_later_directives_follow_trivia_or_prose() {
    for (prefix, directive) in [
        ("#!/ruby\n", true),
        ("# prose\n", false),
        ("# frozen_string_literal: true\n", true),
    ] {
        let source = format!("{prefix}# @!method ghost\n#   audit\n");
        let parser = parse(LANG::Ruby, source.as_bytes());
        for retain in [false, true] {
            let result = runs::<crate::RubyCode>(&parser, retain, true);
            assert_eq!(result.len(), 1);
            let run = &result[0];
            assert_eq!(run.selection, RunSelection::Opener(OpenerReason::Explicit));
            assert!(run.target.is_none());
            assert_eq!(run.uncertainty, None);
            assert_eq!(run.members.len(), 3);
            assert_eq!(run.members[0].directive, directive);
            assert_eq!(run.members[0].selected, !directive);
            identity(
                run.members[1].node,
                "comment",
                prefix.len()..prefix.len() + 16,
            );
            assert_eq!(
                run.members[1].source.pieces()[0].bytes(),
                b"# @!method ghost"
            );
        }
        let mut rows = Vec::new();
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if crate::RubyCode::is_comment(node) {
                super::super::super::positional_comment::<crate::RubyCode>(
                    node,
                    source.as_bytes(),
                    ancestors,
                    |event| {
                        if let super::super::super::LegacyEvent::Coverage { first, last, .. } =
                            event
                        {
                            rows.push((first, last));
                        }
                    },
                );
            }
        });
        assert!(rows.is_empty(), "legacy remains first-member only");
    }
}

#[cfg(feature = "ruby")]
#[test]
fn yard_later_directive_cleanup_is_bounded() {
    for (line, selected) in [
        ("# @!method ghost", true),
        ("##\t@!method ghost", true),
        ("# @!a.b_2. ghost", true),
        ("# @!", false),
        ("# @!method-bad", false),
        ("# @!a..b", false),
        ("#  @!method ghost", false),
        ("#\t\t@!method ghost", false),
        ("# prose @!method ghost", false),
        ("##", false),
    ] {
        let source = format!("# prose\n{line}\n");
        let parser = parse(LANG::Ruby, source.as_bytes());
        let result = runs::<crate::RubyCode>(&parser, false, false);
        assert_eq!(result.len(), usize::from(selected), "{line}");
    }
    for source in [
        b"X = 1 # @!method ghost\n# prose".as_slice(),
        b"foo do\n# prose\n# @!method ghost\nend",
    ] {
        let parser = parse(LANG::Ruby, source);
        assert!(runs::<crate::RubyCode>(&parser, false, false).is_empty());
    }
    let source = b"XX# @!method ghost\n# prose";
    let parser = restricted(LANG::Ruby, source, &[2..source.len()]);
    assert!(runs::<crate::RubyCode>(&parser, false, false).is_empty());
    let result = runs::<crate::RubyCode>(&parser, true, false);
    assert_eq!(result.len(), 1);
    assert!(result[0].members.iter().all(|member| !member.selected));
}

#[cfg(feature = "ruby")]
#[test]
fn nested_positional_exhaustion_does_not_read_the_file_tail() {
    let source = b"class C\n# @!method ghost\nend\nXXXX";
    let parser = restricted(LANG::Ruby, source, &[0..28]);
    let result = runs::<crate::RubyCode>(&parser, false, false);
    assert_eq!(result.len(), 1);
    assert!(!result[0].suffix_gap);
    assert_eq!(
        result[0].selection,
        RunSelection::Opener(OpenerReason::Explicit)
    );
    assert_eq!(result[0].uncertainty, None);
}
