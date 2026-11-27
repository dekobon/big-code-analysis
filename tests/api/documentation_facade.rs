//! The public `Ast::documentation`: the context and the options it hands to the
//! documentation collector of `big-code-analysis-ast`, the index and the errors it
//! returns, and the buffer and tree the index refers to.
//!
//! Values that the extraction contract (`docs/development/documentation_extraction.md`)
//! or the source bytes fix are written out. What a language unit may still change
//! (pending assessments, roles, publication) is compared with the collector called
//! directly on the same tree and bytes, never written out.

#![allow(clippy::single_range_in_vec_init, clippy::naive_bytecount)]

#[cfg(feature = "java")]
#[test]
fn java_path_provenance_decides_package_publication_but_display_labels_do_not() {
    use big_code_analysis::LANG;
    use big_code_analysis::{Ast as JavaAst, Source as JavaSource};
    use big_code_analysis_ast::documentation::{
        DocumentationContext as JavaContext, DocumentationOptions as JavaOptions, Publication,
    };
    let java_source = b"/**pkg*/\npackage p;\nclass C {}\n";
    let directory = tempfile::tempdir().unwrap();
    for (name, expected) in [
        ("package-info.java", Publication::Eligible),
        ("Other.java", Publication::Ineligible),
    ] {
        let path = directory.path().join(name);
        std::fs::write(&path, java_source).unwrap();
        let ast = JavaAst::from_path(&path).unwrap();
        let index = ast
            .documentation(&JavaContext::default(), &JavaOptions::default())
            .unwrap();
        assert_eq!(index.documents().len(), 1);
        assert_eq!(
            index.associations().next().unwrap().get().publication,
            expected
        );
        let explicit = JavaContext::default().with_logical_filename("Other.java");
        assert_eq!(
            ast.documentation(&explicit, &JavaOptions::default())
                .unwrap()
                .associations()
                .next()
                .unwrap()
                .get()
                .publication,
            Publication::Ineligible
        );
    }
    let labelled = JavaAst::parse(
        JavaSource::new(LANG::Java, java_source).with_name(Some("package-info.java".to_owned())),
    )
    .unwrap();
    let index = labelled
        .documentation(&JavaContext::default(), &JavaOptions::default())
        .unwrap();
    assert_eq!(index.documents().len(), 1);
    assert_eq!(
        index.associations().next().unwrap().get().publication,
        Publication::Unknown
    );
}

#[cfg(feature = "rust")]
use std::ops::Range;
#[cfg(feature = "rust")]
use std::path::Path;

#[cfg(feature = "rust")]
use big_code_analysis::{
    Ast, AstCfg, FromPathError, LANG, Metric, MetricsOptions, ParseCancel, Source,
    documentation::{
        CandidateDisposition, CollectionError, CompleteText, DocumentationContext,
        DocumentationIndex, DocumentationOptions, FactError, FactRef, Fragment, FragmentForm,
        NodeEvidence, Origin, OriginView, Owner, OwnerError, Profile, Resolution, SourceError,
        SourcePiece, SourcePieces,
    },
    tree_sitter,
};
#[cfg(feature = "rust")]
use big_code_analysis_ast::documentation::{collect, observations};

/// The contract's primary row for Rust.
#[cfg(feature = "rust")]
const ROSTER: &[u8] = b"///audit\nfn f() {}";

/// Two dangling documentation comments: no item follows either.
#[cfg(feature = "rust")]
const DANGLING: &[u8] = b"fn f() {\n///a\n}\n///b\n";

/// The contract's source for the independence of documentation and metric pruning.
#[cfg(feature = "rust")]
const TESTS_AND_CODE: &[u8] = b"/// outer\n#[test]\nfn test_f() {}\n\n/// kept\nfn f() {}\n";

/// The contract's source for an excluded gap, then a plain function and a documented one.
#[cfg(feature = "rust")]
const GAP_THEN_DOCUMENTED: &[u8] = b"///x\n\nfn f() {}\nfn h() {}\n///y\nfn g() {}";

/// One run of a document's text: its output range, and the source range it copies or
/// `None` where the text is inserted.
#[cfg(feature = "rust")]
type Run = (Range<usize>, Option<Range<usize>>);

/// One document with the function that owns it: the fragments as source ranges, the
/// text, its runs, and the owner's range.
#[cfg(feature = "rust")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<Run>,
    owner: Range<usize>,
}

#[cfg(feature = "rust")]
fn parsed(source: &[u8]) -> Ast {
    let ast = Ast::parse(Source::new(LANG::Rust, source)).unwrap();
    assert!(!ast.as_tree_sitter().root_node().has_error());
    ast
}

/// A tree parsed from the included ranges only, adopted with `label` as display name.
#[cfg(feature = "rust")]
fn adopted(source: &[u8], included: &[Range<usize>], label: Option<&str>) -> Ast {
    let point = |offset: usize| {
        let before = &source[..offset];
        let row = before.iter().filter(|byte| **byte == b'\n').count();
        let column = before
            .iter()
            .rev()
            .take_while(|byte| **byte != b'\n')
            .count();
        tree_sitter::Point::new(row, column)
    };
    let ranges: Vec<_> = included
        .iter()
        .map(|range| tree_sitter::Range {
            start_byte: range.start,
            end_byte: range.end,
            start_point: point(range.start),
            end_point: point(range.end),
        })
        .collect();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&LANG::Rust.tree_sitter_language().unwrap())
        .unwrap();
    parser.set_included_ranges(&ranges).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    Ast::from_tree_sitter(LANG::Rust, tree, source.to_vec(), label.map(str::to_owned)).unwrap()
}

