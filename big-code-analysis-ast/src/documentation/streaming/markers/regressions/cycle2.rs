use super::*;

#[cfg(any(feature = "rust", feature = "java"))]
fn chainless_runs<T: Checker>(parser: &AnyParser, retain: bool) -> Vec<MarkerRun<'_>> {
    let context = SourceContext::new(parser.ts_tree(), parser.code());
    let mut result = Vec::new();
    parser.root_node().act_on_node(&mut |node, _| {
        if T::is_comment(node) {
            source_comment::<T>(
                node,
                Ancestors::unknown(),
                &mut SourceRequest {
                    context: &context,
                    retain_unbound: retain,
                    sink: &mut |run| result.push(run),
                },
            )
            .unwrap();
        }
    });
    result
}

#[cfg(any(feature = "rust", feature = "java"))]
fn prefix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    kind: &str,
    span: Range<usize>,
    target: &(&str, Range<usize>),
) {
    let parser = restricted(T::lang(), source, &[2..source.len()]);
    for retain in [false, true] {
        for result in [
            runs::<T>(&parser, retain),
            chainless_runs::<T>(&parser, retain),
        ] {
            assert_eq!(result.len(), 1);
            let run = &result[0];
            assert!(run.prefix_gap);
            assert!(!run.suffix_gap);
            assert!(run.code_before.is_none());
            assert_eq!(run.members.len(), 1);
            assert_eq!(run.members[0].selection, MarkerSelection::Selected);
            assert_eq!(run.members[0].node.node().kind(), kind);
            assert_eq!(run.members[0].source.pieces()[0].range(), span);
            let candidate = run.target.unwrap().node();
            assert_eq!(candidate.kind(), target.0);
            assert_eq!(candidate.byte_range(), target.1);
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_chainless_marker_prefix_uses_full_buffer() {
    prefix::<crate::RustCode>(
        b"XX//! audit\nfn f() {}",
        "line_comment",
        2..11,
        &("function_item", 12..21),
    );
}
#[cfg(feature = "java")]
#[test]
fn java_chainless_marker_prefix_uses_full_buffer() {
    prefix::<crate::JavaCode>(
        b"XX/// audit\nclass C {}",
        "line_comment",
        2..11,
        &("class_declaration", 12..22),
    );
}

#[cfg(any(feature = "rust", feature = "java"))]
fn suffix<T: Checker + crate::traits::LanguageInfo>(
    source: &[u8],
    end: usize,
    selection: MarkerSelection,
) {
    let parser = restricted(T::lang(), source, &[0..end]);
    for retain in [false, true] {
        for result in [
            runs::<T>(&parser, retain),
            chainless_runs::<T>(&parser, retain),
        ] {
            assert_eq!(
                result.len(),
                usize::from(retain || selection == MarkerSelection::Selected)
            );
            if result.is_empty() {
                continue;
            }
            assert!(result[0].suffix_gap);
            assert_eq!(result[0].members.len(), 1);
            assert_eq!(result[0].members[0].selection, selection);
            assert_eq!(
                result[0].members[0].uncertainty,
                Some(UnresolvedReason::ExcludedGap)
            );
            assert!(result[0].target.is_none());
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_chainless_marker_suffix_uses_full_buffer() {
    suffix::<crate::RustCode>(b"//! audit\nfn f() {}", 10, MarkerSelection::Selected);
}
#[cfg(feature = "java")]
#[test]
fn java_chainless_marker_suffix_uses_full_buffer() {
    suffix::<crate::JavaCode>(b"/// audit\nclass C {}", 10, MarkerSelection::Unattached);
}
