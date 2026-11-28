# Extracting source documentation {#extracting-source-documentation}

Use `Ast::documentation` when a Rust program needs the source comments or attributes attached
to declarations, their cleaned local text, and a way back to the source bytes. This chapter
assumes the [tree-sitter](https://tree-sitter.github.io/tree-sitter/) model of nodes and byte
ranges. For parsing and tree adoption, see [Parse once](parse-once.md); for direct node traversal,
see [Walking the AST directly](ast-traversal.md).

`Ast::documentation` and its argument and result types are part of the public
`big-code-analysis` library API. Import the types through
`big_code_analysis::documentation`; a separate dependency on
`big-code-analysis-ast` is not required. The examples below run in this
repository's integration tests with the `rust` Cargo feature enabled.

## Collect and read a document {#collect-and-read}

Parse once, then collect against that parse. An index borrows the `Ast`'s tree and source buffer;
keep the `Ast` alive while reading the index. Default options select the language's profile.

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};
use big_code_analysis::documentation::{DocumentationContext, DocumentationOptions};

let ast = Ast::parse(Source::new(LANG::Rust, b"/// audit\nfn f() {}"))
    .expect("rust feature enabled");
let index = ast.documentation(&DocumentationContext::default(), &DocumentationOptions::default())
    .expect("Rust adapter available");
assert_eq!(index.documents().len(), 1);
let document = index.documents().next().expect("one document");
let fragment = index.facts().document_fragment(document, 0).expect("first fragment");
assert_eq!(fragment.get().source().pieces()[0].bytes(), b"/// audit");
let complete = index.complete_text(document).expect("same index").expect("complete text");
assert_eq!(complete.text(), "audit");
```

A fragment holds raw included source spelling, including markers. A document combines one or
more fragments and records its role and content state. `complete_text` gives cleaned local text,
not rendered markup or generator-expanded output. `None` does **not** mean no document: inspect
`document.get().content()` to distinguish unavailable, pending, disabled, explicit absence,
and metadata-only content. No entry in `documents()` means that collection produced no selected
document. Pending assessments remain visible in `index.assessments()`.

## Find an owner and map text to source {#owners-and-origins}

This example finds an `@audit` annotation in the cleaned text of a two-fragment Rust document.
The newline inserted between its fragments has no source bytes. A query over the entire text
therefore returns several origin portions; never assume one continuous source span.

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};
use big_code_analysis::documentation::{DocumentationContext, DocumentationOptions, OriginView, Owner, Resolution};

let ast = Ast::parse(Source::new(LANG::Rust, b"/// @audit\n#[doc = \"detail\"]\nfn f() {}"))
    .expect("rust feature enabled");
let index = ast.documentation(&DocumentationContext::default(), &DocumentationOptions::default())
    .expect("Rust adapter available");
let document = index.documents().next().expect("one document");
assert_eq!(document.get().fragment_count(), 2);
let complete = index.complete_text(document).expect("same index").expect("complete text");
assert_eq!(complete.text(), "@audit\ndetail");
let annotation = complete.text().find("@audit").expect("annotation");
let origin: Vec<_> = complete.origins(annotation..annotation + "@audit".len())
    .expect("valid UTF-8 byte range").collect();
assert_eq!(origin.len(), 1);
let OriginView::Copied(piece) = &origin[0].origin else { panic!("copied annotation") };
assert_eq!(piece.bytes(), b"@audit");
assert_eq!(piece.range(), 4..10);
let all: Vec<_> = complete.origins(0..complete.text().len()).expect("valid range").collect();
assert!(all.len() > 1);
assert!(all.iter().any(|portion| matches!(portion.origin, OriginView::Synthetic)));
let association = index.associations().next().expect("one association");
assert_eq!(association.get().resolution, Resolution::Resolved);
assert_eq!(association.get().owner_count(), 1);
assert_eq!(index.facts().associated_document(association).expect("same index"), document);
let owner = index.facts().associated_owner(association, 0).expect("resolved owner");
let Owner::Node(node) = owner.get() else { panic!("function owner") };
assert_eq!(node.node().kind(), "function_item");
let matches = index.node_associations(node.node()).expect("canonical node");
assert_eq!(matches.associations().count(), 1);
```

An association identifies a document, its attachment convention, its resolution and its
publication status. A `Resolved` association names actual owners; an unresolved association
retains candidates, not owner claims. Owners can be `Owner::Node` for a declaration,
`Owner::Slot` for a field or binding, `Owner::File` for a file, or `Owner::Group` for a
multi-piece declaration. `Owner::Virtual` is also available for explicitly documented
virtual targets. A slot can be inspected through `facts().slot_enclosing(owner)`; a group has
ordered source pieces. Neither a semantic owner nor a lexical scope is necessarily a metric
`FuncSpace`. Use `index.node_associations(node)` for a node from this exact tree; for wrappers
with multiple owners, use `index.node_owners(node)` instead. Do not reuse a fact handle from
another index, even over the same tree.

## Distinguish empty results from errors {#empty-and-errors}

An empty source can return a valid empty index. A cancelled collection does not return an
index. Unsupported profiles and unavailable adapters are errors, not evidence that a file
has no documentation.

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};
use big_code_analysis::documentation::{CollectionError, DocumentationContext, DocumentationOptions, Profile};

let context = DocumentationContext::default();
let empty = Ast::parse(Source::new(LANG::Rust, b"")).expect("rust feature enabled");
let index = empty.documentation(&context, &DocumentationOptions::default())
    .expect("empty but complete");