#[cfg(feature = "rust")]
fn named(filename: &str) -> DocumentationContext {
    DocumentationContext::default().with_logical_filename(filename)
}

#[cfg(feature = "rust")]
fn unbound() -> DocumentationOptions {
    DocumentationOptions::default().with_unbound_evidence(true)
}

/// The index under no context and default options.
#[cfg(feature = "rust")]
fn collected(ast: &Ast) -> DocumentationIndex<'_> {
    ast.documentation(
        &DocumentationContext::default(),
        &DocumentationOptions::default(),
    )
    .unwrap()
}

#[cfg(feature = "rust")]
fn rust_logical_filename<'i>(index: &'i DocumentationIndex<'_>) -> Option<&'i str> {
    index.context().logical_filename.as_deref()
}

/// The one node of this kind and byte range.
#[cfg(feature = "rust")]
fn node_at<'t>(
    tree: &'t tree_sitter::Tree,
    kind: &str,
    range: Range<usize>,
) -> tree_sitter::Node<'t> {
    let mut found = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(current) = stack.pop() {
        if current.kind() == kind && current.byte_range() == range {
            found.push(current);
        }
        let mut cursor = current.walk();
        stack.extend(current.children(&mut cursor));
    }
    assert_eq!(found.len(), 1, "{kind} {range:?}");
    found[0]
}

#[cfg(feature = "rust")]
fn spelled(node: tree_sitter::Node<'_>) -> String {
    format!("{} {}..{}", node.kind(), node.start_byte(), node.end_byte())
}

#[cfg(feature = "rust")]
fn spans(pieces: &SourcePieces<'_>) -> Vec<Range<usize>> {
    pieces.pieces().iter().map(SourcePiece::range).collect()
}

#[cfg(feature = "rust")]
fn owner_text<'s>(index: &DocumentationIndex<'s>, owner: FactRef<'_, Owner<'s>>) -> String {
    match owner.get() {
        Owner::Node(declared) => format!("Node({})", spelled(declared.node())),
        Owner::File(root) => format!("File({})", spelled(root.node())),
        Owner::Slot(slot) => format!(
            "Slot({:?} #{} {:?} in {})",
            slot.role,
            slot.ordinal,
            spans(&slot.source),
            owner_text(index, index.facts().slot_enclosing(owner).unwrap())
        ),
        other => format!("{other:?}"),
    }
}

/// What an index publishes, one line per record and in the index's own order: the
/// resolved profile, the capability, the options, the context and the number of
/// qualification records; every document with its fragments and its text and runs, or
/// its content state; every association; every pending assessment; every retained
/// candidate; every owner. Node queries are not part of it.
#[cfg(feature = "rust")]
fn published(index: &DocumentationIndex<'_>) -> Vec<String> {
    // bca: suppress(halstead) -- one flat reading of the index, record by record.
    let facts = index.facts();
    let mut lines = vec![format!(
        "{:?} {:?} {:?} {:?} qualifications {}",
        index.profile(),
        index.capability(),
        index.options(),
        index.context(),
        index.qualifications().len()
    )];
    let documents: Vec<_> = index.documents().collect();
    let position = |wanted| documents.iter().position(|known| *known == wanted);
    for document in &documents {
        let record = document.get();
        lines.push(format!(
            "document {:?} {:?}",
            record.role(),
            record.composition()
        ));
        for at in 0..record.fragment_count() {
            let fragment = facts.document_fragment(*document, at).unwrap();
            let fragment = fragment.get();
            lines.push(format!(
                "  fragment {:?} gap {} {:?} {:?}",
                spans(fragment.source()),
                fragment.source().has_excluded_gap(),
                fragment.form(),
                fragment.node().map(|carrier| spelled(carrier.node()))
            ));
        }
        match index.complete_text(*document).unwrap() {
            Some(complete) => {
                lines.push(format!("  text {:?}", complete.text()));
                for run in complete.mappings() {
                    lines.push(format!("  map {:?} {:?}", run.output.range(), run.origin));
                }
            }
            None => lines.push(format!("  content {:?}", record.content())),
        }
    }
    for association in index.associations() {
        let record = association.get();
        let owners: Vec<_> = (0..record.owner_count())
            .map(|at| owner_text(index, facts.associated_owner(association, at).unwrap()))
            .collect();
        lines.push(format!(
            "association {:?} {:?} {} {:?} binding {:?} scope {} owners {owners:?}",
            position(facts.associated_document(association).unwrap()),
            record.resolution,
            record.convention.name(),
            record.publication,
            record.binding.map(|wrapper| spelled(wrapper.node())),
            spelled(record.lexical_scope.node())
        ));
    }
    for assessment in index.assessments() {
        let record = assessment.get();
        lines.push(format!(
            "assessment {:?} {:?} {:?} {:?}",
            record.axis,
            record.reason,
            record.node.map(|assessed| spelled(assessed.node())),
            index
                .assessed_document(assessment)
                .unwrap()
                .and_then(position)
        ));
    }
    for candidate in index.evidence() {
        let fragment = index.evidence_fragment(candidate).unwrap();
        lines.push(format!(
            "evidence {:?} {:?} gap {}",
            candidate.get().disposition,
            spans(fragment.get().source()),
            fragment.get().source().has_excluded_gap()
        ));
    }
    for owner in facts.owners() {
        lines.push(format!("owner {}", owner_text(index, owner)));
    }
    lines
}

