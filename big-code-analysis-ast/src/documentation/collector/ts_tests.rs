//! TypeScript and TSX source-view tests compare documents, associations, retained candidates
//! and pending assessments through both entry points. Grammar tests collect nothing and pin
//! parse-tree shapes. Node queries are not part of the view.

#[cfg(feature = "typescript")]
use crate::documentation::*;
#[cfg(feature = "typescript")]
use crate::{AnyParser, LANG};
#[cfg(feature = "typescript")]
use std::ops::Range;

#[cfg(feature = "typescript")]
use Run::{Copied, Decoded};

#[cfg(feature = "typescript")]
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Copied(Range<usize>),
    Decoded(Range<usize>),
    Expanded(Range<usize>),
    Synthetic,
}

#[cfg(feature = "typescript")]
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    role: DocumentRole,
    composition: Composition,
    fragments: Vec<Range<usize>>,
    text: String,
    map: Vec<(Range<usize>, Run)>,
}

#[cfg(feature = "typescript")]
type Site = (&'static str, Range<usize>);

#[cfg(feature = "typescript")]
type MemberCase = (&'static [u8], Range<usize>, Range<usize>, Site);

#[cfg(feature = "typescript")]
#[derive(Debug, PartialEq, Eq)]
enum Holder {
    Node(Site),
    File(Range<usize>),
}

#[cfg(feature = "typescript")]
#[derive(Debug, PartialEq, Eq)]
struct Link {
    document: usize,
    owner: Holder,
    binding: Option<Site>,
}

#[cfg(feature = "typescript")]
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    axis: AssessmentAxis,
    reason: &'static str,
    node: Option<Site>,
    document: Option<usize>,
}

/// Documents, associations, retained candidates and unordered pending assessments from a
/// collection. Shared values are asserted in `published`; node queries are not included.
#[cfg(feature = "typescript")]
#[derive(Debug, Default, PartialEq, Eq)]
struct View {
    documents: Vec<Doc>,
    associations: Vec<Link>,
    evidence: Vec<(CandidateDisposition, Range<usize>)>,
    assessments: Vec<Pending>,
}

#[cfg(feature = "typescript")]
const AWAITS_OWNER: &str = "Selected local or wrapper form awaits owner projection";
#[cfg(feature = "typescript")]
const NEEDS_PROJECTION: &str = "Recognized declaration requires a semantic projection";

#[cfg(feature = "typescript")]
fn comment<const N: usize>(range: Range<usize>, text: &str, map: [(Range<usize>, Run); N]) -> Doc {
    Doc {
        role: DocumentRole::Primary,
        composition: Composition::Single,
        fragments: vec![range],
        text: text.to_owned(),
        map: map.into(),
    }
}

#[cfg(feature = "typescript")]
fn file(document: Doc) -> Doc {
    Doc {
        role: DocumentRole::FilePackage,
        ..document
    }
}

#[cfg(feature = "typescript")]
fn link(document: usize, owner: Site, binding: Site) -> Link {
    Link {
        document,
        owner: Holder::Node(owner),
        binding: Some(binding),
    }
}

#[cfg(feature = "typescript")]
fn owned(document: Doc, owner: Site) -> View {
    View {
        documents: vec![document],
        associations: vec![link(0, owner.clone(), owner)],
        ..View::default()
    }
}

#[cfg(feature = "typescript")]
fn wrapped(document: Doc, owner: Site, binding: Site) -> View {
    View {
        documents: vec![document],
        associations: vec![link(0, owner, binding)],
        ..View::default()
    }
}

#[cfg(feature = "typescript")]
fn file_owned(document: Doc, root: Range<usize>) -> View {
    View {
        documents: vec![file(document)],
        associations: vec![Link {
            document: 0,
            owner: Holder::File(root),
            binding: None,
        }],
        ..View::default()
    }
}

#[cfg(feature = "typescript")]
fn pending(reason: &'static str, node: Site, document: Option<usize>) -> Pending {
    Pending {
        axis: AssessmentAxis::Owner,
        reason,
        node: Some(node),
        document,
    }
}

#[cfg(feature = "typescript")]
fn unordered(entries: impl IntoIterator<Item = Pending>) -> Vec<Pending> {
    let mut entries: Vec<_> = entries.into_iter().collect();
    entries.sort_by_cached_key(|entry| format!("{entry:?}"));
    entries
}

#[cfg(feature = "typescript")]
fn unowned(document: Doc, bound: Site, extra: &[Site]) -> View {
    let mut assessments = vec![pending(AWAITS_OWNER, bound, Some(0))];
    assessments.extend(
        extra
            .iter()
            .cloned()
            .map(|site| pending(NEEDS_PROJECTION, site, None)),
    );
    View {
        documents: vec![document],
        assessments: unordered(assessments),
        ..View::default()
    }
}

#[cfg(feature = "typescript")]
fn span(pieces: &SourcePieces<'_>) -> Range<usize> {
    let [piece] = pieces.pieces() else {
        panic!("one piece, found {:?}", pieces.pieces())
    };
    piece.range()
}

#[cfg(feature = "typescript")]
fn site(node: tree_sitter::Node<'_>) -> Site {
    (node.kind(), node.byte_range())
}

#[cfg(feature = "typescript")]
fn fragment_span(fragment: &Fragment<'_>) -> Range<usize> {
    let range = span(fragment.source());
    assert_eq!(fragment.form(), FragmentForm::Comment);
    assert_eq!(
        fragment.node().map(|node| site(node.node())),
        Some(("comment", range.clone()))
    );
    range
}

#[cfg(feature = "typescript")]
fn run_of(origin: &Origin<'_>) -> Run {
    match origin {
        Origin::Copied(pieces) => Run::Copied(span(pieces)),
        Origin::Decoded(pieces) => Run::Decoded(span(pieces)),
        Origin::Expanded(pieces) => Run::Expanded(span(pieces)),
        Origin::Synthetic => Run::Synthetic,
    }
}

#[cfg(feature = "typescript")]
fn published(index: &DocumentationIndex<'_>, root: tree_sitter::Node<'_>) -> View {
    let facts = index.facts();
    let ordered: Vec<_> = index.documents().collect();
    let position = |document| ordered.iter().position(|known| *known == document).unwrap();
    let documents = ordered
        .iter()
        .map(|&document| {
            let complete = index.complete_text(document).unwrap().unwrap();
            Doc {
                role: document.get().role(),
                composition: document.get().composition(),
                fragments: (0..document.get().fragment_count())
                    .map(|at| fragment_span(facts.document_fragment(document, at).unwrap().get()))
                    .collect(),
                text: complete.text().to_owned(),
                map: complete
                    .mappings()
                    .iter()
                    .map(|run| (run.output.range(), run_of(&run.origin)))
                    .collect(),
            }
        })
        .collect();
    let associations = index
        .associations()
        .map(|association| {
            let attachment = association.get();
            assert_eq!(attachment.owner_count(), 1);
            assert_eq!(attachment.convention.name(), "BcaTypescriptSource");
            assert_eq!(attachment.publication, Publication::Unknown);
            assert_eq!(attachment.resolution, Resolution::Resolved);
            let document = facts.associated_document(association).unwrap();
            let comment = facts
                .document_fragment(document, 0)
                .unwrap()
                .get()
                .node()
                .unwrap()
                .node();
            assert_eq!(Some(attachment.lexical_scope.node()), comment.parent());
            let binding = attachment.binding.map(NodeEvidence::node);
            let expected_binding = if document.get().role() == DocumentRole::FilePackage {
                None
            } else {
                let mut sibling = comment.next_sibling();
                while sibling.is_some_and(|node| matches!(node.kind(), "comment" | "decorator")) {
                    sibling = sibling.unwrap().next_sibling();
                }
                sibling
            };
            assert_eq!(binding, expected_binding);
            let owner = facts.associated_owner(association, 0).unwrap();
            let holder = match owner.get() {
                Owner::Node(node) => Holder::Node(site(node.node())),
                Owner::File(file) => {
                    assert_eq!(file.node(), root);
                    Holder::File(file.node().byte_range())
                }
                other => panic!("node or file owner, found {other:?}"),
            };
            Link {
                document: position(document),
                owner: holder,
                binding: binding.map(site),
            }
        })
        .collect();
    let evidence = index
        .evidence()
        .map(|candidate| {
            let fragment = index.evidence_fragment(candidate).unwrap();
            (candidate.get().disposition, fragment_span(fragment.get()))
        })
        .collect();
    let assessments = unordered(index.assessments().map(|assessment| Pending {
        axis: assessment.get().axis,
        reason: assessment.get().reason,
        node: assessment.get().node.map(|node| site(node.node())),
        document: index.assessed_document(assessment).unwrap().map(position),
    }));
    // This source adapter is partial for both variants.
    assert_eq!(index.capability(), Capability::Partial);
    // Named-reference commands alone produce qualifications; none occurs here.
    assert_eq!(index.qualifications().len(), 0);
    View {
        documents,
        associations,
        evidence,
        assessments,
    }
}

#[cfg(feature = "typescript")]
fn clean(language: LANG, source: &[u8]) -> AnyParser {
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
    assert!(
        !parser.ts_tree().root_node().has_error(),
        "{language:?}: {}",
        source.escape_ascii()
    );
    parser
}

#[cfg(feature = "typescript")]
fn view(language: LANG, source: &[u8], retain_unbound: bool) -> View {
    let parser = clean(language, source);
    let options = DocumentationOptions {
        retain_unbound,
        ..DocumentationOptions::default()
    };
    let context = DocumentationContext::default();
    let root = parser.ts_tree().root_node();
    let typed = published(&parser.documentation(&context, &options).unwrap(), root);
    let raw = published(
        &collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
        root,
    );
    assert_eq!(typed, raw, "`AnyParser::documentation` and `collect`");
    typed
}

#[cfg(feature = "typescript")]
#[allow(clippy::needless_pass_by_value)]
fn check(language: LANG, source: &[u8], expected: View) {
    assert_eq!(
        view(language, source, false),
        expected,
        "{language:?}: {}",
        source.escape_ascii()
    );
}

#[cfg(feature = "typescript")]
fn check_retained(
    language: LANG,
    source: &[u8],
    mut expected: View,
    disposition: CandidateDisposition,
    range: Range<usize>,
) {
    assert_eq!(
        view(language, source, false),
        expected,
        "{language:?}: {}",
        source.escape_ascii()
    );
    expected.evidence.push((disposition, range));
    assert_eq!(
        view(language, source, true),
        expected,
        "{language:?}: {}",
        source.escape_ascii()
    );
}

#[cfg(feature = "typescript")]
fn leading_block_documents_the_declaration_after_it(language: LANG) {
    let rows: [(&[u8], Site); 6] = [
        (
            b"/** d */\nfunction f(): void {}\n",
            ("function_declaration", 9..30),
        ),
        (b"/** d */\nclass C {}\n", ("class_declaration", 9..19)),
        (
            b"/** d */\nabstract class A {}\n",
            ("abstract_class_declaration", 9..28),
        ),
        (
            b"/** d */\ninterface I {}\n",
            ("interface_declaration", 9..23),
        ),
        (
            b"/** d */\ntype T = number;\n",
            ("type_alias_declaration", 9..25),
        ),
        (b"/** d */\nenum E { A }\n", ("enum_declaration", 9..21)),
    ];
    for (source, owner) in rows {
        check(
            language,
            source,
            owned(comment(0..8, "d", [(0..1, Copied(4..5))]), owner),
        );
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_leading_block_documents_the_declaration_after_it() {
    leading_block_documents_the_declaration_after_it(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn class_or_interface_member_owns_its_leading_block(language: LANG) {
    let rows: [MemberCase; 6] = [
        (
            b"class C {\n  /** d */\n  m(): void {}\n}\n",
            12..20,
            16..17,
            ("method_definition", 23..35),
        ),
        (
            b"abstract class A {\n  /** d */\n  abstract m(): void;\n}\n",
            21..29,
            25..26,
            ("abstract_method_signature", 32..50),
        ),
        (
            b"interface I {\n  /** d */\n  p: number;\n}\n",
            16..24,
            20..21,
            ("property_signature", 27..36),
        ),
        (
            b"interface I {\n  /** d */\n  m(): void;\n}\n",
            16..24,
            20..21,
            ("method_signature", 27..36),
        ),
        (
            b"interface I {\n  /** d */\n  (): void;\n}\n",
            16..24,
            20..21,
            ("call_signature", 27..35),
        ),
        (
            b"interface I {\n  /** d */\n  [k: string]: number;\n}\n",
            16..24,
            20..21,
            ("index_signature", 27..46),
        ),
    ];
    for (source, fragment, text, owner) in rows {
        check(
            language,
            source,
            owned(comment(fragment, "d", [(0..1, Copied(text))]), owner),
        );
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_class_or_interface_member_owns_its_leading_block() {
    class_or_interface_member_owns_its_leading_block(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_class_or_interface_member_owns_its_leading_block() {
    class_or_interface_member_owns_its_leading_block(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn enum_member_owns_its_leading_block(language: LANG) {
    check(
        language,
        b"enum E {\n  /** a */\n  A,\n  /** b */\n  B = 1,\n}\n",
        View {
            documents: vec![
                comment(11..19, "a", [(0..1, Copied(15..16))]),
                comment(27..35, "b", [(0..1, Copied(31..32))]),
            ],
            associations: vec![
                link(
                    0,
                    ("property_identifier", 22..23),
                    ("property_identifier", 22..23),
                ),
                link(1, ("enum_assignment", 38..43), ("enum_assignment", 38..43)),
            ],
            ..View::default()
        },
    );
    check(
        language,
        b"enum E {\n  /** s */\n  \"s\",\n}\n",
        owned(
            comment(11..19, "s", [(0..1, Copied(15..16))]),
            ("string", 22..25),
        ),
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_enum_member_owns_its_leading_block() {
    enum_member_owns_its_leading_block(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_enum_member_owns_its_leading_block() {
    enum_member_owns_its_leading_block(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn each_overload_signature_and_the_implementation_own_their_block(language: LANG) {
    check(
        language,
        b"/**one*/\nfunction f(x: string): string;\n/**two*/\nfunction f(x: number): number;\n",
        View {
            documents: vec![
                comment(0..8, "one", [(0..3, Copied(3..6))]),
                comment(40..48, "two", [(0..3, Copied(43..46))]),
            ],
            associations: vec![
                link(
                    0,
                    ("function_signature", 9..39),
                    ("function_signature", 9..39),
                ),
                link(
                    1,
                    ("function_signature", 49..79),
                    ("function_signature", 49..79),
                ),
            ],
            // Parameters are pending semantic projections today.
            assessments: unordered([
                pending(NEEDS_PROJECTION, ("required_parameter", 20..29), None),
                pending(NEEDS_PROJECTION, ("required_parameter", 60..69), None),
            ]),
            ..View::default()
        },
    );
    check(
        language,
        b"/** sig */\nfunction f(x: string): string;\nfunction f(x: number): number;\n\
/** impl */\nfunction f(x: any): any { return x; }\n",
        View {
            documents: vec![
                comment(0..10, "sig", [(0..3, Copied(4..7))]),
                comment(73..84, "impl", [(0..4, Copied(77..81))]),
            ],
            associations: vec![
                link(
                    0,
                    ("function_signature", 11..41),
                    ("function_signature", 11..41),
                ),
                link(
                    1,
                    ("function_declaration", 85..122),
                    ("function_declaration", 85..122),
                ),
            ],
            assessments: unordered([
                pending(NEEDS_PROJECTION, ("required_parameter", 22..31), None),
                pending(NEEDS_PROJECTION, ("required_parameter", 53..62), None),
                pending(NEEDS_PROJECTION, ("required_parameter", 96..102), None),
            ]),
            ..View::default()
        },
    );
    check(
        language,
        b"class C {\n  /** one */\n  m(x: string): void;\n  /** two */\n  m(x: number): void;\n  \
/** impl */\n  m(x: any): void {}\n}\n",
        View {
            documents: vec![
                comment(12..22, "one", [(0..3, Copied(16..19))]),
                comment(47..57, "two", [(0..3, Copied(51..54))]),
                comment(82..93, "impl", [(0..4, Copied(86..90))]),
            ],
            associations: vec![
                link(
                    0,
                    ("method_signature", 25..43),
                    ("method_signature", 25..43),
                ),
                link(
                    1,
                    ("method_signature", 60..78),
                    ("method_signature", 60..78),
                ),
                link(
                    2,
                    ("method_definition", 96..114),
                    ("method_definition", 96..114),
                ),
            ],
            assessments: unordered([
                pending(NEEDS_PROJECTION, ("required_parameter", 27..36), None),
                pending(NEEDS_PROJECTION, ("required_parameter", 62..71), None),
                pending(NEEDS_PROJECTION, ("required_parameter", 98..104), None),
            ]),
            ..View::default()
        },
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_each_overload_signature_and_the_implementation_own_their_block() {
    each_overload_signature_and_the_implementation_own_their_block(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_each_overload_signature_and_the_implementation_own_their_block() {
    each_overload_signature_and_the_implementation_own_their_block(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn block_before_export_or_declare_documents_the_wrapped_declaration(language: LANG) {
    let rows: [(&[u8], Site, Site); 6] = [
        (
            b"/** d */\nexport function f(): void {}\n",
            ("function_declaration", 16..37),
            ("export_statement", 9..37),
        ),
        (
            b"/** d */\nexport class C {}\n",
            ("class_declaration", 16..26),
            ("export_statement", 9..26),
        ),
        (
            b"/** d */\nexport abstract class A {}\n",
            ("abstract_class_declaration", 16..35),
            ("export_statement", 9..35),
        ),
        (
            b"/** d */\nexport interface I {}\n",
            ("interface_declaration", 16..30),
            ("export_statement", 9..30),
        ),
        (
            b"/** d */\nexport type T = number;\n",
            ("type_alias_declaration", 16..32),
            ("export_statement", 9..32),
        ),
        (
            b"/** d */\nexport enum E { A }\n",
            ("enum_declaration", 16..28),
            ("export_statement", 9..28),
        ),
    ];
    for (source, owner, binding) in rows {
        check(
            language,
            source,
            wrapped(comment(0..8, "d", [(0..1, Copied(4..5))]), owner, binding),
        );
    }
    check(
        language,
        b"/**audit*/\ndeclare function f(): void;\n",
        wrapped(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_signature", 19..38),
            ("ambient_declaration", 11..38),
        ),
    );
    let source = b"/** above */\nexport /** inside */ function f(): void {}\n";
    check_retained(
        language,
        source,
        wrapped(
            comment(0..12, "above", [(0..5, Copied(4..9))]),
            ("function_declaration", 34..55),
            ("export_statement", 13..55),
        ),
        CandidateDisposition::Unattached,
        20..33,
    );
    let source = b"/** above */\nexport default\n/** inside */\nfunction f(): void {}\n";
    check_retained(
        language,
        source,
        wrapped(
            comment(0..12, "above", [(0..5, Copied(4..9))]),
            ("function_declaration", 42..63),
            ("export_statement", 13..63),
        ),
        CandidateDisposition::Unattached,
        28..41,
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_block_before_export_or_declare_documents_the_wrapped_declaration() {
    block_before_export_or_declare_documents_the_wrapped_declaration(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_block_before_export_or_declare_documents_the_wrapped_declaration() {
    block_before_export_or_declare_documents_the_wrapped_declaration(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn block_before_a_decorator_documents_the_decorated_class_or_member(language: LANG) {
    let rows: [MemberCase; 3] = [
        (
            b"/** d */\n@dec\nclass C {}\n",
            0..8,
            4..5,
            ("class_declaration", 9..24),
        ),
        (
            b"class C {\n  /** d */\n  @dec\n  m(): void {}\n}\n",
            12..20,
            16..17,
            ("method_definition", 30..42),
        ),
        (
            b"class C {\n  /** d */\n  @dec\n  x = 1;\n}\n",
            12..20,
            16..17,
            ("public_field_definition", 23..35),
        ),
    ];
    for (source, fragment, text, owner) in rows {
        check(
            language,
            source,
            owned(comment(fragment, "d", [(0..1, Copied(text))]), owner),
        );
    }
    check(
        language,
        b"/** d */\n@dec\nexport class C {}\n",
        wrapped(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("class_declaration", 21..31),
            ("export_statement", 9..31),
        ),
    );
    let source = b"class C {\n/**a*/\n@dec\n/**b*/\nm() {}\n}\n";
    check_retained(
        language,
        source,
        owned(
            comment(10..16, "a", [(0..1, Copied(13..14))]),
            ("method_definition", 29..35),
        ),
        CandidateDisposition::Unattached,
        22..28,
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_block_before_a_decorator_documents_the_decorated_class_or_member() {
    block_before_a_decorator_documents_the_decorated_class_or_member(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_block_before_a_decorator_documents_the_decorated_class_or_member() {
    block_before_a_decorator_documents_the_decorated_class_or_member(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn nearest_block_before_the_declaration_is_selected(language: LANG) {
    let source = b"/** old */\n/** new */\nfunction f(): void {}\n";
    check_retained(
        language,
        source,
        owned(
            comment(11..21, "new", [(0..3, Copied(15..18))]),
            ("function_declaration", 22..43),
        ),
        CandidateDisposition::Superseded,
        0..10,
    );
    // The plain `//` comment is not a candidate, even with retention enabled.
    let source = b"/** d */\n// plain\nfunction f(): void {}\n";
    let expected = owned(
        comment(0..8, "d", [(0..1, Copied(4..5))]),
        ("function_declaration", 18..39),
    );
    assert_eq!(view(language, source, false), expected);
    assert_eq!(view(language, source, true), expected);
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_nearest_block_before_the_declaration_is_selected() {
    nearest_block_before_the_declaration_is_selected(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_nearest_block_before_the_declaration_is_selected() {
    nearest_block_before_the_declaration_is_selected(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn module_tag_makes_the_block_a_file_document(language: LANG) {
    let mut with_const = file_owned(
        comment(0..28, "@packageDocumentation", [(0..21, Copied(4..25))]),
        0..49,
    );
    // The const and its export wrapper await projections today, not the file document.
    with_const.assessments = unordered([
        pending(NEEDS_PROJECTION, ("export_statement", 29..48), None),
        pending(NEEDS_PROJECTION, ("lexical_declaration", 36..48), None),
    ]);
    check(
        language,
        b"/** @packageDocumentation */\nexport const x = 1;\n",
        with_const,
    );
    check(
        language,
        b"/** @module */\nfunction f(): void {}\n",
        file_owned(comment(0..14, "@module", [(0..7, Copied(4..11))]), 0..37),
    );
    check(
        language,
        b"/**\n * @module m\n */\nfunction f(): void {}\n",
        file_owned(comment(0..20, "@module m", [(0..9, Copied(7..16))]), 0..43),
    );
    check(
        language,
        b"/** @packageDocumentation */\n/** d */\nfunction f(): void {}\n",
        View {
            documents: vec![
                file(comment(
                    0..28,
                    "@packageDocumentation",
                    [(0..21, Copied(4..25))],
                )),
                comment(29..37, "d", [(0..1, Copied(33..34))]),
            ],
            associations: vec![
                Link {
                    document: 0,
                    owner: Holder::File(0..60),
                    binding: None,
                },
                link(
                    1,
                    ("function_declaration", 38..59),
                    ("function_declaration", 38..59),
                ),
            ],
            ..View::default()
        },
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_module_tag_makes_the_block_a_file_document() {
    module_tag_makes_the_block_a_file_document(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_module_tag_makes_the_block_a_file_document() {
    module_tag_makes_the_block_a_file_document(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn block_text_is_cleaned_and_mapped_to_its_source(language: LANG) {
    check(
        language,
        b"/**audit*/\nfunction f(): void {}\n",
        owned(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_declaration", 11..32),
        ),
    );
    check(
        language,
        b"/**\n * a  \n *   b\n */\nfunction f(): void {}\n",
        owned(
            comment(
                0..21,
                "a  \n  b",
                [
                    (0..3, Copied(7..10)),
                    (3..4, Copied(10..11)),
                    (4..7, Copied(14..17)),
                ],
            ),
            ("function_declaration", 22..43),
        ),
    );
    check(
        language,
        b"/**\r\n * a\r\n * b\r\n */\r\nfunction f(): void {}\r\n",
        owned(
            comment(
                0..20,
                "a\nb",
                [
                    (0..1, Copied(8..9)),
                    (1..2, Decoded(9..11)),
                    (2..3, Copied(14..15)),
                ],
            ),
            ("function_declaration", 22..43),
        ),
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_block_text_is_cleaned_and_mapped_to_its_source() {
    block_text_is_cleaned_and_mapped_to_its_source(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_block_text_is_cleaned_and_mapped_to_its_source() {
    block_text_is_cleaned_and_mapped_to_its_source(LANG::Tsx);
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_function_with_a_jsx_element_owns_its_leading_block() {
    check(
        LANG::Tsx,
        b"/**audit*/\nfunction f() { return <div/>; }\n",
        owned(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_declaration", 11..42),
        ),
    );
    check(
        LANG::Tsx,
        b"/** d */\nexport default function App() { return <div/>; }\n",
        wrapped(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("function_declaration", 24..57),
            ("export_statement", 9..57),
        ),
    );
}

// Today's pending forms intentionally move when anonymous default exports, namespaces,
// const bindings and the child of a declare wrapper gain owner projections.
#[cfg(feature = "typescript")]
fn forms_without_an_owner_keep_their_document_and_no_association(language: LANG) {
    let source = b"/** above */\ndeclare /** inside */ function f(): void;\n";
    check_retained(
        language,
        source,
        unowned(
            comment(0..12, "above", [(0..5, Copied(4..9))]),
            ("ambient_declaration", 13..54),
            &[],
        ),
        CandidateDisposition::Unattached,
        21..34,
    );
    let rows: [(&[u8], Doc, Site, &[Site]); 8] = [
        (
            b"/**audit*/\nexport default function() {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("export_statement", 11..39),
            &[],
        ),
        (
            b"/**audit*/\nexport default class {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("export_statement", 11..34),
            &[],
        ),
        (
            b"/** d */\nnamespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("expression_statement", 9..23),
            &[("expression_statement", 9..23)],
        ),
        (
            b"/** d */\nexport namespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("export_statement", 9..30),
            &[],
        ),
        (
            b"/** d */\ndeclare namespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("ambient_declaration", 9..31),
            &[],
        ),
        (
            b"/**audit*/\nconst x = 1;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("lexical_declaration", 11..23),
            &[("lexical_declaration", 11..23)],
        ),
        (
            b"/** d */\nexport const x = 1;\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("export_statement", 9..28),
            &[("export_statement", 9..28), ("lexical_declaration", 16..28)],
        ),
        (
            b"/** d */\ndeclare const x: number;\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("ambient_declaration", 9..33),
            &[
                ("ambient_declaration", 9..33),
                ("lexical_declaration", 17..33),
            ],
        ),
    ];
    for (source, document, bound, extra) in rows {
        check(language, source, unowned(document, bound, extra));
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_forms_without_an_owner_keep_their_document_and_no_association() {
    forms_without_an_owner_keep_their_document_and_no_association(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_forms_without_an_owner_keep_their_document_and_no_association() {
    forms_without_an_owner_keep_their_document_and_no_association(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn node_at<'a>(parser: &'a AnyParser, kind: &str, range: Range<usize>) -> tree_sitter::Node<'a> {
    let found: Vec<_> = parser
        .root_node()
        .preorder()
        .map(|node| node.as_tree_sitter())
        .filter(|node| node.kind() == kind && node.byte_range() == range)
        .collect();
    let [node] = found[..] else {
        panic!("one {kind} at {range:?}, found {found:?}")
    };
    node
}

#[cfg(feature = "typescript")]
fn parts(parent: tree_sitter::Node<'_>) -> Vec<(Option<&'static str>, Site)> {
    let mut cursor = parent.walk();
    let mut found = Vec::new();
    if cursor.goto_first_child() {
        loop {
            found.push((cursor.field_name(), site(cursor.node())));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    found
}

#[cfg(feature = "typescript")]
fn field(parent: tree_sitter::Node<'_>, name: &str, expected: Site) {
    assert_eq!(parent.child_by_field_name(name).map(site), Some(expected));
}

#[cfg(feature = "typescript")]
fn unnamed(parser: &AnyParser, kind: &str, range: Range<usize>) {
    assert!(!node_at(parser, kind, range).is_named());
}

#[cfg(feature = "typescript")]
#[allow(clippy::too_many_lines)]
fn grammar_shapes_the_unit_relies_on(language: LANG) {
    let parser = clean(
        language,
        b"export function f(): void {}\nexport default function g(): void {}\n",
    );
    for (export, declaration) in [(0..28, 7..28), (29..65, 44..65)] {
        field(
            node_at(&parser, "export_statement", export),
            "declaration",
            ("function_declaration", declaration),
        );
    }
    unnamed(&parser, "export", 0..6);
    unnamed(&parser, "export", 29..35);
    unnamed(&parser, "default", 36..43);

    let parser = clean(language, b"export /** inside */ function f(): void {}\n");
    let export = node_at(&parser, "export_statement", 0..42);
    assert_eq!(
        parts(export),
        [
            (None, ("export", 0..6)),
            (None, ("comment", 7..20)),
            (Some("declaration"), ("function_declaration", 21..42)),
        ]
    );
    assert!(node_at(&parser, "comment", 7..20).is_extra());
    unnamed(&parser, "export", 0..6);
    field(export, "declaration", ("function_declaration", 21..42));

    let parser = clean(language, b"@dec\nexport class C {}\n");
    let export = node_at(&parser, "export_statement", 0..22);
    assert_eq!(
        parts(export),
        [
            (Some("decorator"), ("decorator", 0..4)),
            (None, ("export", 5..11)),
            (Some("declaration"), ("class_declaration", 12..22)),
        ]
    );
    field(export, "decorator", ("decorator", 0..4));
    field(export, "declaration", ("class_declaration", 12..22));
    unnamed(&parser, "export", 5..11);

    let parser = clean(language, b"export default function() {}\n");
    let export = node_at(&parser, "export_statement", 0..28);
    field(export, "value", ("function_expression", 15..28));
    assert!(export.child_by_field_name("declaration").is_none());
    let parser = clean(language, b"export default class {}\n");
    field(
        node_at(&parser, "export_statement", 0..23),
        "value",
        ("class", 15..23),
    );
    assert!(node_at(&parser, "class", 15..23).is_named());

    let parser = clean(language, b"declare /** inside */ function f(): void;\n");
    let ambient = node_at(&parser, "ambient_declaration", 0..41);
    assert_eq!(
        parts(ambient),
        [
            (None, ("declare", 0..7)),
            (None, ("comment", 8..21)),
            (None, ("function_signature", 22..41)),
        ]
    );
    unnamed(&parser, "declare", 0..7);
    assert!(node_at(&parser, "comment", 8..21).is_extra());
    assert_eq!(ambient.named_child(0).map(site), Some(("comment", 8..21)));
    assert_eq!(ambient.child_count(), 3);
    let signature = node_at(&parser, "function_signature", 22..41);
    assert_eq!(
        signature.children(&mut signature.walk()).last().map(site),
        Some((";", 40..41))
    );
    unnamed(&parser, ";", 40..41);

    let parser = clean(
        language,
        b"class C {\n  @dec\n  m(): void {}\n  @dec\n  x = 1;\n  n(x: string): void;\n}\n",
    );
    let body = node_at(&parser, "class_body", 8..71);
    assert_eq!(
        parts(body),
        [
            (None, ("{", 8..9)),
            (Some("decorator"), ("decorator", 12..16)),
            (None, ("method_definition", 19..31)),
            (None, ("public_field_definition", 34..46)),
            (None, (";", 46..47)),
            (None, ("method_signature", 50..68)),
            (None, (";", 68..69)),
            (None, ("}", 70..71)),
        ]
    );
    field(body, "decorator", ("decorator", 12..16));
    field(
        node_at(&parser, "public_field_definition", 34..46),
        "decorator",
        ("decorator", 34..38),
    );
    assert_eq!(
        node_at(&parser, "method_signature", 50..68)
            .child_by_field_name("parameters")
            .unwrap()
            .named_child(0)
            .map(site),
        Some(("required_parameter", 52..61))
    );
    for range in [46..47, 68..69] {
        unnamed(&parser, ";", range);
    }

    let parser = clean(language, b"@dec\nclass C {}\n");
    field(
        node_at(&parser, "class_declaration", 0..15),
        "decorator",
        ("decorator", 0..4),
    );

    let parser = clean(language, b"abstract class A {\n  abstract m(): void;\n}\n");
    assert_eq!(
        parser.ts_tree().root_node().child(0).map(site),
        Some(("abstract_class_declaration", 0..42))
    );
    let body = node_at(&parser, "class_body", 17..42);
    assert_eq!(
        parts(body)[1..3],
        [
            (None, ("abstract_method_signature", 21..39)),
            (None, (";", 39..40)),
        ]
    );
    assert_eq!(
        node_at(&parser, "abstract_method_signature", 21..39)
            .child(0)
            .map(site),
        Some(("abstract", 21..29))
    );
    unnamed(&parser, "abstract", 21..29);
    unnamed(&parser, ";", 39..40);

    let parser = clean(
        language,
        b"interface I {\n  p: number;\n  m(): void;\n  (): void;\n  \
[k: string]: number;\n}\n",
    );
    let body = node_at(&parser, "interface_body", 12..76);
    assert_eq!(
        parts(body),
        [
            (None, ("{", 12..13)),
            (None, ("property_signature", 16..25)),
            (None, (";", 25..26)),
            (None, ("method_signature", 29..38)),
            (None, (";", 38..39)),
            (None, ("call_signature", 42..50)),
            (None, (";", 50..51)),
            (None, ("index_signature", 54..73)),
            (None, (";", 73..74)),
            (None, ("}", 75..76)),
        ]
    );
    for range in [25..26, 38..39, 50..51, 73..74] {
        unnamed(&parser, ";", range);
    }

    let parser = clean(language, b"enum E {\n  A,\n  B = 1,\n  \"s\",\n}\n");
    let body = node_at(&parser, "enum_body", 7..31);
    assert_eq!(
        parts(body),
        [
            (None, ("{", 7..8)),
            (Some("name"), ("property_identifier", 11..12)),
            (None, (",", 12..13)),
            (None, ("enum_assignment", 16..21)),
            (None, (",", 21..22)),
            (Some("name"), ("string", 25..28)),
            (None, (",", 28..29)),
            (None, ("}", 30..31)),
        ]
    );
    field(body, "name", ("property_identifier", 11..12));
    for range in [12..13, 21..22, 28..29] {
        unnamed(&parser, ",", range);
    }

    let parser = clean(
        language,
        b"function f(x: string): string;\nfunction f(x: any): any { return x; }\n",
    );
    let signature = node_at(&parser, "function_signature", 0..30);
    let function = node_at(&parser, "function_declaration", 31..68);
    assert_eq!(
        signature
            .child_by_field_name("parameters")
            .unwrap()
            .named_child(0)
            .map(site),
        Some(("required_parameter", 11..20))
    );
    assert_eq!(
        signature.children(&mut signature.walk()).last().map(site),
        Some((";", 29..30))
    );
    unnamed(&parser, ";", 29..30);
    assert_eq!(
        function
            .child_by_field_name("parameters")
            .unwrap()
            .named_child(0)
            .map(site),
        Some(("required_parameter", 42..48))
    );

    let parser = clean(
        language,
        b"namespace N {}\nexport namespace M {}\ndeclare namespace O {}\n",
    );
    let root = parser.ts_tree().root_node();
    assert_eq!(
        parts(root),
        [
            (None, ("expression_statement", 0..14)),
            (None, ("export_statement", 15..36)),
            (None, ("ambient_declaration", 37..59)),
        ]
    );
    assert_eq!(
        parts(node_at(&parser, "expression_statement", 0..14)),
        [(None, ("internal_module", 0..14))]
    );
    field(
        node_at(&parser, "export_statement", 15..36),
        "declaration",
        ("internal_module", 22..36),
    );
    assert_eq!(
        node_at(&parser, "ambient_declaration", 37..59)
            .named_child(0)
            .map(site),
        Some(("internal_module", 45..59))
    );

    let parser = clean(
        language,
        b"const x = 1;\nexport const y = 1;\ndeclare const z: number;\n",
    );
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, ("lexical_declaration", 0..12)),
            (None, ("export_statement", 13..32)),
            (None, ("ambient_declaration", 33..57)),
        ]
    );
    field(
        node_at(&parser, "export_statement", 13..32),
        "declaration",
        ("lexical_declaration", 20..32),
    );
    assert_eq!(
        node_at(&parser, "ambient_declaration", 33..57)
            .named_child(0)
            .map(site),
        Some(("lexical_declaration", 41..57))
    );
    for range in [6..11, 26..31, 47..56] {
        field(
            node_at(&parser, "variable_declarator", range.clone()),
            "name",
            ("identifier", range.start..range.start + 1),
        );
    }

    let parser = clean(
        language,
        b"type T = number;\ninterface I {}\nenum E { A }\nfunction f(): void {}\n",
    );
    assert_eq!(
        parts(parser.ts_tree().root_node()),
        [
            (None, ("type_alias_declaration", 0..16)),
            (None, ("interface_declaration", 17..31)),
            (None, ("enum_declaration", 32..44)),
            (None, ("function_declaration", 45..66)),
        ]
    );

    let source = b"/**\r\n * a\r\n */\r\nfunction f(): void {}\r\n";
    let parser = clean(language, source);
    let root = parser.ts_tree().root_node();
    assert_eq!(site(root), ("program", 0..39));
    assert_eq!(root.end_byte(), source.len());
    assert_eq!(
        parts(root),
        [
            (None, ("comment", 0..14)),
            (None, ("function_declaration", 16..37)),
        ]
    );
    let comment = node_at(&parser, "comment", 0..14);
    assert_eq!(comment.end_position().row - comment.start_position().row, 2);
    assert_eq!(&source[comment.end_byte()..16], b"\r\n");
}

#[cfg(feature = "typescript")]
fn tsx_element_shape() {
    let source = b"function f() { return <div/>; }\n";
    let parser = clean(LANG::Tsx, source);
    let function = node_at(&parser, "function_declaration", 0..31);
    field(function, "body", ("statement_block", 13..31));
    let statement = node_at(&parser, "return_statement", 15..29);
    assert_eq!(
        statement.parent().map(site),
        Some(("statement_block", 13..31))
    );
    assert_eq!(
        parts(statement)[1],
        (None, ("jsx_self_closing_element", 22..28))
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_grammar_shapes_the_unit_relies_on() {
    grammar_shapes_the_unit_relies_on(LANG::Tsx);
    tsx_element_shape();
}
