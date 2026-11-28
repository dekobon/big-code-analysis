//! TypeScript and TSX source-view tests compare documents, associations, retained candidates
//! and pending assessments through both entry points. Grammar tests collect nothing and pin
//! parse-tree shapes. Node queries are not part of the view.

#[cfg(feature = "typescript")]
use crate::documentation::budget::WorkBudget;
#[cfg(feature = "typescript")]
use crate::documentation::*;
#[cfg(feature = "typescript")]
use crate::{AnyParser, LANG};
#[cfg(feature = "typescript")]
use std::cell::Cell;
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
    // bca: suppress(halstead) -- one reading of the four published lists and the rules they share.
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
    let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
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
fn check(language: LANG, source: &[u8], expected: &View) {
    assert_eq!(
        &view(language, source, false),
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
    check(language, source, &expected);
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
            &owned(comment(0..8, "d", [(0..1, Copied(4..5))]), owner),
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
            &owned(comment(fragment, "d", [(0..1, Copied(text))]), owner),
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
        &View {
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
        &owned(
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
        &View {
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
        &View {
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
        &View {
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
            &wrapped(comment(0..8, "d", [(0..1, Copied(4..5))]), owner, binding),
        );
    }
    check(
        language,
        b"/**audit*/\ndeclare function f(): void;\n",
        &wrapped(
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
            &owned(comment(fragment, "d", [(0..1, Copied(text))]), owner),
        );
    }
    check(
        language,
        b"/** d */\n@dec\nexport class C {}\n",
        &wrapped(
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
    let source = b"/** a */\n/** b */\n/** c */\nfunction f(): void {}\n";
    let mut expected = owned(
        comment(18..26, "c", [(0..1, Copied(22..23))]),
        ("function_declaration", 27..48),
    );
    check(language, source, &expected);
    expected.evidence.extend([
        (CandidateDisposition::Superseded, 0..8),
        (CandidateDisposition::Superseded, 9..17),
    ]);
    assert_eq!(
        view(language, source, true),
        expected,
        "{language:?}: {}",
        source.escape_ascii()
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
    let with_const = file_owned(
        comment(0..28, "@packageDocumentation", [(0..21, Copied(4..25))]),
        0..49,
    );
    // A file document has no binding edge to the now-resolved const declaration.
    check(
        language,
        b"/** @packageDocumentation */\nexport const x = 1;\n",
        &with_const,
    );
    check(
        language,
        b"/** @module */\nfunction f(): void {}\n",
        &file_owned(comment(0..14, "@module", [(0..7, Copied(4..11))]), 0..37),
    );
    check(
        language,
        b"/**\n * @module m\n */\nfunction f(): void {}\n",
        &file_owned(comment(0..20, "@module m", [(0..9, Copied(7..16))]), 0..43),
    );
    check(
        language,
        b"/** @packageDocumentation */\n/** d */\nfunction f(): void {}\n",
        &View {
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
        &owned(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_declaration", 11..32),
        ),
    );
    check(
        language,
        b"/**\n * a  \n *   b\n */\nfunction f(): void {}\n",
        &owned(
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
        &owned(
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
        &owned(
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_declaration", 11..42),
        ),
    );
    check(
        LANG::Tsx,
        b"/** d */\nexport default function App() { return <div/>; }\n",
        &wrapped(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("function_declaration", 24..57),
            ("export_statement", 9..57),
        ),
    );
}

// The error in the lexical declaration is kept pending; unit 2 resolves valid owners.
#[cfg(feature = "typescript")]
fn forms_without_an_owner_keep_their_document_and_no_association(language: LANG) {
    for (source, bound, extra) in [
        (
            b"/**audit*/\nconst x = ;\n".as_slice(),
            ("lexical_declaration", 11..22),
            vec![("lexical_declaration", 11..22)],
        ),
        (
            b"/**audit*/\nvar x = ;\n",
            ("variable_declaration", 11..20),
            vec![("variable_declaration", 11..20)],
        ),
        (
            b"/**audit*/\nnamespace N { %%% }\n",
            ("expression_statement", 11..30),
            vec![
                ("expression_statement", 11..30),
                ("internal_module", 11..30),
            ],
        ),
        (
            b"/**audit*/\nusing h = ;\n",
            ("expression_statement", 11..22),
            vec![
                ("expression_statement", 11..22),
                ("assignment_expression", 11..20),
            ],
        ),
        (
            b"/**audit*/\nusing h: Disposable = open();\n",
            ("expression_statement", 11..40),
            vec![
                ("expression_statement", 11..40),
                ("assignment_expression", 11..39),
            ],
        ),
    ] {
        let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
        assert!(parser.ts_tree().root_node().has_error());
        let options = DocumentationOptions::default();
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
        assert_eq!(typed, raw);
        assert_eq!(
            typed,
            unowned(
                comment(0..10, "audit", [(0..5, Copied(3..8))]),
                bound,
                &extra
            )
        );
    }
    for source in [
        b"/**audit*/\ndeclare function f(: void;\n".as_slice(),
        b"/**audit*/\nexport default function( {\n",
    ] {
        let parser = AnyParser::parse(language, source.to_vec(), None, None).unwrap();
        assert!(parser.ts_tree().root_node().has_error());
        let context = DocumentationContext::default();
        let options = DocumentationOptions {
            retain_unbound: true,
            ..DocumentationOptions::default()
        };
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
        assert_eq!(typed, raw);
        assert_eq!(
            typed,
            View {
                evidence: vec![(CandidateDisposition::Unattached, 0..10)],
                ..View::default()
            }
        );
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
    // bca: suppress(abc, halstead) -- sixteen independent shapes, each a parse and its assertions.
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
    assert_eq!(
        parts(export),
        [
            (None, ("export", 0..6)),
            (None, ("default", 7..14)),
            (Some("value"), ("function_expression", 15..28)),
        ]
    );
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
    let function = node_at(&parser, "function_declaration", 16..37);
    assert_eq!(comment.end_position().row - comment.start_position().row, 2);
    assert_eq!(&source[comment.end_byte()..function.start_byte()], b"\r\n");
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

#[cfg(feature = "typescript")]
fn unit2_ambient_comment_control(language: LANG) {
    check_retained(
        language,
        b"/** above */\ndeclare /** inside */ function f(): void;\n",
        wrapped(
            comment(0..12, "above", [(0..5, Copied(4..9))]),
            ("function_signature", 35..54),
            ("ambient_declaration", 13..54),
        ),
        CandidateDisposition::Unattached,
        21..34,
    );
}

#[cfg(feature = "typescript")]
#[allow(clippy::too_many_lines)]
fn unit2_restricted_keywords(language: LANG) {
    for (source, kind, range, extras) in [
        (
            b"/** d */\nnamespace /*\n*/ N {}\n".as_slice(),
            "expression_statement",
            9..29,
            vec![],
        ),
        (
            b"/** d */\nexport namespace\nN {}\n",
            "export_statement",
            9..30,
            vec![],
        ),
        (
            b"/** d */\ndeclare namespace\nN {}\n",
            "ambient_declaration",
            9..31,
            vec![],
        ),
        (
            b"/** d */\nawait\nusing x = f();\n".as_slice(),
            "expression_statement",
            9..29,
            vec![],
        ),
        (
            b"/** d */\nawait /*\n*/ using x = f();\n",
            "expression_statement",
            9..35,
            vec![],
        ),
        (
            b"/**audit*/\nusing\nx = f();\n".as_slice(),
            "expression_statement",
            11..25,
            vec![],
        ),
        (
            b"/**audit*/\nawait using\nx = f();\n",
            "expression_statement",
            11..31,
            vec![],
        ),
        (
            b"/**audit*/\nusing /*\n*/ x = f();\n",
            "expression_statement",
            11..31,
            vec![],
        ),
        (
            b"/**audit*/\nusing /*\r*/ x = f();\n",
            "expression_statement",
            11..31,
            vec![],
        ),
        (
            b"/**audit*/\nusing /*\xe2\x80\xa8*/ x = f();\n",
            "expression_statement",
            11..33,
            vec![],
        ),
        (
            b"/**audit*/\nusing /*\xe2\x80\xa9*/ x = f();\n",
            "expression_statement",
            11..33,
            vec![],
        ),
        (
            b"/**audit*/\ndeclare /*\n*/ function f(): void;\n",
            "ambient_declaration",
            11..44,
            vec![("ambient_declaration", 11..44)],
        ),
        (
            b"/**audit*/\ndeclare /*\n*/ namespace N {}\n",
            "ambient_declaration",
            11..39,
            vec![("ambient_declaration", 11..39)],
        ),
        (
            b"/**audit*/\ndeclare /*\n*/ const x: number;\n",
            "ambient_declaration",
            11..41,
            vec![("ambient_declaration", 11..41)],
        ),
        (
            b"/**audit*/\nexport default async /*\n*/ function() {}\n",
            "export_statement",
            11..51,
            vec![],
        ),
    ] {
        let fragment = if source.starts_with(b"/** d */") {
            0..8
        } else {
            0..10
        };
        let document = if source.starts_with(b"/** d */") {
            comment(fragment, "d", [(0..1, Copied(4..5))])
        } else {
            comment(fragment, "audit", [(0..5, Copied(3..8))])
        };
        check(language, source, &unowned(document, (kind, range), &extras));
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_restricted_keywords() {
    unit2_restricted_keywords(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_restricted_keywords() {
    unit2_restricted_keywords(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn unit2_declare_global_stays_pending(language: LANG) {
    check(
        language,
        b"/** d */\ndeclare global { }\n",
        &unowned(
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("ambient_declaration", 9..27),
            &[],
        ),
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_declare_global_stays_pending() {
    unit2_declare_global_stays_pending(LANG::Typescript);
}

#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_declare_global_stays_pending() {
    unit2_declare_global_stays_pending(LANG::Tsx);
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_ambient_comment_control() {
    unit2_ambient_comment_control(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_ambient_comment_control() {
    unit2_ambient_comment_control(LANG::Tsx);
}

#[cfg(feature = "typescript")]
#[allow(clippy::too_many_lines)]
fn unit2_owner_forms(language: LANG) {
    let rows: [(&[u8], Doc, Site, Site); 19] = [
        (
            b"/**audit*/\nexport default function() {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("function_expression", 26..39),
            ("export_statement", 11..39),
        ),
        (
            b"/**audit*/\nexport default function* () {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("generator_function", 26..41),
            ("export_statement", 11..41),
        ),
        (
            b"/**audit*/\nexport default class {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("class", 26..34),
            ("export_statement", 11..34),
        ),
        (
            b"/** above */\ndeclare /** inside */ function f(): void;\n",
            comment(0..12, "above", [(0..5, Copied(4..9))]),
            ("function_signature", 35..54),
            ("ambient_declaration", 13..54),
        ),
        (
            b"/** d */\nnamespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("internal_module", 9..23),
            ("expression_statement", 9..23),
        ),
        (
            b"/** d */\nexport namespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("internal_module", 16..30),
            ("export_statement", 9..30),
        ),
        (
            b"/** d */\ndeclare namespace N {}\n",
            comment(0..8, "d", [(0..1, Copied(4..5))]),
            ("internal_module", 17..31),
            ("ambient_declaration", 9..31),
        ),
        (
            b"/**audit*/\nconst x = 1;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 17..22),
            ("lexical_declaration", 11..23),
        ),
        (
            b"/**audit*/\nlet x: number;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 15..24),
            ("lexical_declaration", 11..25),
        ),
        (
            b"/**audit*/\nvar x = 1;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 15..20),
            ("variable_declaration", 11..21),
        ),
        (
            b"/**audit*/\nexport const x = 1;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 24..29),
            ("export_statement", 11..30),
        ),
        (
            b"/**audit*/\ndeclare const x: number;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 25..34),
            ("ambient_declaration", 11..35),
        ),
        (
            b"/**audit*/\nusing h = open();\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("assignment_expression", 11..27),
            ("expression_statement", 11..28),
        ),
        (
            b"/**audit*/\nawait using h = open();\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("assignment_expression", 17..33),
            ("expression_statement", 11..34),
        ),
        (
            b"/**audit*/\nawait /* gap */ using h = open();\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("assignment_expression", 27..43),
            ("expression_statement", 11..44),
        ),
        (
            b"/**audit*/\nconst a = 1, b = 2;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 17..22),
            ("lexical_declaration", 11..30),
        ),
        (
            b"/**audit*/\nconst /* gap */ a = 1;\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("variable_declarator", 27..32),
            ("lexical_declaration", 11..33),
        ),
        (
            b"/**audit*/\nusing /* gap */ h = open();\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("assignment_expression", 11..37),
            ("expression_statement", 11..38),
        ),
        (
            b"/**audit*/\nnamespace /* gap */ N {}\n",
            comment(0..10, "audit", [(0..5, Copied(3..8))]),
            ("internal_module", 11..35),
            ("expression_statement", 11..35),
        ),
    ];
    for (source, document, owner, binding) in rows {
        check(
            language,
            source,
            &wrapped(document, owner.clone(), binding.clone()),
        );
        let parser = clean(language, source);
        let context = DocumentationContext::default();
        let options = DocumentationOptions::default();
        for index in [
            parser.documentation(&context, &options).unwrap(),
            collect(
                language,
                parser.ts_tree(),
                parser.code(),
                &context,
                &options,
            )
            .unwrap(),
        ] {
            let owner_node = node_at(&parser, owner.0, owner.1.clone());
            let binding_node = node_at(&parser, binding.0, binding.1.clone());
            for node in [owner_node, binding_node] {
                let projection = index.node_projection(node).unwrap();
                assert!(!projection.pending());
                assert_eq!(projection.owners().len(), 1);
                let prepared = index.node_associations(node).unwrap();
                assert!(!prepared.projection_pending());
                assert_eq!(prepared.associations().len(), 1);
                let handle = projection.owners().next().unwrap();
                assert_eq!(
                    index
                        .owner_associations(handle)
                        .unwrap()
                        .associations()
                        .len(),
                    1
                );
            }
        }
    }
    let parser = clean(language, b"const x = 1;\n");
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    for index in [
        parser.documentation(&context, &options).unwrap(),
        collect(
            language,
            parser.ts_tree(),
            parser.code(),
            &context,
            &options,
        )
        .unwrap(),
    ] {
        for node in [
            node_at(&parser, "lexical_declaration", 0..12),
            node_at(&parser, "variable_declarator", 6..11),
        ] {
            let projection = index.node_projection(node).unwrap();
            assert!(!projection.pending());
            assert_eq!(projection.owners().len(), 1);
            let prepared = index.node_associations(node).unwrap();
            assert!(!prepared.projection_pending());
            assert_eq!(prepared.associations().len(), 0);
            let handle = projection.owners().next().unwrap();
            assert_eq!(
                index
                    .owner_associations(handle)
                    .unwrap()
                    .associations()
                    .len(),
                0
            );
        }
    }
    check(
        language,
        b"/**one*/\nconst a = 1;\n/**two*/\nnamespace N {}\n",
        &View {
            documents: vec![
                comment(0..8, "one", [(0..3, Copied(3..6))]),
                comment(22..30, "two", [(0..3, Copied(25..28))]),
            ],
            associations: vec![
                link(
                    0,
                    ("variable_declarator", 15..20),
                    ("lexical_declaration", 9..21),
                ),
                link(
                    1,
                    ("internal_module", 31..45),
                    ("expression_statement", 31..45),
                ),
            ],
            ..View::default()
        },
    );
    check(
        language,
        b"namespace N {\n/**audit*/\nconst x = 1;\n}\n",
        &View {
            documents: vec![comment(14..24, "audit", [(0..5, Copied(17..22))])],
            associations: vec![link(
                0,
                ("variable_declarator", 31..36),
                ("lexical_declaration", 25..37),
            )],
            ..View::default()
        },
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_owner_forms() {
    unit2_owner_forms(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_owner_forms() {
    unit2_owner_forms(LANG::Tsx);
}

#[cfg(feature = "typescript")]
#[allow(clippy::too_many_lines)]
fn unit2_literal_boundaries(language: LANG) {
    let sources: [&[u8]; 3] = [
        b"const x = '\\n/** audit */\\nexport default function() {}\\n';\n/**real*/\nconst f = 1;\n",
        b"const x = \"\\n/** audit */\\nexport default function() {}\\n\";\n/**real*/\nconst f = 1;\n",
        b"const x = `\n/** audit */\nexport default function() {}\n`;\n/**real*/\nconst f = 1;\n",
    ];
    for source in sources {
        let comment_start = source.len() - b"/**real*/\nconst f = 1;\n".len();
        let declaration_start = comment_start + b"/**real*/\n".len();
        check(
            language,
            source,
            &wrapped(
                comment(
                    comment_start..comment_start + 9,
                    "real",
                    [(0..4, Copied(comment_start + 3..comment_start + 7))],
                ),
                (
                    "variable_declarator",
                    declaration_start + 6..declaration_start + 11,
                ),
                (
                    "lexical_declaration",
                    declaration_start..declaration_start + 12,
                ),
            ),
        );
    }
    let block = b"/*\n/** audit */\nexport default function() {}\n*/\n/**real*/\nfunction f() {}\n";
    let jsx = b"const x = <div>\n/** audit */\nexport default function() {}\n</div>;\n/**real*/\nfunction f() {}\n";
    let assertion = b"const a = <number>b;\n/**real*/\nfunction f() {}\n";
    let default_assertion =
        b"export default <Config>{ a: 1 };\n/**real*/\nexport function f() {}\n";
    let body_assertion = b"function g() { return <number>b; }\n/**real*/\nfunction f() {}\n";
    let expected_function = |start, binding: Site, owner: Site| View {
        documents: vec![comment(
            start..start + 9,
            "real",
            [(0..4, Copied(start + 3..start + 7))],
        )],
        associations: vec![link(0, owner, binding)],
        ..View::default()
    };
    check(language, block, &View::default());
    if language == LANG::Tsx {
        check(
            language,
            jsx,
            &expected_function(
                66,
                ("function_declaration", 76..91),
                ("function_declaration", 76..91),
            ),
        );
        for source in [assertion.as_slice(), default_assertion] {
            check(language, source, &View::default());
        }
        check(
            language,
            body_assertion,
            &View {
                assessments: unordered([pending(
                    NEEDS_PROJECTION,
                    ("function_declaration", 0..34),
                    None,
                )]),
                ..expected_function(
                    35,
                    ("function_declaration", 45..60),
                    ("function_declaration", 45..60),
                )
            },
        );
    } else {
        check(
            language,
            jsx,
            &View {
                assessments: unordered([pending(
                    NEEDS_PROJECTION,
                    ("lexical_declaration", 0..91),
                    None,
                )]),
                ..View::default()
            },
        );
        check(
            language,
            assertion,
            &expected_function(
                21,
                ("function_declaration", 31..46),
                ("function_declaration", 31..46),
            ),
        );
        check(
            language,
            default_assertion,
            &expected_function(
                33,
                ("export_statement", 43..65),
                ("function_declaration", 50..65),
            ),
        );
        check(
            language,
            body_assertion,
            &expected_function(
                35,
                ("function_declaration", 45..60),
                ("function_declaration", 45..60),
            ),
        );
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_literal_boundaries() {
    unit2_literal_boundaries(LANG::Tsx);
}

#[cfg(feature = "typescript")]
#[derive(Default)]
struct TsSteps {
    steps: Cell<usize>,
    bytes: Cell<usize>,
    stop: Option<usize>,
    stop_byte: Option<usize>,
}

#[cfg(feature = "typescript")]
impl WorkBudget for TsSteps {
    type Error = SourceError;

    fn checkpoint(&self) -> Result<(), SourceError> {
        Ok(())
    }

    #[cfg(feature = "typescript")]
    fn step(&self) -> Result<(), SourceError> {
        self.steps.set(self.steps.get() + 1);
        if self.stop == Some(self.steps.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "typescript")]
    fn bytes(&self, count: usize) -> Result<(), SourceError> {
        self.bytes.set(self.bytes.get() + count);
        if self.stop_byte == Some(self.bytes.get()) {
            Err(SourceError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "typescript")]
fn unit2_budgeted_binding_scan(language: LANG) {
    for (source, expected_steps, expected_owner) in [
        (b"/**audit*/\nconst x = 1;\n".as_slice(), 3, Some(17..22)),
        (b"/**audit*/\nconst /* gap */ a = 1;\n", 4, Some(27..32)),
        (b"/**audit*/\nconst a = 1, b = 2;\n", 3, Some(17..22)),
        (b"/**audit*/\nconst { x } = o;\n", 3, None),
    ] {
        let parser = clean(language, source);
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let declaration = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "lexical_declaration")
            .unwrap();
        let id = index
            .canonical
            .lookup(declaration.as_tree_sitter())
            .unwrap();
        let budget = TsSteps::default();
        let found = super::projections::ts_binding_declarator(&index.canonical, id, &budget)
            .unwrap()
            .map(|child| index.canonical.nodes[child.0].evidence.node().byte_range());
        assert_eq!(found, expected_owner);
        // expected: keyword, optional comment, first declarator, then one name lookup.
        assert_eq!(budget.steps.get(), expected_steps);
        assert_eq!(budget.bytes.get(), 0);
        for step in 1..=expected_steps {
            let budget = TsSteps {
                stop: Some(step),
                ..TsSteps::default()
            };
            assert!(matches!(
                super::projections::ts_binding_declarator(&index.canonical, id, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), step);
        }
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_budgeted_binding_scan() {
    unit2_budgeted_binding_scan(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_budgeted_binding_scan() {
    unit2_budgeted_binding_scan(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn unit2_budgeted_await_scan(language: LANG) {
    for (source, expected_steps, expected_bytes) in [
        (b"/**audit*/\nawait using h = open();\n".as_slice(), 3, 2),
        (
            b"/**audit*/\nawait /* a */ /* b */ using h = open();\n",
            5,
            18,
        ),
    ] {
        let parser = clean(language, source);
        let statement = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "await_expression")
            .unwrap();
        let node = crate::Node::from_tree_sitter(statement.as_tree_sitter());
        let parent = crate::Node::from_tree_sitter(statement.parent().unwrap().as_tree_sitter());
        let chain = [parent];
        let ancestors = crate::Ancestors::known(&chain);
        let budget = TsSteps::default();
        assert!(super::ts_await_using(&node, source, ancestors, &budget).unwrap());
        assert_eq!(budget.steps.get(), expected_steps);
        // expected: await-to-assignment gap, then using-to-name gap.
        assert_eq!(budget.bytes.get(), expected_bytes);
        for step in 1..=expected_steps {
            let budget = TsSteps {
                stop: Some(step),
                ..TsSteps::default()
            };
            assert!(matches!(
                super::ts_await_using(&node, source, ancestors, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), step);
        }
        for byte in 1..=expected_bytes {
            let budget = TsSteps {
                stop_byte: Some(byte),
                ..TsSteps::default()
            };
            assert!(matches!(
                super::ts_await_using(&node, source, ancestors, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.bytes.get(), byte);
        }
    }
    let source = format!(
        "/**audit*/\nawait {}using h = open();\n",
        "/* gap */ ".repeat(1_200)
    );
    let parser = clean(language, source.as_bytes());
    let options = DocumentationOptions::default();
    let context = DocumentationContext::default();
    let result = crate::documentation::budget::cancel_in_phase(
        crate::documentation::budget::WorkPhase::TsAwait,
        || parser.documentation(&context, &options),
    );
    assert!(matches!(result, Err(CollectionError::Cancelled)));
    assert_eq!(
        parser
            .documentation(&context, &options)
            .unwrap()
            .associations()
            .len(),
        1
    );
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_budgeted_await_scan() {
    unit2_budgeted_await_scan(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_budgeted_await_scan() {
    unit2_budgeted_await_scan(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn unit2_restricted_keyword_costs(language: LANG) {
    for (source, expected_steps, expected_bytes) in [
        (
            b"/** d */\nexport default async function() {}\n".as_slice(),
            2,
            1,
        ),
        (
            b"/** d */\nexport default async /* a */ /* b */ function() {}\n",
            4,
            17,
        ),
        (
            b"/** d */\nexport default async /*\n*/ function() {}\n",
            3,
            4,
        ),
    ] {
        let parser = clean(language, source);
        let node = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "function_expression")
            .unwrap();
        let value = crate::Node::from_tree_sitter(node.as_tree_sitter());
        let budget = TsSteps::default();
        let result = super::ts_async_export_value(&value, source, &budget).unwrap();
        assert_eq!(result, !source.windows(3).any(|w| w == b"/*\n"));
        // expected: `async`, optional comment children, and `function` are polled once.
        assert_eq!(budget.steps.get(), expected_steps);
        // expected: the gap between `async` and `function`, through a terminator if present.
        assert_eq!(budget.bytes.get(), expected_bytes);
        for step in 1..=expected_steps {
            let budget = TsSteps {
                stop: Some(step),
                ..TsSteps::default()
            };
            assert!(matches!(
                super::ts_async_export_value(&value, source, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), step);
        }
        for byte in 1..=expected_bytes {
            let budget = TsSteps {
                stop_byte: Some(byte),
                ..TsSteps::default()
            };
            assert!(matches!(
                super::ts_async_export_value(&value, source, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.bytes.get(), byte);
        }
    }
    for (source, expected_steps, expected_bytes) in [
        (b"/** d */\ndeclare abstract class A {}\n".as_slice(), 2, 1),
        (
            b"/** d */\ndeclare /* a */ /* b */ abstract class A {}\n",
            4,
            17,
        ),
        (b"/** d */\ndeclare /*\n*/ abstract class A {}\n", 3, 4),
    ] {
        let parser = clean(language, source);
        let index = parser
            .documentation(
                &DocumentationContext::default(),
                &DocumentationOptions::default(),
            )
            .unwrap();
        let ambient = parser
            .root_node()
            .preorder()
            .find(|node| node.kind() == "ambient_declaration")
            .unwrap();
        let id = index.canonical.lookup(ambient.as_tree_sitter()).unwrap();
        let budget = TsSteps::default();
        let builder = super::Builder {
            index,
            drafts: Vec::new(),
            pending_attributes: Vec::new(),
            rust_values: std::collections::HashMap::new(),
            names: super::super::names::Names::default(),
            pod_doubt: super::pod_doubt::Doubt::default(),
            pod_hidden: super::pod::Hidden::default(),
            tcl_scan: crate::lang_helpers::tcl_scan::CommandScan::default(),
            shdoc: crate::lang_helpers::shdoc::Reading::default(),
        };
        let result = builder.ts_ambient_continues(id, &budget).unwrap();
        assert_eq!(result, !source.windows(3).any(|w| w == b"/*\n"));
        // expected: `declare`, optional comment children, then the first declaration child.
        assert_eq!(budget.steps.get(), expected_steps);
        // expected: every gap byte until the first line terminator, or the full gap.
        assert_eq!(budget.bytes.get(), expected_bytes);
        for step in 1..=expected_steps {
            let budget = TsSteps {
                stop: Some(step),
                ..TsSteps::default()
            };
            assert!(matches!(
                builder.ts_ambient_continues(id, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.steps.get(), step);
        }
        for byte in 1..=expected_bytes {
            let budget = TsSteps {
                stop_byte: Some(byte),
                ..TsSteps::default()
            };
            assert!(matches!(
                builder.ts_ambient_continues(id, &budget),
                Err(SourceError::Cancelled)
            ));
            assert_eq!(budget.bytes.get(), byte);
        }
    }
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_restricted_keyword_costs() {
    unit2_restricted_keyword_costs(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_restricted_keyword_costs() {
    unit2_restricted_keyword_costs(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn unit2_binding_collection_cancels(language: LANG) {
    use crate::documentation::budget;
    let source = format!("/**audit*/\nconst {}x = 1;\n", "/* gap */ ".repeat(2_048));
    let parser = clean(language, source.as_bytes());
    let context = DocumentationContext::default();
    let options = DocumentationOptions::default();
    let result = budget::cancel_in_phase(budget::WorkPhase::Projection, || {
        parser.documentation(&context, &options)
    });
    assert!(matches!(result, Err(CollectionError::Cancelled)));
    let completed = parser.documentation(&context, &options).unwrap();
    assert_eq!(completed.associations().len(), 1);
}

#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_binding_collection_cancels() {
    unit2_binding_collection_cancels(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_binding_collection_cancels() {
    unit2_binding_collection_cancels(LANG::Tsx);
}

#[cfg(feature = "typescript")]
fn unit2_controls(language: LANG) {
    for source in [
        b"/**audit*/\nconst { x } = o;\n".as_slice(),
        b"/**audit*/\nx = 1;\n",
        b"/**audit*/\nexport default 1;\n",
    ] {
        check_retained(
            language,
            source,
            View::default(),
            CandidateDisposition::Unattached,
            0..10,
        );
    }
    for source in [
        b"/**audit*/\nusing a = open(), b = open();\n".as_slice(),
        b"/**audit*/\nlet\nx = 1;\n",
    ] {
        check_retained(
            language,
            source,
            View::default(),
            CandidateDisposition::Unattached,
            0..10,
        );
    }
}
#[cfg(feature = "typescript")]
#[test]
fn typescript_unit2_controls() {
    unit2_controls(LANG::Typescript);
}
#[cfg(feature = "typescript")]
#[test]
fn tsx_unit2_controls() {
    unit2_controls(LANG::Tsx);
}