/// The range of a fragment and its comment node. A fragment here is one piece of
/// comment spelling, and its node is the `line_comment` that starts with it; the node
/// may end later, where it holds the line end.
#[cfg(feature = "rust")]
fn comment_span<'s>(fragment: &Fragment<'s>) -> (Range<usize>, tree_sitter::Node<'s>) {
    let [piece] = fragment.source().pieces() else {
        panic!("one piece, found {:?}", fragment.source().pieces())
    };
    let carrier = fragment.node().unwrap().node();
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        (carrier.kind(), carrier.start_byte()),
        ("line_comment", piece.range().start)
    );
    (piece.range(), carrier)
}

/// The runs of a complete text. A copied run here reads one piece, and a query for
/// the whole text returns the same runs.
#[cfg(feature = "rust")]
fn runs(complete: CompleteText<'_, '_>) -> Vec<Run> {
    let map: Vec<Run> = complete
        .mappings()
        .iter()
        .map(|run| match &run.origin {
            Origin::Copied(pieces) => (run.output.range(), Some(spans(pieces)[0].clone())),
            Origin::Synthetic => (run.output.range(), None),
            other => panic!("a copied or an inserted run, found {other:?}"),
        })
        .collect();
    let queried: Vec<Run> = complete
        .origins(0..complete.text().len())
        .unwrap()
        .map(|portion| match portion.origin {
            OriginView::Copied(piece) => (portion.output, Some(piece.range())),
            OriginView::Synthetic => (portion.output, None),
            other => panic!("a copied or an inserted portion, found {other:?}"),
        })
        .collect();
    assert_eq!(queried, map);
    map
}

/// The documents of an index in which every document is a run of `///` comments owned
/// by the function that follows it. What holds for every such document is asserted
/// here and is in no expected value: association `i` belongs to document `i` and is
/// resolved; its one owner is a `function_item` node, which is also its binding; and
/// its lexical scope is the parent of the document's first comment.
#[cfg(feature = "rust")]
fn documented_functions(index: &DocumentationIndex<'_>) -> Vec<Doc> {
    let facts = index.facts();
    assert_eq!(index.associations().len(), index.documents().len());
    let mut documents = Vec::new();
    for (document, association) in index.documents().zip(index.associations()) {
        assert_eq!(facts.associated_document(association), Ok(document));
        assert_eq!(association.get().resolution, Resolution::Resolved);
        assert_eq!(association.get().owner_count(), 1);
        let owner = facts.associated_owner(association, 0).unwrap();
        let Owner::Node(declared) = owner.get() else {
            panic!("a node owner, found {:?}", owner.get())
        };
        let function = declared.node();
        assert_eq!(function.kind(), "function_item");
        assert_eq!(
            association.get().binding.map(NodeEvidence::node),
            Some(function)
        );
        let fragments: Vec<_> = (0..document.get().fragment_count())
            .map(|at| comment_span(facts.document_fragment(document, at).unwrap().get()))
            .collect();
        assert_eq!(
            Some(association.get().lexical_scope.node()),
            fragments[0].1.parent()
        );
        let complete = index.complete_text(document).unwrap().unwrap();
        documents.push(Doc {
            fragments: fragments.into_iter().map(|(range, _)| range).collect(),
            text: complete.text().to_owned(),
            map: runs(complete),
            owner: function.byte_range(),
        });
    }
    documents
}

#[cfg(feature = "rust")]
fn doc<const F: usize, const R: usize>(
    fragments: [Range<usize>; F],
    text: &str,
    map: [Run; R],
    owner: Range<usize>,
) -> Doc {
    Doc {
        fragments: fragments.into(),
        text: text.to_owned(),
        map: map.into(),
        owner,
    }
}

/// The retained candidates as disposition and included pieces, in the index's order.
#[cfg(feature = "rust")]
fn candidates(index: &DocumentationIndex<'_>) -> Vec<(CandidateDisposition, Vec<Range<usize>>)> {
    index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, spans(fragment.get().source()))
        })
        .collect()
}

/// A file under a new directory, as `<directory>/nested/<name>`.
#[cfg(feature = "rust")]
fn written(directory: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let nested = directory.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let path = nested.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

// `Ast` gained a field and the index is handed out by the root crate: both stay
// `Send + Sync`, as the contract asks of the index.
#[cfg(feature = "rust")]
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Ast>();
    assert_send_sync::<DocumentationIndex<'static>>();
};

#[cfg(feature = "rust")]
#[test]
fn contract_rows_reach_the_index_through_the_root_ast() {
    type Row = (&'static [u8], Vec<Doc>, &'static str);
    let rows: [Row; 4] = [
        // The primary row: the fragment ends before the line end, and the text is
        // the comment without its `///`.
        (
            ROSTER,
            vec![doc([0..8], "audit", [(0..5, Some(3..8))], 9..18)],
            "source_file",
        ),
        // Two comments are one document; the line break between them is inserted.
        (
            b"///a\n///b\nfn f() {}",
            vec![doc(
                [0..4, 5..9],
                "a\nb",
                [(0..1, Some(3..4)), (1..2, None), (2..3, Some(8..9))],
                10..19,
            )],
            "source_file",
        ),
        // A lexical scope that is not the root.
        (
            b"mod m {\n///audit\nfn f() {}\n}",
            vec![doc([8..16], "audit", [(0..5, Some(11..16))], 17..26)],
            "declaration_list",
        ),
        // Two documents, in source order; an attribute stands between the first
        // comment and its function.
        (
            TESTS_AND_CODE,
            vec![
                doc([0..9], "outer", [(0..5, Some(4..9))], 18..32),
                doc([34..42], "kept", [(0..4, Some(38..42))], 43..52),
            ],
            "source_file",
        ),
    ];
    for (source, expected, scope) in rows {
        let ast = parsed(source);
        let index = collected(&ast);
        let shown = String::from_utf8_lossy(source);
        assert_eq!(documented_functions(&index), expected, "{shown}");
        let root = ast.as_tree_sitter().root_node();
        for association in index.associations() {
            let lexical = association.get().lexical_scope.node();
            assert_eq!(lexical.kind(), scope, "{shown}");
            assert_eq!(lexical == root, scope == "source_file", "{shown}");
        }
        // The index borrows the buffer the `Ast` holds, so its offsets are that
        // buffer's.
        assert!(std::ptr::eq(index.source(), ast.source()), "{shown}");
        assert_eq!(ast.source(), source, "{shown}");
    }
}