assert_eq!(index.documents().len(), 0);
let ast = Ast::parse(Source::new(LANG::Rust, b"/// audit\nfn f() {}"))
    .expect("rust feature enabled");
let wrong = DocumentationOptions::default().with_profile(Profile::Javadoc);
assert!(matches!(ast.documentation(&context, &wrong),
    Err(CollectionError::UnsupportedProfile {
        language: LANG::Rust, profile: Profile::Javadoc, ..
    })));
```

Handle `CollectionError::Cancelled`, `LanguageDisabled`, `UnsupportedProfile`,
`AdapterUnavailable`, `GrammarMismatch`, `SourceBounds` and `Source` as separate failure
conditions. A completed index may still contain pending assessments or documents whose
`Content` is `Unavailable` or `PendingAdapter`. Inspect these before treating a missing
association as a supported absence.

A present document with empty complete text differs from no document. A byte-string
attribute retains its raw fragment when its value cannot be decoded as prose:

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};
use big_code_analysis::documentation::{Content, DocumentationContext, DocumentationOptions};

let context = DocumentationContext::default();
let options = DocumentationOptions::default();
let empty = Ast::parse(Source::new(LANG::Rust, b"#[doc = \"\"] fn f() {}"))
    .expect("rust feature enabled");
let index = empty.documentation(&context, &options).expect("Rust adapter available");
assert_eq!(index.documents().len(), 1);
let document = index.documents().next().expect("present document");
assert_eq!(index.complete_text(document).expect("same index").expect("complete").text(), "");

let computed = Ast::parse(Source::new(LANG::Rust, br#"#[doc = b"\xff"] fn f() {}"#))
    .expect("rust feature enabled");
let index = computed.documentation(&context, &options).expect("Rust adapter available");
let document = index.documents().next().expect("present document");
assert!(matches!(document.get().content(), Content::Unavailable(_)));
assert!(index.complete_text(document).expect("same index").is_none());
let fragment = index.facts().document_fragment(document, 0).expect("raw fragment");
assert!(fragment.get().source().pieces()[0].bytes().starts_with(b"#[doc ="));
```

## Choose source coordinates and a profile {#source-and-profiles}

`Ast::parse` uses the supplied bytes (or, with explicit C-family preprocessor inputs, the
expanded bytes). `Ast::from_path` reads a file and normalizes its end-of-line characters and
UTF-8 BOM before parsing. Every fragment and origin range indexes `index.source()`, which is
the same buffer as `ast.source()`, **not** necessarily the file's original bytes. A path-backed
`Ast` supplies its final path component as the default logical filename. A name set on an
in-memory `Source` is only a display label: use
`DocumentationContext::default().with_logical_filename(name)` when a profile
needs that filename. The caller's explicit filename takes precedence.

`Ast::from_tree_sitter` adopts the caller's tree and bytes. For a parse with included ranges,
excluded gaps are not source text: fragments retain separate included pieces and can report
`has_excluded_gap()`. An incomplete tree or a mismatch between adopted tree and buffer cannot
be treated as an empty document result. See [the tree-adoption precondition](parse-once.md#adopting-a-caller-built-tree).

Check source offsets against the buffer that was parsed, especially for a path read from a
file with CRLF line endings:

```rust,ignore
use big_code_analysis::Ast;
use big_code_analysis::documentation::{DocumentationContext, DocumentationOptions};

let dir = tempfile::tempdir().expect("temporary directory");
let path = dir.path().join("example.rs");
std::fs::write(&path, b"/// a\r\n/// b\r\nfn f() {}\r\n").expect("write source");
let ast = Ast::from_path(&path).expect("read Rust source");
assert_eq!(ast.source(), b"/// a\n/// b\nfn f() {}\n");
let index = ast.documentation(&DocumentationContext::default(), &DocumentationOptions::default())
    .expect("Rust adapter available");
assert_eq!(index.context().logical_filename.as_deref(), Some("example.rs"));
let document = index.documents().next().expect("one document");
let second = index.facts().document_fragment(document, 1).expect("second fragment");
assert_eq!(second.get().source().pieces()[0].range(), 6..11);
assert_eq!(index.complete_text(document).expect("same index").expect("complete").text(), "a\nb");
```

`DocumentationOptions::default()` selects the language's named profile. Use
`with_profile` to choose another profile. The
[language-profile table](../languages.md#documentation-profiles) lists the defaults and
current unavailable adapters. `Ccomment` and `Preproc` are helper grammars: a successful
collection reports `Capability::NoDeclarationModel`, not a set of declarations. Profile
compatibility is checked before collection, and a valid but unfinished adapter can return
`CollectionError::AdapterUnavailable`.

Source extraction and [DLOC](../metrics.md#lines-of-code) answer different questions.
DLOC and documentation-aware [MI](../metrics.md#maintainability-index-mi) count legacy
documentation rows under metric policy. Extraction records source fragments, document text
and semantic attachment, independently of metric selection and test pruning. A source
document does not imply that the same line is charged to DLOC; a DLOC row does not establish
an owner. For the contributor rules behind these paths, see the
[extraction contract](../../../docs/development/documentation_extraction.md#recognition),
[role tables](https://docs.rs/big-code-analysis-ast/*/big_code_analysis_ast/roles/index.html)
and [grammar dispatch](../../../docs/development/grammar_dispatch.md).

[Runnable copies of these examples](https://github.com/dekobon/big-code-analysis/blob/main/tests/api/book_documentation_examples.rs)
guard the calls and expected values against API drift.
