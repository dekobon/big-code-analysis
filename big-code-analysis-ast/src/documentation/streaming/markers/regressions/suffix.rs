use super::*;

#[cfg(any(feature = "rust", feature = "java", feature = "c"))]
fn check_suffix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    end: usize,
    selection: MarkerSelection,
) {
    for included_end in [end - 1, end] {
        let parser = restricted(T::lang(), source, &[0..included_end]);
        let truncated =
            AnyParser::parse(T::lang(), source[..included_end].to_vec(), None, None).unwrap();
        for retain in [false, true] {
            let result = runs::<T>(&parser, retain);
            let control = runs::<T>(&truncated, retain);
            let count = usize::from(retain || selection == MarkerSelection::Selected);
            assert_eq!(result.len(), count);
            assert_eq!(control.len(), count);
            if count == 0 {
                continue;
            }
            let run = &result[0];
            assert!(run.suffix_gap);
            assert!(!control[0].suffix_gap);
            assert!(run.target.is_none());
            assert_eq!(run.members.len(), 1);
            assert_eq!(run.members[0].selection, selection);
            assert_eq!(
                run.members[0].uncertainty,
                Some(UnresolvedReason::ExcludedGap)
            );
            assert_eq!(control[0].members[0].uncertainty, None);
            assert_eq!(
                run.members[0].source.pieces()[0].bytes(),
                control[0].members[0].source.pieces()[0].bytes()
            );
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_local_selection_keeps_suffix_composition_gap() {
    check_suffix::<crate::RustCode>(b"//! audit\nfn f() {}", 10, MarkerSelection::Selected);
}
#[cfg(feature = "java")]
#[test]
fn java_unattached_exhaustion_keeps_unknown_suffix() {
    check_suffix::<crate::JavaCode>(b"/// audit\nclass C {}", 10, MarkerSelection::Unattached);
}
#[cfg(feature = "c")]
#[test]
fn c_gatherer_keeps_selected_suffix_evidence() {
    check_suffix::<crate::CCode>(b"/** @file */\nint x;", 13, MarkerSelection::Selected);
}

#[cfg(feature = "c")]
#[test]
fn gathered_markers_remain_selected_with_incomplete_suffix() {
    let source = b"/** prose */\n/** @file */\nint x;";
    let parser = restricted(LANG::C, source, &[0..25]);
    for retain in [false, true] {
        let result = runs::<crate::CCode>(&parser, retain);
        assert_eq!(result.len(), 1);
        let run = &result[0];
        assert!(run.suffix_gap);
        assert!(run.target.is_none());
        assert_eq!(run.members.len(), 2);
        assert!(matches!(
            run.members[0].placement,
            MarkerPlacement::Gathered(_)
        ));
        assert!(matches!(
            run.members[1].placement,
            MarkerPlacement::Gatherer
        ));
        for (member, span) in run.members.iter().zip([0..12, 13..25]) {
            assert_eq!(member.node.node().byte_range(), span);
            assert_eq!(member.source.pieces()[0].range(), span);
            assert_eq!(member.selection, MarkerSelection::Selected);
            assert_eq!(member.uncertainty, Some(UnresolvedReason::ExcludedGap));
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn marker_exhaustion_checks_after_final_skipped_trivia() {
    let source = b"//! audit\n#[allow(dead_code)]\nfn f() {}";
    let end = 29;
    let parser = restricted(LANG::Rust, source, &[0..end]);
    for retain in [false, true] {
        let result = runs::<crate::RustCode>(&parser, retain);
        assert_eq!(result.len(), 1);
        assert!(result[0].suffix_gap);
        assert!(result[0].target.is_none());
        assert_eq!(result[0].members.len(), 1);
        assert_eq!(result[0].members[0].selection, MarkerSelection::Selected);
        assert_eq!(
            result[0].members[0].uncertainty,
            Some(UnresolvedReason::ExcludedGap)
        );
    }
    // The excluded attribute gap is not the exhaustion tail: the last examined
    // sibling is the attribute, whose included end reaches actual EOF here.
    let truncated = AnyParser::parse(LANG::Rust, source[..end].to_vec(), None, None).unwrap();
    let result = runs::<crate::RustCode>(&truncated, false);
    assert_eq!(result.len(), 1);
    assert!(!result[0].suffix_gap);
    assert_eq!(result[0].members[0].uncertainty, None);
}

#[cfg(feature = "rust")]
#[test]
fn nested_marker_exhaustion_uses_lexical_scope_not_file_tail() {
    let source = b"mod m { //! audit\n}\nXXXX";
    let parser = restricted(LANG::Rust, source, &[0..20]);
    let result = runs::<crate::RustCode>(&parser, false);
    assert_eq!(result.len(), 1);
    assert!(!result[0].suffix_gap);
    assert_eq!(result[0].members[0].selection, MarkerSelection::Selected);
    assert_eq!(result[0].members[0].uncertainty, None);
}