#[cfg(feature = "rust")]
#[test]
fn an_undocumented_function_is_a_supported_owner_and_its_keyword_is_not() {
    for (source, function, documents) in [(&b"fn f() {}"[..], 0..9, 0), (ROSTER, 9..18, 1)] {
        let ast = parsed(source);
        let index = collected(&ast);
        // The root crate's own node wrapper reaches a query through its tree-sitter node.
        let item = ast
            .root_node()
            .children()
            .find(|child| child.kind() == "function_item")
            .unwrap();
        assert_eq!(item.as_tree_sitter().byte_range(), function);
        let query = index.node_associations(item.as_tree_sitter()).unwrap();
        assert!(!query.projection_pending());
        assert!(
            !index
                .node_projection(item.as_tree_sitter())
                .unwrap()
                .pending()
        );
        assert_eq!(query.associations().len(), documents);
        assert_eq!(index.documents().len(), documents);
        let keyword = item.children().next().unwrap();
        assert_eq!(keyword.kind(), "fn");
        assert_eq!(
            index.node_associations(keyword.as_tree_sitter()).err(),
            Some(OwnerError::UnsupportedOwner)
        );
    }
}

// test-lang-gates: hand-written(rust) — `from_path` picks the grammar from the
//     `.rs` extension of each path, and nothing in the body names it
#[cfg(feature = "rust")]
#[test]
fn a_file_read_from_a_path_supplies_its_final_component() {
    let directory = tempfile::tempdir().unwrap();
    let file = written(directory.path(), "lib.rs", b"///audit\nfn f() {}\n");
    let nested = file.parent().unwrap();
    let spellings = [
        file.clone(),
        nested.join("..").join("nested").join("lib.rs"),
        nested.join(".").join("lib.rs"),
    ];
    for path in &spellings {
        let ast = Ast::from_path(path).unwrap();
        // The display name is the whole path as given; only its final component is
        // the logical filename.
        assert_eq!(ast.name(), path.to_str());
        assert_eq!(rust_logical_filename(&collected(&ast)), Some("lib.rs"));
    }
    assert_ne!(spellings[1].to_str(), spellings[0].to_str());
    // The name is supplied as it is written: upper case, a space, a hyphen, a second
    // dot and a letter outside ASCII all stay.
    let as_written = "Año Package-Info.v2.rs";
    let odd = written(directory.path(), as_written, b"///audit\nfn f() {}\n");
    let odd = Ast::from_path(&odd).unwrap();
    assert_eq!(rust_logical_filename(&collected(&odd)), Some(as_written));
    // A relative path: cargo runs an integration test in the package root.
    let relative = Ast::from_path(Path::new("src/lib.rs")).unwrap();
    assert_eq!(relative.name(), Some("src/lib.rs"));
    assert_eq!(rust_logical_filename(&collected(&relative)), Some("lib.rs"));
    // A path without a final component names a directory, so there is no `Ast` to
    // hold a provenance.
    let above = nested.join("..");
    assert_eq!(above.file_name(), None);
    assert!(matches!(Ast::from_path(&above), Err(FromPathError::Io(_))));
}

// test-lang-gates: hand-written(rust) — `from_path` picks the grammar from the
//     `.rs` extension of the path, and nothing in the body names it
#[cfg(all(unix, feature = "rust"))]
#[test]
fn a_symbolic_link_supplies_its_own_name() {
    let directory = tempfile::tempdir().unwrap();
    let file = written(directory.path(), "lib.rs", b"///audit\nfn f() {}\n");
    let link = file.with_file_name("link.rs");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let ast = Ast::from_path(&link).unwrap();
    // The link is followed for its bytes and never for its name: the path is not resolved.
    assert_eq!(ast.name(), link.to_str());
    let index = collected(&ast);
    assert_eq!(index.documents().len(), 1);
    assert_eq!(rust_logical_filename(&index), Some("link.rs"));
}

// test-lang-gates: hand-written(rust) — `from_path` picks the grammar from the
//     `.rs` extension of the path, and nothing in the body names it
#[cfg(feature = "rust")]
#[test]
fn an_ast_read_from_a_path_prints_its_language_and_display_name_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = written(directory.path(), "lib.rs", b"///audit\nfn f() {}\n");
    let read = Ast::from_path(&path).unwrap();
    // This `Ast` holds a provenance.
    assert_eq!(rust_logical_filename(&collected(&read)), Some("lib.rs"));
    // expected: the language and the display name, as before the field existed; `..`
    // stands for every other field.
    assert_eq!(
        format!("{read:?}"),
        format!("Ast {{ language: Rust, name: {:?}, .. }}", path.to_str())
    );
}

#[cfg(feature = "rust")]
#[test]
fn a_display_label_never_becomes_a_logical_filename() {
    let source = b"///audit\nfn f() {}\n";
    let directory = tempfile::tempdir().unwrap();
    let path = written(directory.path(), "lib.rs", source);
    // A bare file name, and the whole path of the file read below: a caller may
    // label a source with its path.
    for label in ["lib.rs", path.to_str().unwrap()] {
        let labelled =
            Ast::parse(Source::new(LANG::Rust, source).with_name(Some(label.to_owned()))).unwrap();
        assert_eq!(labelled.name(), Some(label));
        assert_eq!(rust_logical_filename(&collected(&labelled)), None);
        let taken = adopted(source, &[0..source.len()], Some(label));
        assert_eq!(taken.name(), Some(label));
        assert_eq!(rust_logical_filename(&collected(&taken)), None);
    }
    // The same bytes read from that path: the name is known.
    let read = Ast::from_path(&path).unwrap();
    assert_eq!(read.source(), source);
    assert_eq!(rust_logical_filename(&collected(&read)), Some("lib.rs"));
}

#[cfg(feature = "rust")]
#[test]
fn an_explicit_logical_filename_always_wins() {
    let source = b"///audit\nfn f() {}\n";
    let directory = tempfile::tempdir().unwrap();
    let read = Ast::from_path(&written(directory.path(), "lib.rs", source)).unwrap();
    let options = DocumentationOptions::default();
    // The provenance is there to be overridden.
    assert_eq!(rust_logical_filename(&collected(&read)), Some("lib.rs"));
    for ast in [
        read,
        parsed(source),
        adopted(source, &[0..source.len()], Some("lib.rs")),
    ] {
        // A name with a directory is passed on whole, and an empty name is a name too.
        for explicit in ["other.rs", "nested/other.rs", ""] {
            let index = ast.documentation(&named(explicit), &options).unwrap();
            assert_eq!(rust_logical_filename(&index), Some(explicit));
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn offsets_of_a_file_refer_to_the_normalized_buffer() {
    let bytes = b"///a\r\n///b\r\nfn f() {}\r\n";
    let directory = tempfile::tempdir().unwrap();
    let read = Ast::from_path(&written(directory.path(), "crlf.rs", bytes)).unwrap();
    // The reader turns each CRLF into LF before the parse.
    assert_eq!(read.source(), b"///a\n///b\nfn f() {}\n");
    let index = collected(&read);
    assert!(std::ptr::eq(index.source(), read.source()));
    assert_eq!(
        documented_functions(&index),
        [doc(
            [0..4, 5..9],
            "a\nb",
            [(0..1, Some(3..4)), (1..2, None), (2..3, Some(8..9))],
            10..19,
        )]
    );
    // The same bytes parsed as they are: every offset after the first line differs.
    let verbatim = Ast::parse(Source::new(LANG::Rust, bytes)).unwrap();
    assert_eq!(verbatim.source(), bytes);
    assert_eq!(
        documented_functions(&collected(&verbatim)),
        [doc(
            [0..4, 6..10],
            "a\nb",
            [(0..1, Some(3..4)), (1..2, None), (2..3, Some(9..10))],
            12..21,
        )]
    );
}

#[cfg(feature = "rust")]
#[test]
fn a_cancelled_parse_is_reported_after_its_scope_and_the_next_parse_collects() {
    let source = "/// doc\nfn f() {}\n".repeat(200);
    let cancel = ParseCancel::new();
    cancel.cancel();
    let stopped = cancel.run(|| Ast::parse(Source::new(LANG::Rust, source.as_bytes())).unwrap());
    // The parse did stop: the tree is empty and the buffer is whole.
    assert_eq!(stopped.as_tree_sitter().root_node().byte_range(), 0..0);
    assert_eq!(stopped.source(), source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    assert_eq!(
        stopped.documentation(&context, &options).err(),
        Some(CollectionError::Cancelled)
    );
    // The next parse of the same bytes on this thread is complete.
    let complete = parsed(source.as_bytes());
    let index = collected(&complete);
    // expected: one comment before each of the 200 functions.
    assert_eq!(index.documents().len(), 200);
    assert_eq!(index.associations().len(), 200);
}

#[cfg(feature = "rust")]
#[test]
fn an_empty_source_gives_an_empty_index() {
    let cancel = ParseCancel::new();
    cancel.cancel();
    // An empty parse finishes before the parser first reads the flag, so a flag at
    // parse time alone is no cancelled parse.
    let under_flag = cancel.run(|| Ast::parse(Source::new(LANG::Rust, b"")).unwrap());
    for empty in [parsed(b""), under_flag] {
        assert_eq!(empty.as_tree_sitter().root_node().byte_range(), 0..0);
        let index = empty
            .documentation(&DocumentationContext::default(), &unbound())
            .unwrap();
        assert_eq!(index.documents().len(), 0);
        assert_eq!(index.associations().len(), 0);
        assert_eq!(index.assessments().len(), 0);
        assert_eq!(index.evidence().len(), 0);
    }
    // The same call on the primary row publishes its document.
    assert_eq!(collected(&parsed(ROSTER)).documents().len(), 1);
}

#[cfg(feature = "rust")]
#[test]
fn a_collection_under_a_cancelled_flag_is_cancelled_and_the_parse_stays_complete() {
    let ast = parsed(ROSTER);
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let documents = || {
        ast.documentation(&context, &options)
            .map(|index| index.documents().len())
    };
    let cancel = ParseCancel::new();
    // A flag that is installed and not cancelled stops nothing.
    assert_eq!(cancel.run(documents), Ok(1));
    cancel.cancel();
    assert_eq!(cancel.run(documents), Err(CollectionError::Cancelled));
    // Outside the scope the same `Ast` collects again.
    assert_eq!(documents(), Ok(1));
}

// test-lang-gates: hand-written(rust) — `from_path` picks the grammar from the
//     `.rs` extension of the path, and nothing in the body names it
#[cfg(feature = "rust")]
#[test]
fn a_file_read_from_a_path_is_cancelled_as_a_parsed_source_is() {
    let source = "/// doc\nfn f() {}\n".repeat(200);
    let directory = tempfile::tempdir().unwrap();
    let path = written(directory.path(), "lib.rs", source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let read = Ast::from_path(&path).unwrap();
    // Every collection below is given the name the path supplies.
    assert_eq!(rust_logical_filename(&collected(&read)), Some("lib.rs"));
    let documents = || {
        read.documentation(&context, &options)
            .map(|index| index.documents().len())
    };
    let cancel = ParseCancel::new();
    // expected: one comment before each of the 200 functions.
    assert_eq!(cancel.run(documents), Ok(200));
    cancel.cancel();
    assert_eq!(cancel.run(documents), Err(CollectionError::Cancelled));
    assert_eq!(documents(), Ok(200));
    // The same file parsed under the cancelled flag: the parse did stop, and the
    // collection reports it after the scope of the flag.
    let stopped = cancel.run(|| Ast::from_path(&path).unwrap());
    assert_eq!(stopped.as_tree_sitter().root_node().byte_range(), 0..0);
    assert_eq!(stopped.source(), source.as_bytes());
    assert_eq!(stopped.name(), Some(path.to_str().unwrap()));
    assert_eq!(
        stopped.documentation(&context, &options).err(),
        Some(CollectionError::Cancelled)
    );
    let fresh = Ast::from_path(&path).unwrap();
    let index = fresh.documentation(&context, &options).unwrap();
    assert_eq!(index.context().logical_filename.as_deref(), Some("lib.rs"));
    assert_eq!(index.documents().len(), 200);
}

#[cfg(feature = "rust")]
#[test]
fn an_adopted_tree_is_collected_as_given() {
    // One included range that ends after the first function.
    let two = b"///x\nfn f() {}\n///y\nfn g() {}";
    let first = adopted(two, &[0..14], None);
    assert_eq!(
        documented_functions(&collected(&first)),
        [doc([0..4], "x", [(0..1, Some(3..4))], 5..14)]
    );
    // The whole source has two documents, so a parse of the buffer would too.
    assert_eq!(collected(&parsed(two)).documents().len(), 2);

    // The contract's gap: the line end after the comment is excluded. The collector
    // also keeps pending assessments for this comment. They are an unfinished state
    // of the Rust adapter and are not written out, here or below.
    let gap = b"///x\n\nfn f() {}";
    let split = adopted(gap, &[0..4, 6..15], None);
    let context = DocumentationContext::default();
    let index = split.documentation(&context, &unbound()).unwrap();
    assert_eq!(index.documents().len(), 0);
    assert_eq!(index.associations().len(), 0);
    // The comment token runs to the end of the tree, because nothing included ends
    // its line. It is kept as its two included pieces, never as one range over the gap.
    let comment = node_at(split.as_tree_sitter(), "line_comment", 0..15);
    assert_eq!(comment.parent(), Some(split.as_tree_sitter().root_node()));
    assert_eq!(
        candidates(&index),
        [(CandidateDisposition::Unattached, vec![0..4, 6..15])]
    );
    assert_eq!(collected(&split).evidence().len(), 0);
    // Without the gap the comment documents the function.
    assert_eq!(
        documented_functions(&collected(&parsed(gap))),
        [doc([0..4], "x", [(0..1, Some(3..4))], 6..15)]
    );

    // A document that lies wholly in the later range is published as usual.
    let later = adopted(GAP_THEN_DOCUMENTED, &[0..4, 6..40], None);
    assert_eq!(
        documented_functions(&collected(&later)),
        [doc([26..30], "y", [(0..1, Some(29..30))], 31..40)]
    );
}

#[cfg(feature = "rust")]
#[test]
fn two_indexes_over_one_ast_are_independent() {
    let ast = parsed(ROSTER);
    let first = collected(&ast);
    let second = collected(&ast);
    assert_eq!(published(&first), published(&second));
    // A handle belongs to the index it came from.
    let document = first.documents().next().unwrap();
    assert_eq!(
        first.complete_text(document).unwrap().unwrap().text(),
        "audit"
    );
    assert_eq!(
        second.complete_text(document).err(),
        Some(FactError::ForeignIndex)
    );
    // A node of the tree the `Ast` holds may be asked of both; the same node of
    // another parse of the same bytes, or of a clone of the tree, of neither.
    let function = node_at(ast.as_tree_sitter(), "function_item", 9..18);
    let other = parsed(ROSTER);
    let cloned = ast.as_tree_sitter().clone();
    for index in [&first, &second] {
        let query = index.node_associations(function).unwrap();
        assert_eq!(query.associations().len(), 1);
        for tree in [other.as_tree_sitter(), &cloned] {
            assert_eq!(
                index
                    .node_associations(node_at(tree, "function_item", 9..18))
                    .err(),
                Some(OwnerError::Source(SourceError::ForeignTree))
            );
        }
    }
}

#[cfg(feature = "rust")]
#[test]
fn the_other_ast_operations_neither_collect_nor_change_what_is_collected() {
    let directory = tempfile::tempdir().unwrap();
    let path = written(directory.path(), "lib.rs", TESTS_AND_CODE);
    let unbuilt = observations();
    // Read from a path, so that the view below holds a supplied logical filename.
    // `from_path` builds through `Ast::parse`; the third constructor adopts a tree.
    let ast = Ast::from_path(&path).unwrap();
    let whole = adopted(TESTS_AND_CODE, &[0..TESTS_AND_CODE.len()], None);
    let idle = observations();
    let before = collected(&ast);
    let collecting = observations();
    // The collection moves these counters on this thread, so a zero below is a reading.
    // expected: the walk enters every node once.
    assert_eq!(
        collecting.visits - idle.visits,
        ast.root_node().preorder().count()
    );
    // expected: one render per document.
    assert_eq!(collecting.renders - idle.renders, 2);
    // expected: the file and the two functions.
    assert_eq!(collecting.owner_projections - idle.owner_projections, 3);
    // The checkpoint total has no closed form.
    assert_ne!(collecting.polls - idle.polls, 0);
    // Building an `Ast` collects nothing, whichever constructor builds it.
    assert_eq!(whole.source(), ast.source());
    assert_eq!(idle.visits - unbuilt.visits, 0);
    assert_eq!(idle.renders - unbuilt.renders, 0);
    assert_eq!(idle.owner_projections - unbuilt.owner_projections, 0);
    assert_eq!(idle.polls - unbuilt.polls, 0);
    assert_eq!(rust_logical_filename(&before), Some("lib.rs"));
    let view = published(&before);

    let metrics = |options: MetricsOptions| ast.metrics(options).unwrap().metrics.loc.dloc();
    // The metric options do take effect: the documentation rows of the test function
    // are counted, or pruned with it.
    assert_eq!(metrics(MetricsOptions::default()), 2);
    assert_eq!(
        metrics(MetricsOptions::default().with_exclude_tests(true)),
        1
    );
    assert_eq!(
        metrics(MetricsOptions::default().with_mi_charges_docs(true)),
        2
    );
    assert_eq!(
        metrics(MetricsOptions::default().with_only(&[Metric::Loc])),
        2
    );
    assert_eq!(
        metrics(MetricsOptions::default().with_only(&[Metric::Cyclomatic])),
        0
    );
    assert_eq!(ast.ops().unwrap().spaces.len(), 2);
    assert_eq!(ast.functions().len(), 2);
    let functions = ["function_item".to_owned()];
    assert_eq!(ast.find(&functions).unwrap().len(), 2);
    assert_eq!(ast.count(&functions).0, 2);
    // A stripped copy is a buffer of its own.
    assert_ne!(ast.strip_comments().as_deref(), Some(ast.source()));
    assert_eq!(ast.suppressions().len(), 0);
    for comment in [false, true] {
        for span in [false, true] {
            let configuration = || AstCfg {
                id: String::new(),
                language: String::new(),
                comment,
                span,
            };
            assert!(ast.dump(configuration()).root.is_some());
            assert!(ast.dump_cursor(configuration()).root.is_some());
            assert!(ast.dump_indexed(configuration()).root.is_some());
        }
    }

    let after = observations();
    assert_eq!(after.visits - collecting.visits, 0);
    assert_eq!(after.renders - collecting.renders, 0);
    assert_eq!(after.owner_projections - collecting.owner_projections, 0);
    assert_eq!(after.polls - collecting.polls, 0);
    // The buffer is the one that was parsed. The index taken before is still whole,
    // and a new one says the same, the supplied name included.
    assert_eq!(ast.source(), TESTS_AND_CODE);
    assert_eq!(published(&before), view);
    assert_eq!(published(&collected(&ast)), view);
}

#[cfg(feature = "rust")]
#[test]
fn the_facade_publishes_what_the_collector_publishes_on_the_same_tree_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let read = Ast::from_path(&written(directory.path(), "lib.rs", TESTS_AND_CODE)).unwrap();
    let nothing = DocumentationContext::default();
    let defaults = DocumentationOptions::default();
    let both = unbound().with_directive_evidence(true);
    // Each row: the `Ast`, the caller's context, the context the collector must
    // receive, the options, and how many documents and retained candidates there are.
    let rows = [
        (parsed(ROSTER), &nothing, nothing.clone(), defaults, 1, 0),
        (parsed(DANGLING), &nothing, nothing.clone(), unbound(), 0, 2),
        (
            parsed(TESTS_AND_CODE),
            &named("other.rs"),
            named("other.rs"),
            both,
            2,
            0,
        ),
        (
            adopted(GAP_THEN_DOCUMENTED, &[0..4, 6..40], Some("lib.rs")),
            &nothing,
            nothing.clone(),
            unbound(),
            1,
            1,
        ),
        // The provenance of a path is the same as its final component named outright.
        (read, &nothing, named("lib.rs"), defaults, 2, 0),
    ];
    for (ast, context, received, options, documents, retained) in rows {
        let index = ast.documentation(context, &options).unwrap();
        let direct = collect(
            LANG::Rust,
            ast.as_tree_sitter(),
            ast.source(),
            &received,
            &options,
        )
        .unwrap();
        let view = published(&index);
        assert_eq!(view, published(&direct));
        assert_eq!(
            (index.documents().len(), index.evidence().len()),
            (documents, retained),
            "{view:?}"
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn collection_options_reach_the_collector() {
    let ast = parsed(DANGLING);
    let context = DocumentationContext::default();
    let directives = DocumentationOptions::default().with_directive_evidence(true);
    // Neither comment has an item to document, and neither is a directive.
    for options in [DocumentationOptions::default(), directives] {
        let index = ast.documentation(&context, &options).unwrap();
        assert_eq!(index.documents().len(), 0);
        assert_eq!(candidates(&index), []);
    }
    let index = ast.documentation(&context, &unbound()).unwrap();
    assert!(index.options().retain_unbound);
    assert_eq!(index.documents().len(), 0);
    // Both are kept on request, in source order.
    assert_eq!(
        candidates(&index),
        [
            (CandidateDisposition::Unattached, vec![9..13]),
            (CandidateDisposition::Unattached, vec![16..20]),
        ]
    );
}

#[cfg(feature = "rust")]
#[test]
fn collection_errors_come_back_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let read = Ast::from_path(&written(directory.path(), "lib.rs", ROSTER)).unwrap();
    let context = DocumentationContext::default();
    let profiled = |profile| DocumentationOptions::default().with_profile(profile);
    for ast in [parsed(ROSTER), read] {
        // A profile of another language.
        assert!(matches!(
            ast.documentation(&context, &profiled(Profile::Javadoc)),
            Err(CollectionError::UnsupportedProfile {
                language: LANG::Rust,
                profile: Profile::Javadoc,
                ..
            })
        ));
        // The language's own profile, named or by default, resolves to itself.
        for profile in [Profile::Rust, Profile::Default] {
            let index = ast.documentation(&context, &profiled(profile)).unwrap();
            assert_eq!(index.profile(), Profile::Rust);
        }
    }
    // A tree adopted with a buffer shorter than itself.
    let tree = parsed(ROSTER).as_tree_sitter().clone();
    assert_eq!(tree.root_node().end_byte(), ROSTER.len());
    let short = Ast::from_tree_sitter(LANG::Rust, tree, ROSTER[..8].to_vec(), None).unwrap();
    assert_eq!(
        short
            .documentation(&context, &DocumentationOptions::default())
            .err(),
        Some(CollectionError::SourceBounds)
    );
}

#[cfg(feature = "rust")]
#[test]
fn a_long_logical_filename_is_charged_to_the_collection() {
    let ast = parsed(b"fn f() {}");
    let checkpoints = |context: &DocumentationContext| {
        let before = observations().polls;
        let index = ast
            .documentation(context, &DocumentationOptions::default())
            .unwrap();
        let after = observations().polls;
        (
            after - before,
            index.context().logical_filename.as_ref().map(String::len),
        )
    };
    let (unnamed, absent) = checkpoints(&DocumentationContext::default());
    assert_eq!(absent, None);
    // A collection has checkpoints of its own; their number has no closed form.
    assert_ne!(unnamed, 0);
    // 64 KiB of name, in one-byte and in two-byte characters.
    for name in ["a".repeat(65_536), "é".repeat(32_768)] {
        let (charged, length) = checkpoints(&named(&name));
        assert_eq!(length, Some(65_536));
        // expected: the copy of the name is charged by its bytes, and a checkpoint
        // falls on every 16 KiB charged.
        assert_eq!(charged - unnamed, 4);
    }
}

#[cfg(feature = "rust")]
#[test]
fn public_documentation_builders_and_error_traits_are_usable() {
    fn error<T: std::error::Error>() {}
    error::<CollectionError>();
    error::<OwnerError>();
    error::<FactError>();
    error::<SourceError>();
    error::<big_code_analysis::documentation::TextRangeError>();

    let ast = parsed(ROSTER);
    let context = DocumentationContext::default().with_logical_filename("explicit.rs");
    let options = DocumentationOptions::default()
        .with_profile(Profile::Rust)
        .with_unbound_evidence(true)
        .with_directive_evidence(true);
    let index = ast.documentation(&context, &options).unwrap();
    assert_eq!(
        index.context().logical_filename.as_deref(),
        Some("explicit.rs")
    );
    assert_eq!(index.options().profile, Profile::Rust);
    assert!(index.options().retain_unbound);
    assert!(index.options().retain_directives);
    assert_eq!(index.documents().len(), 1);
    let _: big_code_analysis::documentation::Convention =
        index.associations().next().unwrap().get().convention;
    let text = index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap();
    let range: big_code_analysis::documentation::TextRange =
        big_code_analysis::documentation::TextRange::new(text.text(), 0..1).unwrap();
    assert_eq!(range.range(), 0..1);
    let portions: big_code_analysis::documentation::MappingPortions<'_, '_> =
        text.origins(range.range()).unwrap();
    assert_eq!(portions.count(), 1);
    let node = ast.as_tree_sitter().root_node().named_child(1).unwrap();
    let projection = index.node_projection(node).unwrap();
    let debug_projection = format!("{projection:?}");
    assert!(debug_projection.contains("pending: false"));
    assert!(debug_projection.contains("owners: 1"));
    assert!(!debug_projection.contains("DocumentationIndex"));
    let owner = index.node_owners(node).unwrap().next().unwrap();
    let associations = index.owner_associations(owner).unwrap();
    let debug_associations = format!("{associations:?}");
    assert!(debug_associations.contains("projection_pending: false"));
    assert!(debug_associations.contains("associations: 1"));
    assert!(!debug_associations.contains("DocumentationIndex"));
    assert_eq!(
        CollectionError::Source(SourceError::ForeignTree).to_string(),
        "documentation source: documentation node belongs to another tree"
    );
    assert_eq!(
        std::error::Error::source(&CollectionError::Source(SourceError::ForeignTree))
            .map(ToString::to_string)
            .as_deref(),
        Some("documentation node belongs to another tree")
    );
}
