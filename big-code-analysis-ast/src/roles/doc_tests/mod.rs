//! The seven documentation rows — `Comment`, `DocComment`, `Docstring`,
//! `DocTarget`, `DocRunOpener`, `DocRunDirective`, `HoldsDocInBody` —
//! pinned independently of the `Checker` predicates that read them
//! (#1699): for each language, a fixture spelling every owner kind and
//! both alternatives of every gate, and a hand-written table of what
//! each row answers per node, as the predicates answered before they
//! read the rows. Beside the rows, the two byte-less `Checker`
//! spellings (`is_comment`, `is_doc_comment`) are read on the same
//! node, so a forward that stops reading its row fails on the same
//! line as a row that loses a kind. The `DocComment` half of
//! `is_comment`'s union is observable only where a language declares
//! a `DocComment` owner the `Comment` row does not also hold — Groovy's
//! `groovydoc_comment`, Perl's `pod_statement` — so the comment-only
//! languages below pin the `Comment` forward alone.
//!
//! A kind absent from a table answers all-false; every declared owner
//! must be met by the fixture; a defensive alias must stay declared and
//! unseen (`owners_covered`'s shape, #1698).
//!
//! Beyond the per-language tables: where a Checker composes a row with
//! context it cannot see (`checker/<lang>.rs` — a wrapper's parent, a
//! scope fold, a run's content), a `checker_composes_*` test reads row
//! and Checker on the same nodes; the Doxygen family holds each row to
//! the exact id set the old name list denoted; and `parity.rs` holds
//! the cross-language pins — the derived space-tree parity on the
//! declared lists and on gated nodes, and `is_doc_comment ⇒ is_comment`
//! as the Checker answers it.

use std::collections::BTreeSet;

use crate::test_support::{for_each_node_with_chain, owners_covered};
use crate::{Ancestors, Checker, Node};

/// What one node answers: the seven rows' `owns_kind`, then the three
/// byte-less `Checker` spellings that read them with no chain —
/// `is_comment`, `is_doc_comment`, `is_doc_run_directive` — so a
/// forward that hands its row an empty chain is read beside the row
/// asked with the real one. The slots are named in `slot`, which is
/// what the tables spell.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
type Answers = [bool; 10];

/// The slot of each reading in [`Answers`].
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod slot {
    pub(super) const COMMENT: usize = 0;
    pub(super) const DOC_COMMENT: usize = 1;
    pub(super) const DOCSTRING: usize = 2;
    pub(super) const DOC_TARGET: usize = 3;
    pub(super) const RUN_OPENER: usize = 4;
    pub(super) const RUN_DIRECTIVE: usize = 5;
    pub(super) const HOLDS_IN_BODY: usize = 6;
    pub(super) const IS_COMMENT: usize = 7;
    pub(super) const IS_DOC_COMMENT: usize = 8;
    pub(super) const IS_RUN_DIRECTIVE: usize = 9;
}
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
use slot::*;

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const NONE: Answers = [false; 10];

/// A reader over a language's seven row modules and its `Checker`.
// test-lang-gates: hand-written(c, cpp, mozcpp, objc) — read by
//     `doc_tests/doxygen.rs` through `use super::*`, which the derivation
//     does not follow across files
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
macro_rules! answers {
    ($roles:path, $checker:ty) => {{
        use $roles::*;
        |node: &Node<'_>, code: &[u8], ancestors: Ancestors<'_, '_>| -> Answers {
            let kind = node.kind_id();
            [
                comment::owns_kind(kind, node, code, ancestors),
                doc_comment::owns_kind(kind, node, code, ancestors),
                docstring::owns_kind(kind, node, code, ancestors),
                doc_target::owns_kind(kind, node, code, ancestors),
                doc_run_opener::owns_kind(kind, node, code, ancestors),
                doc_run_directive::owns_kind(kind, node, code, ancestors),
                holds_doc_in_body::owns_kind(kind, node, code, ancestors),
                <$checker>::is_comment(node),
                <$checker>::is_doc_comment(node, code),
                <$checker>::is_doc_run_directive(node, code),
            ]
        }
    }};
}

/// Every declared owner of the seven rows, for the fixture guard.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
macro_rules! declared {
    ($roles:path) => {{
        use $roles::*;
        [
            comment::OWNERS,
            doc_comment::OWNERS,
            docstring::OWNERS,
            doc_target::OWNERS,
            doc_run_opener::OWNERS,
            doc_run_directive::OWNERS,
            holds_doc_in_body::OWNERS,
        ]
    }};
}

/// Walks `source`, answers the rows and the `Checker` for every node
/// whose kind the `expected` table names (or that any reading answers
/// non-false for), and compares against the table in walk order, with
/// the node's text beside each row so a table entry can be told from
/// its neighbours. `defensive` kinds must be declared and never
/// emitted; every other declared owner must be.
// test-lang-gates: hand-written(c, cpp, mozcpp, objc) — read by
//     `doc_tests/doxygen.rs` through `use super::*`, which the derivation
//     does not follow across files
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn check<C: Checker + crate::LanguageInfo>(
    source: &[u8],
    read: impl Fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> Answers,
    expected: &[(&str, &str, Answers)],
    declared: &[&[u16]],
    defensive: &[u16],
) {
    let mut seen_kinds = BTreeSet::new();
    let mut got: Vec<(String, String, Answers)> = Vec::new();
    for_each_node_with_chain::<C>(source, |node, chain| {
        seen_kinds.insert(node.kind_id());
        let answers = read(node, source, Ancestors::known(chain));
        let listed = node.is_named() && expected.iter().any(|(k, ..)| *k == node.kind());
        if listed || answers != NONE {
            let text = node.utf8_text(source).unwrap_or("<non-utf8>");
            let text = text.lines().next().unwrap_or("").to_owned();
            got.push((node.kind().to_owned(), text, answers));
        }
    });
    let want: Vec<(String, String, Answers)> = expected
        .iter()
        .map(|(k, t, a)| ((*k).to_owned(), (*t).to_owned(), *a))
        .collect();
    pretty_assertions::assert_eq!(got, want);
    owners_covered(&seen_kinds, declared, defensive);
}

/// The answers with the slots in `set` true.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const fn at(set: &[usize]) -> Answers {
    let mut a = NONE;
    let mut i = 0;
    while i < set.len() {
        a[set[i]] = true;
        i += 1;
    }
    a
}

/// A plain comment: the `Comment` row and `is_comment`, nothing else.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozjs",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const PLAIN: Answers = at(&[COMMENT, IS_COMMENT]);

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const DOC: Answers = at(&[COMMENT, DOC_COMMENT, IS_COMMENT, IS_DOC_COMMENT]);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
const TARGET: Answers = at(&[DOC_TARGET]);

// The comment-only languages: one `Comment` row, six absent rows, and
// `is_comment` reading the row's kind table. Each fixture carries a
// comment that *looks* like documentation to another language's tool
// — a `##`, a `---`, a `/** */` — so the table pins that no
// documentation reading answers for it here.

// The Doxygen family's exact-alias guards and recovery pins, and the
// cross-language parity and implication pins, in their own files; both
// read the harness above.
#[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
mod doxygen;
mod parity;

#[cfg(feature = "bash")]
mod bash {
    use super::*;
    use crate::BashCode;

    #[test]
    fn rows_answer_the_table() {
        let source = b"#!/usr/bin/env bash\n# plain\n## doubled, still plain\nf() {\n  # in a body\n  echo 1\n}\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "#!/usr/bin/env bash", PLAIN),
            ("comment", "# plain", PLAIN),
            ("comment", "## doubled, still plain", PLAIN),
            ("comment", "# in a body", PLAIN),
        ];
        check::<BashCode>(
            source,
            answers!(crate::roles::bash, BashCode),
            expected,
            &declared!(crate::roles::bash),
            &[],
        );
    }
}

#[cfg(feature = "tcl")]
mod tcl {
    use super::*;
    use crate::TclCode;

    #[test]
    fn rows_answer_the_table() {
        let source =
            b"# plain\n## doubled, still plain\nproc f {} {\n  # in a body\n  return 1\n}\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "# plain", PLAIN),
            ("comment", "## doubled, still plain", PLAIN),
            ("comment", "# in a body", PLAIN),
        ];
        check::<TclCode>(
            source,
            answers!(crate::roles::tcl, TclCode),
            expected,
            &declared!(crate::roles::tcl),
            &[],
        );
    }
}

#[cfg(feature = "irules")]
mod irules {
    use super::*;
    use crate::IrulesCode;

    #[test]
    fn rows_answer_the_table() {
        let source =
            b"# plain\n## doubled, still plain\nwhen HTTP_REQUEST {\n  # in a body\n  pool p\n}\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "# plain", PLAIN),
            ("comment", "## doubled, still plain", PLAIN),
            ("comment", "# in a body", PLAIN),
        ];
        check::<IrulesCode>(
            source,
            answers!(crate::roles::irules, IrulesCode),
            expected,
            &declared!(crate::roles::irules),
            &[],
        );
    }
}

#[cfg(feature = "c-family-helpers")]
mod ccomment {
    use super::*;
    use crate::CcommentCode;

    // A `/** */` is a comment the stripping pass elides, not
    // documentation: the pass reads none.
    #[test]
    fn rows_answer_the_table() {
        let source = b"/** doc-looking */\nint x; // plain\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "/** doc-looking */", PLAIN),
            ("comment", "// plain", PLAIN),
        ];
        check::<CcommentCode>(
            source,
            answers!(crate::roles::ccomment, CcommentCode),
            expected,
            &declared!(crate::roles::ccomment),
            &[],
        );
    }
}

#[cfg(feature = "c-family-helpers")]
mod preproc {
    use super::*;
    use crate::PreprocCode;

    #[test]
    fn rows_answer_the_table() {
        // A comment after a directive on its row is the directive's
        // text to this grammar, so the plain one sits on its own row.
        let source = b"/** doc-looking */\n#define X 1\n// plain\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "/** doc-looking */", PLAIN),
            ("comment", "// plain", PLAIN),
        ];
        check::<PreprocCode>(
            source,
            answers!(crate::roles::preproc, PreprocCode),
            expected,
            &declared!(crate::roles::preproc),
            &[],
        );
    }
}

#[cfg(feature = "elixir")]
mod elixir {
    use super::*;
    use crate::ElixirCode;

    // `@doc` and `@moduledoc` are `unary_operator`s over a call holding
    // a string, not comments and not bare docstrings: nothing here
    // answers for them (D19, #1676). Both attributes and their strings
    // are listed all-false so the fixture cannot lose them unnoticed
    // (testing.md, "Perturb the fixture as well"). A `#` comment is
    // the one comment kind.
    #[test]
    fn rows_answer_the_table() {
        let source = b"# plain\ndefmodule M do\n  @moduledoc \"\"\"\n  doc\n  \"\"\"\n  @doc \"a doc\"\n  # in a body\n  def f, do: 1\nend\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "# plain", PLAIN),
            ("unary_operator", "@moduledoc \"\"\"", NONE),
            ("string", "\"\"\"", NONE),
            ("unary_operator", "@doc \"a doc\"", NONE),
            ("string", "\"a doc\"", NONE),
            ("comment", "# in a body", PLAIN),
        ];
        check::<ElixirCode>(
            source,
            answers!(crate::roles::elixir, ElixirCode),
            expected,
            &declared!(crate::roles::elixir),
            &[],
        );
    }
}

// The marker languages: a `Comment` row, a `DocComment` row under a
// byte gate, a `DocTarget` row, and where the Checker names kinds
// beyond the callables, a `HoldsDocInBody` row. Each fixture carries
// every target kind, each `HoldsDocInBody` kind with a doc comment in
// it, and the marker's edges — the empty `/**/`, a `/***`, a `///`.

#[cfg(feature = "java")]
mod java {
    use super::*;
    use crate::JavaCode;

    /// A kind that holds a doc comment in its body beyond the callables.
    const BODY: Answers = at(&[HOLDS_IN_BODY]);

    // Every target kind, the package gate's three cases, every
    // `HoldsDocInBody` kind holding a doc comment, and the marker's
    // edges. The package rule: `package a;` has a package after it, so
    // it documents; `package b;` has a class after it, so it does not;
    // `package c;` at the end has nothing, so it does. The `/**/` is a
    // comment and not documentation; `///` is (JEP 467).
    #[test]
    fn rows_answer_the_table() {
        let source = b"/** file */\npackage a;\n/** dangling: a class follows */\npackage b;\n/**/\n// line\n/// markdown\nclass C {\n  /** f */\n  int f;\n  C() { }\n  static { /** in static */ int s = 1; }\n  { /** in block */ int b = 1; }\n  void m() {\n    /* plain */ int x = switch (f) { default -> { /** in switch */ yield 1; } };\n    Runnable r = () -> { /** in lambda */ };\n  }\n}\nenum E { /** k */ K; }\nrecord R(int a) { R { } }\ninterface I { int K = 1; }\n@interface An { int v() default 1; }\n/** module */\nmodule m { }\npackage c;\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("block_comment", "/** file */", DOC),
            ("package_declaration", "package a;", TARGET),
            ("block_comment", "/** dangling: a class follows */", DOC),
            ("package_declaration", "package b;", NONE),
            ("block_comment", "/**/", PLAIN),
            ("line_comment", "// line", PLAIN),
            ("line_comment", "/// markdown", DOC),
            ("class_declaration", "class C {", TARGET),
            ("block_comment", "/** f */", DOC),
            ("field_declaration", "int f;", TARGET),
            ("constructor_declaration", "C() { }", TARGET),
            (
                "static_initializer",
                "static { /** in static */ int s = 1; }",
                BODY,
            ),
            ("block", "{ /** in static */ int s = 1; }", BODY),
            ("block_comment", "/** in static */", DOC),
            ("block", "{ /** in block */ int b = 1; }", BODY),
            ("block_comment", "/** in block */", DOC),
            ("method_declaration", "void m() {", TARGET),
            ("block", "{", BODY),
            ("block_comment", "/* plain */", PLAIN),
            (
                "switch_block",
                "{ default -> { /** in switch */ yield 1; } }",
                BODY,
            ),
            ("block", "{ /** in switch */ yield 1; }", BODY),
            ("block_comment", "/** in switch */", DOC),
            ("block", "{ /** in lambda */ }", BODY),
            ("block_comment", "/** in lambda */", DOC),
            ("enum_declaration", "enum E { /** k */ K; }", TARGET),
            ("block_comment", "/** k */", DOC),
            ("enum_constant", "K", TARGET),
            ("record_declaration", "record R(int a) { R { } }", TARGET),
            ("compact_constructor_declaration", "R { }", TARGET),
            ("block", "{ }", BODY),
            (
                "interface_declaration",
                "interface I { int K = 1; }",
                TARGET,
            ),
            ("constant_declaration", "int K = 1;", TARGET),
            (
                "annotation_type_declaration",
                "@interface An { int v() default 1; }",
                TARGET,
            ),
            (
                "annotation_type_element_declaration",
                "int v() default 1;",
                TARGET,
            ),
            ("block_comment", "/** module */", DOC),
            ("module_declaration", "module m { }", TARGET),
            ("package_declaration", "package c;", TARGET),
        ];
        check::<JavaCode>(
            source,
            answers!(crate::roles::java, JavaCode),
            expected,
            &declared!(crate::roles::java),
            &[],
        );
    }

    /// The Checker's `is_doc_target` is the row; `holds_doc_comment_in_body`
    /// is the callables, the lambda, then the row — pinned per kind on the
    /// same fixture so the composition cannot drop a term.
    #[test]
    fn checker_composes_the_rows() {
        let source = b"class C {\n  C() { }\n  static { }\n  { }\n  void m() { int x = switch (1) { default -> { yield 1; } }; Runnable r = () -> { }; }\n}\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<JavaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let target = JavaCode::is_doc_target(node, source, ancestors);
            let holds = JavaCode::holds_doc_comment_in_body(node, ancestors, node);
            if target || holds {
                seen.push((node.kind(), target, holds));
            }
        });
        assert_eq!(
            seen,
            [
                ("class_declaration", true, false),
                ("constructor_declaration", true, true),
                ("static_initializer", false, true),
                ("block", false, true),
                ("block", false, true),
                ("method_declaration", true, true),
                ("block", false, true),
                ("switch_block", false, true),
                ("block", false, true),
                ("lambda_expression", false, true),
                ("block", false, true),
            ]
        );
    }
}

#[cfg(feature = "groovy")]
mod groovy {
    use super::*;
    use crate::GroovyCode;

    /// A Groovydoc: on the `DocComment` row only, never on `Comment`,
    /// and a comment to the Checker through the union.
    const DOC: Answers = at(&[DOC_COMMENT, IS_COMMENT, IS_DOC_COMMENT]);
    /// The empty `/**/`: the grammar's `groovydoc_comment` kind, which
    /// sits only on the `DocComment` row and fails its gate — so the
    /// row answers nothing, and `is_comment` answers through the union.
    /// This is the partition the union exists for.
    const EMPTY_GROOVYDOC: Answers = at(&[IS_COMMENT]);
    const BODY: Answers = at(&[HOLDS_IN_BODY]);

    #[test]
    fn rows_answer_the_table() {
        let source = b"/** c */\nclass C {\n  /** f */\n  int f\n  /** ctor */\n  C() { }\n  /**/\n  // line\n  /* block */\n  /*** three */\n  static { /** in static */ int s = 1 }\n  /** m */\n  def m() { def c = { /** in closure */ 1 } }\n}\ntrait T { }\ninterface I { }\nenum E { /** k */ K }\nrecord R(int a) { }\n@interface An { }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("groovydoc_comment", "/** c */", DOC),
            ("class_declaration", "class C {", TARGET),
            ("groovydoc_comment", "/** f */", DOC),
            ("field_declaration", "int f", TARGET),
            ("groovydoc_comment", "/** ctor */", DOC),
            ("constructor_declaration", "C() { }", TARGET),
            ("groovydoc_comment", "/**/", EMPTY_GROOVYDOC),
            ("line_comment", "// line", PLAIN),
            ("block_comment", "/* block */", PLAIN),
            ("groovydoc_comment", "/*** three */", DOC),
            (
                "static_initializer",
                "static { /** in static */ int s = 1 }",
                BODY,
            ),
            ("groovydoc_comment", "/** in static */", DOC),
            ("groovydoc_comment", "/** m */", DOC),
            (
                "method_declaration",
                "def m() { def c = { /** in closure */ 1 } }",
                TARGET,
            ),
            ("groovydoc_comment", "/** in closure */", DOC),
            ("trait_declaration", "trait T { }", TARGET),
            ("interface_declaration", "interface I { }", TARGET),
            ("enum_declaration", "enum E { /** k */ K }", TARGET),
            ("groovydoc_comment", "/** k */", DOC),
            ("enum_constant", "K", TARGET),
            ("record_declaration", "record R(int a) { }", TARGET),
            ("annotation_type_declaration", "@interface An { }", TARGET),
        ];
        check::<GroovyCode>(
            source,
            answers!(crate::roles::groovy, GroovyCode),
            expected,
            &declared!(crate::roles::groovy),
            &[],
        );
    }

    /// `is_doc_comment ⇒ is_comment` is the union: dropping the
    /// `DocComment` half from `is_comment` turns the `/**/` row above
    /// and every Groovydoc into a non-comment, which the table catches;
    /// this pins the implication directly on every comment node.
    #[test]
    fn doc_comment_implies_comment_through_the_union() {
        let source = b"/** d */\n/**/\n// l\nclass C { }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<GroovyCode>(source, |node, _| {
            if node.kind().ends_with("comment") {
                seen.push((
                    node.kind(),
                    GroovyCode::is_comment(node),
                    GroovyCode::is_doc_comment(node, source),
                    crate::roles::groovy::comment::lists_kind(node.kind_id()),
                ));
            }
        });
        assert_eq!(
            seen,
            [
                ("groovydoc_comment", true, true, false),
                ("groovydoc_comment", true, false, false),
                ("line_comment", true, false, true),
            ]
        );
    }
}

#[cfg(feature = "kotlin")]
mod kotlin {
    use super::*;
    use crate::{Kotlin, KotlinCode};

    const BODY: Answers = at(&[HOLDS_IN_BODY]);

    // Every target kind, the three `HoldsDocInBody` kinds each holding a
    // KDoc (a `val` must be `var` for a setter; the fixture keeps `val`
    // and the grammar still parses the accessor), a KDoc in a lambda (a `Closure` owner, not a row entry), and
    // the marker's edges: `/**/` is no KDoc; a line comment never is.
    #[test]
    fn rows_answer_the_table() {
        let source = b"/** c */\nclass C(/** p */ val p: Int, q: Int) {\n    /** prop */\n    val a = 1\n        /** getter */\n        get() { /** in getter */ return field }\n        set(v) { /** in setter */ field = v }\n    init { /** in init */ }\n    /** ctor */\n    constructor() : this(1, 2)\n    /**/\n    // line\n    /* block */\n    /** f */\n    fun f(/** param */ x: Int) { val l = { /** in lambda */ 1 } }\n    companion object { }\n}\nobject O { }\ntypealias A = Int\nenum class E { /** k */ K }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("block_comment", "/** c */", DOC),
            (
                "class_declaration",
                "class C(/** p */ val p: Int, q: Int) {",
                TARGET,
            ),
            ("block_comment", "/** p */", DOC),
            ("class_parameter", "val p: Int", TARGET),
            ("class_parameter", "q: Int", TARGET),
            ("block_comment", "/** prop */", DOC),
            ("property_declaration", "val a = 1", TARGET),
            ("block_comment", "/** getter */", DOC),
            ("getter", "get() { /** in getter */ return field }", BODY),
            ("block_comment", "/** in getter */", DOC),
            ("setter", "set(v) { /** in setter */ field = v }", BODY),
            ("block_comment", "/** in setter */", DOC),
            ("anonymous_initializer", "init { /** in init */ }", BODY),
            ("block_comment", "/** in init */", DOC),
            ("block_comment", "/** ctor */", DOC),
            (
                "secondary_constructor",
                "constructor() : this(1, 2)",
                TARGET,
            ),
            ("block_comment", "/**/", PLAIN),
            ("line_comment", "// line", PLAIN),
            ("block_comment", "/* block */", PLAIN),
            ("block_comment", "/** f */", DOC),
            (
                "function_declaration",
                "fun f(/** param */ x: Int) { val l = { /** in lambda */ 1 } }",
                TARGET,
            ),
            ("block_comment", "/** param */", DOC),
            ("parameter", "x: Int", TARGET),
            (
                "property_declaration",
                "val l = { /** in lambda */ 1 }",
                TARGET,
            ),
            ("block_comment", "/** in lambda */", DOC),
            ("companion_object", "companion object { }", TARGET),
            ("object_declaration", "object O { }", TARGET),
            ("type_alias", "typealias A = Int", TARGET),
            ("class_declaration", "enum class E { /** k */ K }", TARGET),
            ("block_comment", "/** k */", DOC),
            ("enum_entry", "K", TARGET),
        ];
        check::<KotlinCode>(
            source,
            answers!(crate::roles::kotlin, KotlinCode),
            expected,
            &declared!(crate::roles::kotlin),
            &[],
        );
    }

    /// The Checker's body test is the callables, the closures and the
    /// row, *then* the body-child test — except the lambda, all body.
    /// Pinned on `(ancestor, below)` pairs so neither the row read nor
    /// the `below` refinement can be dropped.
    #[test]
    fn checker_composes_the_rows_then_the_body_child() {
        let source = b"class C {\n    val a = 1\n        get() { return field }\n    init { }\n    fun f() { val l = { 1 } }\n}\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<KotlinCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind: Kotlin = node.kind_id().into();
            if !matches!(
                kind,
                Kotlin::Getter
                    | Kotlin::AnonymousInitializer
                    | Kotlin::FunctionDeclaration
                    | Kotlin::LambdaLiteral
                    | Kotlin::PropertyDeclaration
            ) {
                return;
            }
            let body = node
                .children()
                .find(|c| matches!(c.kind_id().into(), Kotlin::FunctionBody | Kotlin::Block));
            let not_body = node.child(0).expect("every listed node has a child");
            seen.push((
                node.kind(),
                body.is_some_and(|b| KotlinCode::holds_doc_comment_in_body(node, ancestors, &b)),
                KotlinCode::holds_doc_comment_in_body(node, ancestors, &not_body),
            ));
        });
        assert_eq!(
            seen,
            [
                ("property_declaration", false, false),
                ("getter", true, false),
                ("anonymous_initializer", true, false),
                ("function_declaration", true, false),
                ("property_declaration", false, false),
                ("lambda_literal", false, true),
            ]
        );
    }
}

#[cfg(feature = "csharp")]
mod csharp {
    use super::*;
    use crate::{Csharp, CsharpCode};

    /// A property or indexer: a target, and a `HoldsDocInBody` owner in
    /// every form (bodied, expression-bodied, with an accessor list).
    const TARGET_BODY: Answers = at(&[DOC_TARGET, HOLDS_IN_BODY]);

    // Every target kind, the marker's edges (`////` and `/**/` are no
    // doc, `/** … */` is), and a doc comment in an accessor list and in
    // an expression body — the `below` cases the Checker tells apart.
    #[test]
    fn rows_answer_the_table() {
        let source = b"/// <summary>c</summary>\nclass C {\n    /// f\n    int f;\n    /** block doc */\n    C() { }\n    ~C() { }\n    /**/\n    //// banner\n    // line\n    /* block */\n    int P { /// in accessors\n        get; set; }\n    int Q => /// in body\n        1;\n    int this[int i] { get => i; }\n    event System.Action E;\n    event System.Action F { add { } remove { } }\n    void M() { }\n    static C operator +(C a, C b) => a;\n    static explicit operator int(C a) => 1;\n}\nstruct S { }\nrecord R(int A);\ninterface I { }\nenum E { /// k\n    K }\ndelegate void D();\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "/// <summary>c</summary>", DOC),
            ("class_declaration", "class C {", TARGET),
            ("comment", "/// f", DOC),
            ("field_declaration", "int f;", TARGET),
            ("comment", "/** block doc */", DOC),
            ("constructor_declaration", "C() { }", TARGET),
            ("destructor_declaration", "~C() { }", TARGET),
            ("comment", "/**/", PLAIN),
            ("comment", "//// banner", PLAIN),
            ("comment", "// line", PLAIN),
            ("comment", "/* block */", PLAIN),
            (
                "property_declaration",
                "int P { /// in accessors",
                TARGET_BODY,
            ),
            ("comment", "/// in accessors", DOC),
            ("property_declaration", "int Q => /// in body", TARGET_BODY),
            ("comment", "/// in body", DOC),
            (
                "indexer_declaration",
                "int this[int i] { get => i; }",
                TARGET_BODY,
            ),
            ("event_field_declaration", "event System.Action E;", TARGET),
            (
                "event_declaration",
                "event System.Action F { add { } remove { } }",
                TARGET,
            ),
            ("method_declaration", "void M() { }", TARGET),
            (
                "operator_declaration",
                "static C operator +(C a, C b) => a;",
                TARGET,
            ),
            (
                "conversion_operator_declaration",
                "static explicit operator int(C a) => 1;",
                TARGET,
            ),
            ("struct_declaration", "struct S { }", TARGET),
            ("record_declaration", "record R(int A);", TARGET),
            ("interface_declaration", "interface I { }", TARGET),
            ("enum_declaration", "enum E { /// k", TARGET),
            ("comment", "/// k", DOC),
            ("enum_member_declaration", "K", TARGET),
            ("delegate_declaration", "delegate void D();", TARGET),
        ];
        check::<CsharpCode>(
            source,
            answers!(crate::roles::csharp, CsharpCode),
            expected,
            &declared!(crate::roles::csharp),
            &[],
        );
    }

    /// The Checker reads the row first: a property holds a comment in
    /// its body unless `below` is the accessor list, in both the bodied
    /// and the expression-bodied form. That the callable test is never
    /// consulted for one (#1612) is a *cost* fact no verdict can see —
    /// swapping the two branches leaves every answer here unchanged —
    /// so the order is pinned by the accessor-search counter in the test
    /// after this one, not by these answers.
    #[test]
    fn property_and_indexer_hold_doc_except_under_the_accessor_list() {
        let source =
            b"class C { int P { get; } int Q => 1; int this[int i] { get => i; } void M() { } }\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<CsharpCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind: Csharp = node.kind_id().into();
            if !matches!(
                kind,
                Csharp::PropertyDeclaration
                    | Csharp::IndexerDeclaration
                    | Csharp::MethodDeclaration
            ) {
                return;
            }
            let accessors = node
                .children()
                .find(|c| c.kind_id() == Csharp::AccessorList as u16);
            let first = node.child(0).expect("a member has children");
            seen.push((
                node.kind(),
                accessors
                    .is_some_and(|a| CsharpCode::holds_doc_comment_in_body(node, ancestors, &a)),
                CsharpCode::holds_doc_comment_in_body(node, ancestors, &first),
            ));
        });
        assert_eq!(
            seen,
            [
                ("property_declaration", false, true),
                ("property_declaration", false, true),
                ("indexer_declaration", false, true),
                ("method_declaration", false, true),
            ]
        );
    }

    /// The cost pin for the order above: asking the body test of a
    /// property or indexer runs **no** accessor search, because the row
    /// answers before `is_func` (whose `has_no_accessors` gate would
    /// scan the accessor list) is reached. Swapping the branches makes
    /// the count one per member — a cost no verdict and no scaling probe
    /// reaches, which is why it is counted here.
    #[test]
    fn csharp_body_test_reads_the_row_before_the_accessor_search() {
        let source = b"class C { int P { get; set; } int Q => 1; int this[int i] { get => i; } }
";
        let mut members = 0;
        let before = crate::checker::csharp_accessor_searches::observed();
        for_each_node_with_chain::<CsharpCode>(source, |node, chain| {
            let kind: Csharp = node.kind_id().into();
            if matches!(
                kind,
                Csharp::PropertyDeclaration | Csharp::IndexerDeclaration
            ) {
                let first = node.child(0).expect("a member has children");
                assert!(CsharpCode::holds_doc_comment_in_body(
                    node,
                    Ancestors::known(chain),
                    &first
                ));
                members += 1;
            }
        });
        assert_eq!(members, 3);
        assert_eq!(
            crate::checker::csharp_accessor_searches::observed() - before,
            0,
            "the row is read before the callable test"
        );
    }
}

#[cfg(feature = "php")]
mod php {
    use super::*;
    use crate::{Php, PhpCode};

    // Every target kind and the marker's edges: `/**/` and `/**no
    // space*/` are no DocBlock (PHP's scanner wants whitespace after
    // the marker); `#` and `//` and `/* */` never are. `HoldsDocInBody`
    // is absent, so a DocBlock in a closure or arrow body answers for
    // no body kind here — the Checker's body-child test is pinned
    // separately below.
    #[test]
    fn rows_answer_the_table() {
        let source = b"<?php\n/** file */\n/** c */\nclass C {\n    /** p */\n    public $p;\n    /** k */\n    const K = 1;\n    /**/\n    /**no space*/\n    // line\n    # hash\n    /* block */\n    /** m */\n    function m() { $f = function () { /** in closure */ }; $g = fn() => /** in arrow */ 1; }\n}\ninterface I { const J = 2; }\ntrait T { }\nenum E { /** case */ case A; }\n/** f */\nfunction f() { }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "/** file */", DOC),
            ("comment", "/** c */", DOC),
            ("class_declaration", "class C {", TARGET),
            ("comment", "/** p */", DOC),
            ("property_declaration", "public $p;", TARGET),
            ("comment", "/** k */", DOC),
            ("const_declaration", "const K = 1;", TARGET),
            ("comment", "/**/", PLAIN),
            ("comment", "/**no space*/", PLAIN),
            ("comment", "// line", PLAIN),
            ("comment", "# hash", PLAIN),
            ("comment", "/* block */", PLAIN),
            ("comment", "/** m */", DOC),
            (
                "method_declaration",
                "function m() { $f = function () { /** in closure */ }; $g = fn() => /** in arrow */ 1; }",
                TARGET,
            ),
            ("comment", "/** in closure */", DOC),
            ("comment", "/** in arrow */", DOC),
            (
                "interface_declaration",
                "interface I { const J = 2; }",
                TARGET,
            ),
            ("const_declaration", "const J = 2;", TARGET),
            ("trait_declaration", "trait T { }", TARGET),
            ("enum_declaration", "enum E { /** case */ case A; }", TARGET),
            ("comment", "/** case */", DOC),
            ("enum_case", "case A;", TARGET),
            ("comment", "/** f */", DOC),
            ("function_definition", "function f() { }", TARGET),
        ];
        check::<PhpCode>(
            source,
            answers!(crate::roles::php, PhpCode),
            expected,
            &declared!(crate::roles::php),
            // One `const_declaration` id for a class's, an interface's and
            // an enum's constant at the pin; the alias is defensive.
            &[Php::ConstDeclaration2 as u16],
        );
    }

    /// With no `HoldsDocInBody` row, the Checker's body test is the
    /// body-child test alone: a function holds a comment under its
    /// `compound_statement`, an arrow under its last child, and neither
    /// under a parameter list.
    #[test]
    fn body_child_test_stays_in_the_checker() {
        let source = b"<?php\nfunction f($a) { }\n$g = fn($b) => 1;\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<PhpCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind: Php = node.kind_id().into();
            if !matches!(kind, Php::FunctionDefinition | Php::ArrowFunction) {
                return;
            }
            let params = node
                .children()
                .find(|c| c.kind_id() == Php::FormalParameters as u16)
                .expect("a parameter list");
            let last = node.children().last().expect("a body");
            seen.push((
                node.kind(),
                PhpCode::holds_doc_comment_in_body(node, ancestors, &params),
                PhpCode::holds_doc_comment_in_body(node, ancestors, &last),
            ));
        });
        assert_eq!(
            seen,
            [
                ("function_definition", false, true),
                ("arrow_function", false, true)
            ]
        );
        assert_eq!(crate::roles::php::holds_doc_in_body::OWNERS, &[] as &[u16]);
    }
}

#[cfg(feature = "rust")]
mod rust {
    use super::*;
    use crate::{Rust, RustCode};

    // Every target kind; every doc form (`///`, `//!`, `/** */`,
    // `/*! */`) a `DocComment`, and `////`, `/**/`, `//`, `/* */` not —
    // decided by the `doc_comment` child, never the bytes. The tuple
    // field's slot (`u8` under `ordered_field_declaration_list`) is no
    // row's: the Checker's clause, pinned below.
    #[test]
    fn rows_answer_the_table() {
        let source = b"//! inner\n/// m\nmod m { }\n/// s\nstruct S { /// f\n    f: u8 }\n/// tuple\nstruct T(/// slot\n    u8, #[doc = \"x\"] u16);\n/// u\nunion U { a: u8 }\n/// e\nenum E { /// v\n    V }\n/// c\nconst C: u8 = 1;\n/// st\nstatic ST: u8 = 1;\n/// ty\ntype Ty = u8;\n/// f\nfn f() { //! inner in body\n    /// on a stmt\n    let x = 1; let c = || { /// in closure\n    1 }; }\n//// banner\n/**/\n// line\n/* block */\n/** block doc */\n/*! inner block */\ntrait Tr { /// sig\n    fn g(); /// assoc\n    type A; }\nimpl S { }\nuse std::fmt;\nextern crate core;\nmacro_rules! mk { () => {} }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("line_comment", "//! inner", DOC),
            ("line_comment", "/// m", DOC),
            ("mod_item", "mod m { }", TARGET),
            ("line_comment", "/// s", DOC),
            ("struct_item", "struct S { /// f", TARGET),
            ("line_comment", "/// f", DOC),
            ("field_declaration", "f: u8", TARGET),
            ("line_comment", "/// tuple", DOC),
            ("struct_item", "struct T(/// slot", TARGET),
            ("line_comment", "/// slot", DOC),
            ("line_comment", "/// u", DOC),
            ("union_item", "union U { a: u8 }", TARGET),
            ("field_declaration", "a: u8", TARGET),
            ("line_comment", "/// e", DOC),
            ("enum_item", "enum E { /// v", TARGET),
            ("line_comment", "/// v", DOC),
            ("enum_variant", "V", TARGET),
            ("line_comment", "/// c", DOC),
            ("const_item", "const C: u8 = 1;", TARGET),
            ("line_comment", "/// st", DOC),
            ("static_item", "static ST: u8 = 1;", TARGET),
            ("line_comment", "/// ty", DOC),
            ("type_item", "type Ty = u8;", TARGET),
            ("line_comment", "/// f", DOC),
            ("function_item", "fn f() { //! inner in body", TARGET),
            ("line_comment", "//! inner in body", DOC),
            ("line_comment", "/// on a stmt", DOC),
            ("line_comment", "/// in closure", DOC),
            ("line_comment", "//// banner", PLAIN),
            ("block_comment", "/**/", PLAIN),
            ("line_comment", "// line", PLAIN),
            ("block_comment", "/* block */", PLAIN),
            ("block_comment", "/** block doc */", DOC),
            ("block_comment", "/*! inner block */", DOC),
            ("trait_item", "trait Tr { /// sig", TARGET),
            ("line_comment", "/// sig", DOC),
            ("function_signature_item", "fn g();", TARGET),
            ("line_comment", "/// assoc", DOC),
            ("associated_type", "type A;", TARGET),
            ("impl_item", "impl S { }", TARGET),
            ("use_declaration", "use std::fmt;", TARGET),
            ("extern_crate_declaration", "extern crate core;", TARGET),
            ("macro_definition", "macro_rules! mk { () => {} }", TARGET),
        ];
        check::<RustCode>(
            source,
            answers!(crate::roles::rust, RustCode),
            expected,
            &declared!(crate::roles::rust),
            &[],
        );
    }

    /// The tuple-field slot is the Checker's clause over the parent, not
    /// a row: under `ordered_field_declaration_list` every named
    /// non-comment node is a target (`u8`, the attribute, `u16`), the
    /// row says `false` for each, and the comment there is a doc
    /// comment and not a target.
    #[test]
    fn tuple_field_slot_is_the_checkers_clause_over_the_row() {
        let source = b"struct T(/// slot\n    u8, #[doc = \"x\"] u16);\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<RustCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if ancestors.parent_has_kind(node, Rust::OrderedFieldDeclarationList as u16)
                && node.is_named()
            {
                seen.push((
                    node.kind(),
                    RustCode::is_doc_target(node, source, ancestors),
                    crate::roles::rust::doc_target::owns(node, source, ancestors),
                ));
            }
        });
        // The row reads structure, not bytes: `TEXT_GATED` beside the
        // walk's answers so the pin is not a constant on its own.
        assert_eq!(
            (seen, crate::roles::rust::doc_comment::TEXT_GATED),
            (
                vec![
                    ("line_comment", false, false),
                    ("primitive_type", true, false),
                    ("attribute_item", true, false),
                    ("primitive_type", true, false),
                ],
                false
            )
        );
    }
}

#[cfg(feature = "perl")]
mod perl {
    use super::*;
    use crate::PerlCode;

    /// A POD block: on the `DocComment` row only, a comment through the
    /// union.
    const POD: Answers = at(&[DOC_COMMENT, IS_COMMENT, IS_DOC_COMMENT]);
    /// A `=begin comment` region: the `pod_statement` kind failing its
    /// gate, so no row answers and only the union makes it a comment.
    const POD_COMMENT_REGION: Answers = at(&[IS_COMMENT]);

    // The partition: `#` comments on `Comment`, POD on `DocComment`
    // under its gate; a POD block in a sub body is documentation
    // (perldoc reads it there); a `=begin comment` region is a comment
    // and not documentation.
    #[test]
    fn rows_answer_the_table() {
        let source = b"# plain\n\n=head1 NAME\n\nDoc.\n\n=cut\n\nsub f {\n    # in a body\n\n=pod\n\nIn a sub.\n\n=cut\n\n    return 1;\n}\n\n=begin comment\n\ncommented out\n\n=end comment\n\n=cut\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comments", "# plain", PLAIN),
            ("pod_statement", "=head1 NAME", POD),
            ("comments", "# in a body", PLAIN),
            ("pod_statement", "=pod", POD),
            ("pod_statement", "=begin comment", POD_COMMENT_REGION),
        ];
        check::<PerlCode>(
            source,
            answers!(crate::roles::perl, PerlCode),
            expected,
            &declared!(crate::roles::perl),
            &[],
        );
    }
}

// The positional languages: no marker. A comment documents by where
// its run sits, decided per run (`leading_doc_run`) from `DocTarget`, a
// `DocRunOpener` or a `DocRunDirective`; `DocComment` is absent, so
// `is_doc_comment` answers `false` on every comment.

#[cfg(feature = "go")]
mod go {
    use super::*;
    use crate::GoCode;

    /// A go/doc directive: a comment, and a run member that is not text.
    const DIRECTIVE: Answers = at(&[COMMENT, RUN_DIRECTIVE, IS_COMMENT, IS_RUN_DIRECTIVE]);

    // Every target kind at the top level, and every gated kind — the
    // ten under `outside_function_body`, the interface element under
    // its own gate — inside a function body too, each gated out; an
    // interface's method and name-led embedded element against a `~int`
    // constraint and against the `type_elem`s an instantiation's
    // arguments parse as, outside any interface; and a `//go:` directive
    // beside a plain comment.
    #[test]
    fn rows_answer_the_table() {
        let source = b"// Package p.\npackage p\n\n//go:generate stringer\n// plain\nimport \"fmt\"\n\n// T doc\ntype T struct {\n\t// F doc\n\tF int\n}\n\ntype (\n\t// A doc\n\tA int\n\t// B alias\n\tB = int\n)\n\nvar (\n\t// V doc\n\tV = 1\n)\n\nconst (\n\t// K doc\n\tK = 2\n)\n\ntype G[K comparable, V any] struct{}\nvar gm G[string, int]\n\n// I doc\ntype I interface {\n\t// M doc\n\tM()\n\t// embedded\n\tfmt.Stringer\n\t~int\n}\n\n// F doc\nfunc F() {\n\t// local type: not a target\n\ttype L struct {\n\t\t// LF: not a target\n\t\tLF int\n\t}\n\tvar w = 3\n\tconst k = 1\n\ttype LA = int\n\ttype LI interface {\n\t\tLM()\n\t\tfmt.Stringer\n\t}\n\tf := func() { }\n\t_ = w\n\t_ = f\n}\n\n// Recv doc\nfunc (t T) Recv() { }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "// Package p.", PLAIN),
            ("package_clause", "package p", TARGET),
            ("comment", "//go:generate stringer", DIRECTIVE),
            ("comment", "// plain", PLAIN),
            ("comment", "// T doc", PLAIN),
            ("type_declaration", "type T struct {", TARGET),
            ("type_spec", "T struct {", TARGET),
            ("comment", "// F doc", PLAIN),
            ("field_declaration", "F int", TARGET),
            ("type_declaration", "type (", TARGET),
            ("comment", "// A doc", PLAIN),
            ("type_spec", "A int", TARGET),
            ("comment", "// B alias", PLAIN),
            ("type_alias", "B = int", TARGET),
            ("var_declaration", "var (", TARGET),
            ("comment", "// V doc", PLAIN),
            ("var_spec", "V = 1", TARGET),
            ("const_declaration", "const (", TARGET),
            ("comment", "// K doc", PLAIN),
            ("const_spec", "K = 2", TARGET),
            (
                "type_declaration",
                "type G[K comparable, V any] struct{}",
                TARGET,
            ),
            ("type_spec", "G[K comparable, V any] struct{}", TARGET),
            ("var_declaration", "var gm G[string, int]", TARGET),
            ("var_spec", "gm G[string, int]", TARGET),
            ("type_elem", "string", NONE),
            ("type_elem", "int", NONE),
            ("comment", "// I doc", PLAIN),
            ("type_declaration", "type I interface {", TARGET),
            ("type_spec", "I interface {", TARGET),
            ("comment", "// M doc", PLAIN),
            ("method_elem", "M()", TARGET),
            ("comment", "// embedded", PLAIN),
            ("type_elem", "fmt.Stringer", TARGET),
            ("type_elem", "~int", NONE),
            ("comment", "// F doc", PLAIN),
            ("function_declaration", "func F() {", TARGET),
            ("comment", "// local type: not a target", PLAIN),
            ("type_declaration", "type L struct {", NONE),
            ("type_spec", "L struct {", NONE),
            ("comment", "// LF: not a target", PLAIN),
            ("field_declaration", "LF int", NONE),
            ("var_declaration", "var w = 3", NONE),
            ("var_spec", "w = 3", NONE),
            ("const_declaration", "const k = 1", NONE),
            ("const_spec", "k = 1", NONE),
            ("type_declaration", "type LA = int", NONE),
            ("type_alias", "LA = int", NONE),
            ("type_declaration", "type LI interface {", NONE),
            ("type_spec", "LI interface {", NONE),
            ("method_elem", "LM()", NONE),
            ("type_elem", "fmt.Stringer", NONE),
            ("comment", "// Recv doc", PLAIN),
            ("method_declaration", "func (t T) Recv() { }", TARGET),
        ];
        check::<GoCode>(
            source,
            answers!(crate::roles::go, GoCode),
            expected,
            &declared!(crate::roles::go),
            &[],
        );
    }
}

#[cfg(feature = "lua")]
mod lua {
    use super::*;
    use crate::{Lua, LuaCode};

    /// A `---` comment where LDoc reads one: the opener candidate.
    const OPENER: Answers = at(&[COMMENT, RUN_OPENER, IS_COMMENT]);

    // Every item kind with its gate's two sides (one name / two names,
    // dotted / indexed, named field / positional, a returned table / a
    // returned name), a `---` in a function body (gated out), and a long
    // comment. A `---` directly under a `--` row continues that group
    // and is no opener — the Checker test below spells that.
    #[test]
    fn rows_answer_the_table() {
        let source = b"--- Module doc.\n-- continues\nlocal M = {}\n\n-- plain\n\n--- f doc\nfunction M.f() end\n\n--- local f\nlocal function g() end\n\n--- one name\nM.x = 1\n\n--- local one name\nlocal y = 2\n\n--- two names: no item\nlocal a, b = 1, 2\n\n--- index: no item\nM[1] = 3\n\n--- @tag only\n--- in a body\nlocal function h()\n  --- in body: not read\n  local z = 1\n  return z\nend\n\nlocal t = {\n  --- field\n  new = function() end,\n  --- positional: no item\n  4,\n}\n\n--[[ long ]]\n--- returns a table\nreturn { M }\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "--- Module doc.", OPENER),
            ("comment", "-- continues", PLAIN),
            ("variable_declaration", "local M = {}", TARGET),
            ("assignment_statement", "M = {}", TARGET),
            ("comment", "-- plain", PLAIN),
            ("comment", "--- f doc", OPENER),
            ("function_declaration", "function M.f() end", TARGET),
            ("comment", "--- local f", OPENER),
            ("function_declaration", "local function g() end", TARGET),
            ("comment", "--- one name", OPENER),
            ("assignment_statement", "M.x = 1", TARGET),
            ("comment", "--- local one name", OPENER),
            ("variable_declaration", "local y = 2", TARGET),
            ("assignment_statement", "y = 2", TARGET),
            ("comment", "--- two names: no item", OPENER),
            ("variable_declaration", "local a, b = 1, 2", NONE),
            ("assignment_statement", "a, b = 1, 2", NONE),
            ("comment", "--- index: no item", OPENER),
            ("assignment_statement", "M[1] = 3", NONE),
            ("comment", "--- @tag only", OPENER),
            ("comment", "--- in a body", OPENER),
            ("function_declaration", "local function h()", TARGET),
            ("comment", "--- in body: not read", PLAIN),
            ("variable_declaration", "local z = 1", TARGET),
            ("assignment_statement", "z = 1", TARGET),
            ("return_statement", "return z", NONE),
            ("variable_declaration", "local t = {", TARGET),
            ("assignment_statement", "t = {", TARGET),
            ("comment", "--- field", OPENER),
            ("field", "new = function() end", TARGET),
            ("comment", "--- positional: no item", OPENER),
            ("field", "4", NONE),
            ("comment", "--[[ long ]]", PLAIN),
            ("comment", "--- returns a table", OPENER),
            ("return_statement", "return { M }", TARGET),
            ("field", "M", NONE),
        ];
        check::<LuaCode>(
            source,
            answers!(crate::roles::lua, LuaCode),
            expected,
            &declared!(crate::roles::lua),
            // One `function_declaration` id for every spelling at the pin
            // (`function M.f`, `function M:m`, `local function g`), and
            // one `assignment_statement` / `variable_declaration` id for a
            // plain, an attributed (`<const>`) and a multi-target
            // assignment alike; the aliases are defensive, as the
            // space-tree test and the root consumers keep them.
            &[
                Lua::FunctionDeclaration2 as u16,
                Lua::FunctionDeclaration3 as u16,
                Lua::AssignmentStatement2 as u16,
                Lua::VariableDeclaration2 as u16,
            ],
        );
    }

    /// `Checker::is_doc_target` is the trait default — `false` on every
    /// node — while the row owns the items: the row is read by the
    /// opener's content decision only, since LDoc attaches nothing to a
    /// declaration a plain `--` run precedes. And the Checker's opener
    /// is the row *and* the content: a `---` above no item with no tag,
    /// and one with no text, are candidates that do not document; a
    /// `---` under a trailing `-- note` continues that `--` group and is
    /// no candidate at all. The file opens with code, so no block is the
    /// file's first.
    #[test]
    fn target_row_is_read_by_the_opener_not_the_checker() {
        let source = b"local a = 1\n--- documents\nlocal function f() end\n--- no item below\nlocal b, c = 1, 2\n--- @tag\nlocal d, e = 1, 2\n---\nlocal g = 1\nx = 1 -- note\n--- continues the group\nlocal function h() end\n";
        let mut targets = Vec::new();
        let mut openers = Vec::new();
        for_each_node_with_chain::<LuaCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            if crate::roles::lua::doc_target::owns(node, source, ancestors) {
                targets.push((node.kind(), LuaCode::is_doc_target(node, source, ancestors)));
            }
            if node.kind_id() == Lua::Comment as u16 {
                let next = ancestors.siblings_after(node).next();
                openers.push((
                    crate::roles::lua::doc_run_opener::owns(node, source, ancestors),
                    LuaCode::is_doc_run_opener(node, next.as_ref(), source, ancestors),
                ));
            }
        });
        assert_eq!(
            targets,
            [
                ("variable_declaration", false),
                ("assignment_statement", false),
                ("function_declaration", false),
                ("variable_declaration", false),
                ("assignment_statement", false),
                ("assignment_statement", false),
                ("function_declaration", false),
            ]
        );
        assert_eq!(
            openers,
            [
                (true, true),
                (true, false),
                (true, true),
                (true, false),
                (false, false),
                (false, false),
            ]
        );
    }
}

#[cfg(feature = "ruby")]
mod ruby {
    use super::*;
    use crate::{Ruby, RubyCode};

    const DIRECTIVE: Answers = at(&[COMMENT, RUN_DIRECTIVE, IS_COMMENT, IS_RUN_DIRECTIVE]);
    const OPENER: Answers = at(&[COMMENT, RUN_OPENER, IS_COMMENT]);

    // Every candidate kind with its gate's two sides: a constant, scoped
    // and class-variable assignment against an instance-variable and a
    // local one; a method alias against a global one; `class << C`
    // against `class << self`; `private def` against `self.private def`;
    // the `attr` and `alias_method` calls against `define_method`. The
    // shebang and the magic comment are directives; the `##` block is an
    // opener in the class body and not in a `def` body, where YARD does
    // not read one. The row knows no scope: a `def` in a
    // `def` body or a `case` arm is a candidate here, and the Checker
    // test below is where the scope says no.
    #[test]
    fn rows_answer_the_table() {
        let source = b"#!/usr/bin/env ruby\n# frozen_string_literal: true\n# plain\n# C doc\nclass C\n  # K doc\n  K = 1\n  # scoped\n  C::L = 2\n  # cvar\n  @@count = 0\n  # ivar: no\n  @x = 1\n  # local: no\n  y = 2\n  # m doc\n  def m\n    # in a def: not read\n    ##\n    # directive in a def: not read\n    z = 1\n    z\n  end\n  # smeth\n  def self.s; end\n  # attr\n  attr_reader :a\n  # alias_method\n  alias_method :n, :m\n  # alias stmt\n  alias o m\n  # global alias: no\n  alias $new $old\n  # singleton of const\n  class << self; end\n  class << C; end\n  # private def\n  private def p; end\n  # receiver: no\n  self.private def q; end\n  ##\n  # directive block\n  define_method(:dm) { }\n  # modifier\n  def r; end if true\n  case 1\n  when 1\n    # in a case: not read\n    def t; end\n  end\nend\n# M doc\nmodule M; end\n# then wrapper\nif true then\n  # first in then\n  def u; end\nend\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("comment", "#!/usr/bin/env ruby", DIRECTIVE),
            ("comment", "# frozen_string_literal: true", DIRECTIVE),
            ("comment", "# plain", PLAIN),
            ("comment", "# C doc", PLAIN),
            ("class", "class C", TARGET),
            ("comment", "# K doc", PLAIN),
            ("assignment", "K = 1", TARGET),
            ("comment", "# scoped", PLAIN),
            ("assignment", "C::L = 2", TARGET),
            ("comment", "# cvar", PLAIN),
            ("assignment", "@@count = 0", TARGET),
            ("comment", "# ivar: no", PLAIN),
            ("assignment", "@x = 1", NONE),
            ("comment", "# local: no", PLAIN),
            ("assignment", "y = 2", NONE),
            ("comment", "# m doc", PLAIN),
            ("method", "def m", TARGET),
            ("comment", "# in a def: not read", PLAIN),
            ("comment", "##", PLAIN),
            ("comment", "# directive in a def: not read", PLAIN),
            ("assignment", "z = 1", NONE),
            ("comment", "# smeth", PLAIN),
            ("singleton_method", "def self.s; end", TARGET),
            ("comment", "# attr", PLAIN),
            ("call", "attr_reader :a", TARGET),
            ("comment", "# alias_method", PLAIN),
            ("call", "alias_method :n, :m", TARGET),
            ("comment", "# alias stmt", PLAIN),
            ("alias", "alias o m", TARGET),
            ("comment", "# global alias: no", PLAIN),
            ("alias", "alias $new $old", NONE),
            ("comment", "# singleton of const", PLAIN),
            ("singleton_class", "class << self; end", NONE),
            ("singleton_class", "class << C; end", TARGET),
            ("comment", "# private def", PLAIN),
            ("call", "private def p; end", TARGET),
            ("method", "def p; end", TARGET),
            ("comment", "# receiver: no", PLAIN),
            ("call", "self.private def q; end", NONE),
            ("method", "def q; end", TARGET),
            ("comment", "##", OPENER),
            ("comment", "# directive block", PLAIN),
            ("call", "define_method(:dm) { }", NONE),
            ("comment", "# modifier", PLAIN),
            ("method", "def r; end", TARGET),
            ("comment", "# in a case: not read", PLAIN),
            ("method", "def t; end", TARGET),
            ("comment", "# M doc", PLAIN),
            ("module", "module M; end", TARGET),
            ("comment", "# then wrapper", PLAIN),
            ("comment", "# first in then", PLAIN),
            ("method", "def u; end", TARGET),
        ];
        check::<RubyCode>(
            source,
            answers!(crate::roles::ruby, RubyCode),
            expected,
            &declared!(crate::roles::ruby),
            // Every call spelling the pinned grammar emits carries `Call3`
            // (`call_string_tests.rs`); `Call`, `Call2` and `Call4` are the
            // other visible aliases and are defensive here too. Every
            // assignment spelling — plain, multiple, scoped — carries
            // `Assignment`; `Assignment2` is the grammar's aliased
            // production, kept defensively as the Checker did.
            &[
                Ruby::Call as u16,
                Ruby::Call2 as u16,
                Ruby::Call4 as u16,
                Ruby::Assignment2 as u16,
            ],
        );
    }

    /// The Checker composes the row with two pieces of context the row
    /// cannot see: the `then` / modifier / body wrappers stand for their
    /// first statement, and YARD's scope says where a candidate is read
    /// — a `def` in a `def` body is (YARD's `DEF_READS`), a `class` there
    /// is not, and nothing under a `case` is. Row and Checker pinned side
    /// by side.
    #[test]
    fn checker_composes_the_row_with_wrappers_and_scope() {
        let source = b"class C\n  def m\n    def n; end\n    class D; end\n  end\n  case 1\n  when 1\n    def t; end\n  end\n  def r; end if true\nend\nif true then\n  def u; end\nend\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<RubyCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind: Ruby = node.kind_id().into();
            if !matches!(
                kind,
                Ruby::Method | Ruby::Class | Ruby::IfModifier | Ruby::Then | Ruby::BodyStatement
            ) {
                return;
            }
            let row = crate::roles::ruby::doc_target::owns(node, source, ancestors);
            let checker = RubyCode::is_doc_target(node, source, ancestors);
            seen.push((node.kind(), row, checker));
        });
        assert_eq!(
            seen,
            [
                ("class", true, true),
                ("body_statement", false, true),
                ("method", true, true),
                ("body_statement", false, true),
                ("method", true, true),
                ("class", true, false),
                ("then", false, false),
                ("method", true, false),
                ("if_modifier", false, true),
                ("method", true, true),
                ("then", false, true),
                ("method", true, true),
            ]
        );
    }
}

// The JS family: the `Comment` row holds both the `comment` and the
// Annex-B `html_comment`, only the first a `DocComment` under the
// tool's block marker; `DocTarget` is what JSDoc (JavaScript, Mozjs) or
// tsc (TypeScript, TSX) documents. `HoldsDocInBody` is absent — the
// override is the body-child test alone — and a doc comment in a body
// stays documentation (`DOC_COMMENTS_IN_FUNCTION_BODIES`, #1597).

// Every JSDoc target with its gate's two sides: a declaration whose
// first declarator binds a name against a destructuring one; an
// export of a declaration against a bare `export { … }` list; an
// assignment to a name, a member and a parenthesised name against
// a destructuring assignment and a call. The marker's edges:
// `/***` is no JSDoc, `/**/` none, and the HTML comment is a comment
// that is never documentation.
// One fixture for the two JavaScript grammars: Mozilla's names its
// nodes as tree-sitter-javascript does and owns no file extension, so
// its table is pinned on the same source (grammar-dispatch, "a LANG
// variant that owns no file extension"). `using x = …` (TC39 explicit
// resource management) is a kind of its own, named as JSDoc names it
// and gated as a `const` is.
#[cfg(any(feature = "javascript", feature = "mozjs"))]
const JS_SOURCE: &[u8] = b"/** fn */\nfunction f() { /** in body */ const inner = 1; }\n/** gen */\nfunction* g() { }\n/** cls */\nclass C {\n  /** field */\n  x = 1;\n  /** method */\n  m() { }\n}\n/** var */\nvar v = 1;\n/** let destructure: no */\nlet { a } = o;\n/** const */\nconst k = 2;\n/** obj */\nconst o2 = {\n  /** pair */\n  p: 1,\n  /** shorthand */\n  q,\n};\n/** export decl */\nexport const e = 3;\n/** export destructure */\nexport const { ed } = o;\n/** export list: no */\nexport { e };\n/** assign */\nx = 1;\n/** member assign */\na.b += 1;\n/** paren assign */\n(y = 2);\n/** destructure assign: no */\n({ z } = o);\n/** call: no */\nf();\n/** using */\nusing u = f();\n/** using destructure: no */\nusing { ud } = o;\n/*** three */\n// line\n<!-- html -->\n/**/\n";

#[cfg(any(feature = "javascript", feature = "mozjs"))]
const JS_EXPECTED: &[(&str, &str, Answers)] = &[
    ("comment", "/** fn */", DOC),
    (
        "function_declaration",
        "function f() { /** in body */ const inner = 1; }",
        TARGET,
    ),
    ("comment", "/** in body */", DOC),
    ("lexical_declaration", "const inner = 1;", TARGET),
    ("comment", "/** gen */", DOC),
    (
        "generator_function_declaration",
        "function* g() { }",
        TARGET,
    ),
    ("comment", "/** cls */", DOC),
    ("class_declaration", "class C {", TARGET),
    ("comment", "/** field */", DOC),
    ("field_definition", "x = 1", TARGET),
    ("comment", "/** method */", DOC),
    ("method_definition", "m() { }", TARGET),
    ("comment", "/** var */", DOC),
    ("variable_declaration", "var v = 1;", TARGET),
    ("comment", "/** let destructure: no */", DOC),
    ("lexical_declaration", "let { a } = o;", NONE),
    ("comment", "/** const */", DOC),
    ("lexical_declaration", "const k = 2;", TARGET),
    ("comment", "/** obj */", DOC),
    ("lexical_declaration", "const o2 = {", TARGET),
    ("comment", "/** pair */", DOC),
    ("pair", "p: 1", TARGET),
    ("comment", "/** shorthand */", DOC),
    ("shorthand_property_identifier", "q", TARGET),
    ("comment", "/** export decl */", DOC),
    ("export_statement", "export const e = 3;", TARGET),
    ("lexical_declaration", "const e = 3;", TARGET),
    ("comment", "/** export destructure */", DOC),
    ("export_statement", "export const { ed } = o;", TARGET),
    ("lexical_declaration", "const { ed } = o;", NONE),
    ("comment", "/** export list: no */", DOC),
    ("export_statement", "export { e };", NONE),
    ("comment", "/** assign */", DOC),
    ("expression_statement", "x = 1;", TARGET),
    ("comment", "/** member assign */", DOC),
    ("expression_statement", "a.b += 1;", TARGET),
    ("comment", "/** paren assign */", DOC),
    ("expression_statement", "(y = 2);", TARGET),
    ("comment", "/** destructure assign: no */", DOC),
    ("expression_statement", "({ z } = o);", NONE),
    ("comment", "/** call: no */", DOC),
    ("expression_statement", "f();", NONE),
    ("comment", "/** using */", DOC),
    ("using_declaration", "using u = f();", TARGET),
    ("comment", "/** using destructure: no */", DOC),
    ("using_declaration", "using { ud } = o;", NONE),
    ("comment", "/*** three */", PLAIN),
    ("comment", "// line", PLAIN),
    ("html_comment", "<!-- html -->", PLAIN),
    ("comment", "/**/", PLAIN),
];

#[cfg(feature = "javascript")]
#[test]
fn javascript_rows_answer_the_table() {
    check::<crate::JavascriptCode>(
        JS_SOURCE,
        answers!(crate::roles::javascript, crate::JavascriptCode),
        JS_EXPECTED,
        &declared!(crate::roles::javascript),
        &[],
    );
}

#[cfg(feature = "mozjs")]
#[test]
fn mozjs_rows_answer_the_table() {
    check::<crate::MozjsCode>(
        JS_SOURCE,
        answers!(crate::roles::mozjs, crate::MozjsCode),
        JS_EXPECTED,
        &declared!(crate::roles::mozjs),
        &[],
    );
}

#[cfg(feature = "typescript")]
mod typescript {
    use super::*;
    use crate::{Typescript, TypescriptCode};

    pub(super) const SOURCE: &[u8] = b"/** fn */\nfunction f(/** req */ a: number, /** opt */ b?: number) { }\n/** iface */\ninterface I {\n  /** prop */\n  p: number;\n  /** msig */\n  m(): void;\n  /** index */\n  [k: string]: unknown;\n  /** call */\n  (): void;\n  /** construct */\n  new (): I;\n}\n/** alias */\ntype A = number;\n/** enum */\nenum E {\n  /** assigned */\n  X = 1,\n  /** bare */\n  Y,\n  /** string */\n  \"z\",\n  /** computed */\n  [\"c\"],\n}\n/** ns */\nnamespace N { }\n/** module */\nmodule M { }\n/** ambient */\ndeclare const d: number;\n/** abstract */\nabstract class B {\n  /** abstract m */\n  abstract am(): void;\n  /** field */\n  public fld = 1;\n}\n/** export decl */\nexport const e = 3;\n/** export destructure: no */\nexport const { ed } = o;\n/** export list */\nexport { e };\n/** export star: no */\nexport * from \"m\";\n/** export default fn */\nexport default function () { }\n/** export default class */\nexport default class { }\n/** export default gen */\nexport default function* () { }\n/** export default expr: no */\nexport default 1;\n/** assign: no */\nx = 1;\n/** using */\nusing u = f();\n/** fsig */\nfunction fs(): void;\n/*** three */\n<!-- html -->\nconst o = { \"a\": 1, [\"k\"]: 2 };\nclass S { \"m\"() { } }\n/** var */\nvar v = 1;\n/** gen */\nfunction* g() { }\n/** shorthand */\nconst sh = { v };\n";

    pub(super) const EXPECTED: &[(&str, &str, Answers)] = &[
        ("comment", "/** fn */", DOC),
        (
            "function_declaration",
            "function f(/** req */ a: number, /** opt */ b?: number) { }",
            TARGET,
        ),
        ("comment", "/** req */", DOC),
        ("required_parameter", "a: number", TARGET),
        ("comment", "/** opt */", DOC),
        ("optional_parameter", "b?: number", TARGET),
        ("comment", "/** iface */", DOC),
        ("interface_declaration", "interface I {", TARGET),
        ("comment", "/** prop */", DOC),
        ("property_signature", "p: number", TARGET),
        ("property_identifier", "p", NONE),
        ("comment", "/** msig */", DOC),
        ("method_signature", "m(): void", TARGET),
        ("property_identifier", "m", NONE),
        ("comment", "/** index */", DOC),
        ("index_signature", "[k: string]: unknown", TARGET),
        ("comment", "/** call */", DOC),
        ("call_signature", "(): void", TARGET),
        ("comment", "/** construct */", DOC),
        ("construct_signature", "new (): I", TARGET),
        ("comment", "/** alias */", DOC),
        ("type_alias_declaration", "type A = number;", TARGET),
        ("comment", "/** enum */", DOC),
        ("enum_declaration", "enum E {", TARGET),
        ("comment", "/** assigned */", DOC),
        ("enum_assignment", "X = 1", TARGET),
        ("property_identifier", "X", NONE),
        ("comment", "/** bare */", DOC),
        ("property_identifier", "Y", TARGET),
        ("comment", "/** string */", DOC),
        ("string", "\"z\"", TARGET),
        ("comment", "/** computed */", DOC),
        ("computed_property_name", "[\"c\"]", TARGET),
        ("string", "\"c\"", NONE),
        ("comment", "/** ns */", DOC),
        ("expression_statement", "namespace N { }", TARGET),
        ("comment", "/** module */", DOC),
        ("module", "module M { }", TARGET),
        ("comment", "/** ambient */", DOC),
        ("ambient_declaration", "declare const d: number;", TARGET),
        ("lexical_declaration", "const d: number;", TARGET),
        ("comment", "/** abstract */", DOC),
        ("abstract_class_declaration", "abstract class B {", TARGET),
        ("comment", "/** abstract m */", DOC),
        ("abstract_method_signature", "abstract am(): void", TARGET),
        ("property_identifier", "am", NONE),
        ("comment", "/** field */", DOC),
        ("public_field_definition", "public fld = 1", TARGET),
        ("property_identifier", "fld", NONE),
        ("comment", "/** export decl */", DOC),
        ("export_statement", "export const e = 3;", TARGET),
        ("lexical_declaration", "const e = 3;", TARGET),
        ("comment", "/** export destructure: no */", DOC),
        ("export_statement", "export const { ed } = o;", NONE),
        ("lexical_declaration", "const { ed } = o;", NONE),
        ("comment", "/** export list */", DOC),
        ("export_statement", "export { e };", TARGET),
        ("comment", "/** export star: no */", DOC),
        ("export_statement", "export * from \"m\";", NONE),
        ("string", "\"m\"", NONE),
        ("comment", "/** export default fn */", DOC),
        ("export_statement", "export default function () { }", TARGET),
        ("comment", "/** export default class */", DOC),
        ("export_statement", "export default class { }", TARGET),
        ("comment", "/** export default gen */", DOC),
        (
            "export_statement",
            "export default function* () { }",
            TARGET,
        ),
        ("comment", "/** export default expr: no */", DOC),
        ("export_statement", "export default 1;", NONE),
        ("comment", "/** assign: no */", DOC),
        ("expression_statement", "x = 1;", NONE),
        ("comment", "/** using */", DOC),
        ("expression_statement", "using u = f();", TARGET),
        ("comment", "/** fsig */", DOC),
        ("function_signature", "function fs(): void;", TARGET),
        ("comment", "/*** three */", DOC),
        ("html_comment", "<!-- html -->", PLAIN),
        (
            "lexical_declaration",
            "const o = { \"a\": 1, [\"k\"]: 2 };",
            TARGET,
        ),
        ("pair", "\"a\": 1", TARGET),
        ("string", "\"a\"", NONE),
        ("pair", "[\"k\"]: 2", TARGET),
        ("computed_property_name", "[\"k\"]", NONE),
        ("string", "\"k\"", NONE),
        ("class_declaration", "class S { \"m\"() { } }", TARGET),
        ("method_definition", "\"m\"() { }", TARGET),
        ("string", "\"m\"", NONE),
        ("comment", "/** var */", DOC),
        ("variable_declaration", "var v = 1;", TARGET),
        ("comment", "/** gen */", DOC),
        (
            "generator_function_declaration",
            "function* g() { }",
            TARGET,
        ),
        ("comment", "/** shorthand */", DOC),
        ("lexical_declaration", "const sh = { v };", TARGET),
        ("shorthand_property_identifier", "v", TARGET),
    ];

    // Every tsc target: the signatures and parameters, an enum member
    // assigned, bare (a `property_identifier` under the `enum_body`,
    // and not the ones naming a signature) and spelled as a `string`,
    // a namespace (an `internal_module` under an `expression_statement`),
    // an ambient declaration, an export of a declaration, of a list and
    // of each anonymous default form (a function, a class, a generator)
    // against `export *` and `export default <expr>`, an assignment (no
    // target: tsc does not declare one), and `using`; `/***` is TSDoc
    // to tsc (not to JSDoc), the HTML comment never; a `string` as an
    // object key or a method name is no enum member. The row answers
    // the *candidate*: the `const e` under `export` and the `const d`
    // under `declare` are each a row owner here, and the Checker test
    // below is where the wrapper context says no.
    #[test]
    fn rows_answer_the_table() {
        check::<TypescriptCode>(
            SOURCE,
            answers!(crate::roles::typescript, TypescriptCode),
            EXPECTED,
            &declared!(crate::roles::typescript),
            &defensive_ts(),
        );
    }

    /// The second parameter aliases the grammar gives no spelling:
    /// `required_parameter2` / `optional_parameter2` are the tuple-member
    /// aliases, and a tuple type's members take the primary ids at the
    /// pin.
    fn defensive_ts() -> Vec<u16> {
        vec![
            Typescript::RequiredParameter2 as u16,
            Typescript::OptionalParameter2 as u16,
        ]
    }

    /// The Checker composes the row with the wrapper context: a
    /// declaration directly under `export` or `declare` is no target to
    /// tsc (the wrapper is), while the row owns it as a candidate. And
    /// the chain-gated owners (`property_identifier` under `enum_body`)
    /// are never an export's `declaration` child, which is why the
    /// export gate may ask the row's `owns` of that child with the
    /// export's own chain — pinned by the disjointness assertion.
    #[test]
    fn checker_composes_the_row_with_the_wrapper_context() {
        let source = b"export const e = 3;\ndeclare const d: number;\nexport default function () { }\nexport abstract class X { }\nconst k = 1;\nexport const { ed } = o;\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<TypescriptCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind: Typescript = node.kind_id().into();
            if !matches!(
                kind,
                Typescript::LexicalDeclaration
                    | Typescript::ExportStatement
                    | Typescript::AmbientDeclaration
                    | Typescript::AbstractClassDeclaration
            ) {
                return;
            }
            seen.push((
                node.kind(),
                crate::roles::typescript::doc_target::owns(node, source, ancestors),
                TypescriptCode::is_doc_target(node, source, ancestors),
            ));
        });
        assert_eq!(
            seen,
            [
                ("export_statement", true, true),
                ("lexical_declaration", true, false),
                ("ambient_declaration", true, true),
                ("lexical_declaration", true, false),
                ("export_statement", true, true),
                ("export_statement", true, true),
                ("abstract_class_declaration", true, false),
                ("lexical_declaration", true, true),
                // The destructuring export: the export's gate asks the row
                // of its child under the child's own gate, which binds no
                // name — row and Checker both `false`, as at base, where a
                // kind-table read answered `true` and documented it (the
                // one drift the corpus oracle could not see).
                ("export_statement", false, false),
                ("lexical_declaration", false, false),
            ]
        );
        // The export gate asks the row of its child with the *export's*
        // chain, one short for the child; exact only because the row's
        // chain-gated owners can never be that child. What the child can
        // be is the grammar's `declaration` supertype (`node-types.json`,
        // which the grammar crate ships), read here rather than copied;
        // the fixture below spells every subtype, and the parsed ids are
        // held to that list, so a grammar bump that adds a form fails
        // here until the fixture spells it and the owners are re-checked.
        let node_types: serde_json::Value =
            serde_json::from_str(tree_sitter_typescript::TYPESCRIPT_NODE_TYPES)
                .expect("the grammar crate ships valid node-types.json");
        let declaration_subtypes: std::collections::BTreeSet<&str> = node_types
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["type"] == "declaration")
            .and_then(|t| t["subtypes"].as_array())
            .expect("a `declaration` supertype")
            .iter()
            .filter_map(|t| t["type"].as_str())
            .collect();
        let chain_gated = [
            Typescript::PropertyIdentifier as u16,
            Typescript::ComputedPropertyName as u16,
            Typescript::String as u16,
        ];
        let exports = b"export const a = 1;\nexport let b = 2;\nexport var c = 3;\nexport function f() { }\nexport function* g() { }\nexport class C { }\nexport abstract class A { }\nexport interface I { }\nexport type T = number;\nexport enum E { X }\nexport namespace N { }\nexport module M { }\nexport declare const d: number;\nexport declare namespace DN { }\nexport function fs(): void;\nexport import X = N;\n";
        let mut declaration_children = std::collections::BTreeSet::new();
        let mut declaration_kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<TypescriptCode>(exports, |node, _| {
            if node.kind_id() == Typescript::ExportStatement as u16 {
                let child = node
                    .child_by_field_name("declaration")
                    .expect("every export in the fixture declares");
                declaration_children.insert(child.kind_id());
                declaration_kinds.insert(child.kind());
            }
        });
        assert_eq!(
            declaration_kinds, declaration_subtypes,
            "the fixture spells every `declaration` subtype of the pinned grammar"
        );
        assert!(
            chain_gated
                .iter()
                .all(|k| !declaration_children.contains(k))
        );
    }
}

#[cfg(feature = "typescript")]
mod tsx {
    use super::*;
    use crate::{Tsx, TsxCode};

    /// TypeScript's fixture with the Annex-B `<!-- -->` line — a parse
    /// error under JSX — replaced by the closing form `-->`, which the
    /// scanner reads as an `html_comment` at the start of a line even
    /// in TSX; and TypeScript's table with that row's text to match. So
    /// the TSX `Comment` row's second owner is met on TSX's own ids.
    fn tsx_fixture() -> (Vec<u8>, Vec<(&'static str, &'static str, Answers)>) {
        const OPENING: &[u8] = b"<!-- html -->";
        const CLOSING: &[u8] = b"--> html";
        let at = super::typescript::SOURCE
            .windows(OPENING.len())
            .position(|w| w == OPENING)
            .expect("the TypeScript fixture spells the HTML comment");
        let mut source = super::typescript::SOURCE[..at].to_vec();
        source.extend_from_slice(CLOSING);
        source.extend_from_slice(&super::typescript::SOURCE[at + OPENING.len()..]);
        let expected = super::typescript::EXPECTED
            .iter()
            .map(|&(kind, text, answers)| match kind {
                "html_comment" => (kind, "--> html", answers),
                _ => (kind, text, answers),
            })
            .collect();
        (source, expected)
    }

    // The TSX grammar is TypeScript's with JSX: the same rows, the same
    // fragment, instantiated with this grammar's ids — the `string` kind
    // an enum member can be spelled as is `String2` here against
    // `String` in TS, the one id the two rows differ in (#1699). So the
    // whole TypeScript table is pinned here on this grammar, every owner
    // and both sides of every gate met on TSX's own ids, not TS's.
    #[test]
    fn rows_answer_the_table() {
        let (source, expected) = tsx_fixture();
        check::<TsxCode>(
            &source,
            answers!(crate::roles::tsx, TsxCode),
            &expected,
            &declared!(crate::roles::tsx),
            // The tuple-member parameter aliases, as in TS.
            &[
                Tsx::RequiredParameter2 as u16,
                Tsx::OptionalParameter2 as u16,
            ],
        );
        // The one id the TS and TSX rows differ in.
        assert!(crate::roles::tsx::doc_target::OWNERS.contains(&(Tsx::String2 as u16)));
        assert!(!crate::roles::tsx::doc_target::OWNERS.contains(&(Tsx::String as u16)));
    }

    /// The wrapper context composes here as in TS: a declaration under
    /// `export` / `declare` is no target, the rest are.
    #[test]
    fn checker_composes_the_wrapper_context() {
        let (source, _) = tsx_fixture();
        let mut seen = Vec::new();
        for_each_node_with_chain::<TsxCode>(&source, |node, chain| {
            if node.kind_id() == Tsx::LexicalDeclaration as u16 {
                let text = node.utf8_text(&source).unwrap_or("");
                seen.push((
                    text.lines().next().unwrap_or("").to_owned(),
                    TsxCode::is_doc_target(node, &source, Ancestors::known(chain)),
                ));
            }
        });
        let under_wrappers: Vec<_> = seen
            .iter()
            .filter(|(_, t)| !t)
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(
            under_wrappers,
            ["const d: number;", "const e = 3;", "const { ed } = o;"]
        );
        assert!(seen.iter().filter(|(_, t)| *t).count() >= 2, "{seen:?}");
    }
}

// Python: the one language with a `Docstring` row. A docstring is a
// string statement, so neither comment predicate reads it — pinned
// beside a `#` comment answering `true`.

#[cfg(feature = "python")]
mod python {
    use super::*;
    use crate::{Python, PythonCode};

    const DOC_STRING: Answers = at(&[DOCSTRING]);

    // Every documenting position (PEP 257: the body's first statement
    // and an additional docstring after it; PEP 258 / Sphinx: after an
    // assignment to a name at module or class level, annotated or not,
    // and to `self.x` at the top of `__init__`) against every
    // non-documenting one (after an import, after an assignment to
    // another object's attribute or a local, in the middle of a body,
    // an f-string, a bytes literal, an element of a discarded tuple in
    // the one place a single string would document — after an
    // assignment). The concatenated docstring is one
    // owner — the `concatenated_string`, never a fragment. Every string
    // and comment is listed, so `(false, false)` for `is_comment` /
    // `is_doc_comment` on each docstring is asserted, not implied.
    #[test]
    fn rows_answer_the_table() {
        let source = b"\"\"\"module doc\"\"\"\n\"\"\"additional\"\"\"\n# plain\nimport os\n\"\"\"after import: no\"\"\"\nx = 1\n\"\"\"attribute doc\"\"\"\ny: int = 2\n\"\"\"annotated attribute doc\"\"\"\nclass C:\n    \"\"\"class doc\"\"\"\n    'single' \"concatenated\"\n    z = 3\n    \"\"\"z doc\"\"\"\n    def __init__(self):\n        \"\"\"init doc\"\"\"\n        self.a = 1\n        \"\"\"self.a doc\"\"\"\n        other.b = 2\n        \"\"\"other.b: no\"\"\"\n    def m(self):\n        \"\"\"m doc\"\"\"\n        w = 1\n        \"\"\"local: no\"\"\"\n        \"\"\"discarded in the middle\"\"\"\n        return w\ndef f():\n    f\"\"\"fstring: no\"\"\"\n    b\"\"\"bytes: no\"\"\"\n    return 1\nt = 1\n\"tuple\", \"element: no\"\n";
        let expected: &[(&str, &str, Answers)] = &[
            ("string", "\"\"\"module doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"additional\"\"\"", DOC_STRING),
            ("comment", "# plain", PLAIN),
            ("string", "\"\"\"after import: no\"\"\"", NONE),
            ("string", "\"\"\"attribute doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"annotated attribute doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"class doc\"\"\"", DOC_STRING),
            (
                "concatenated_string",
                "'single' \"concatenated\"",
                DOC_STRING,
            ),
            ("string", "'single'", NONE),
            ("string", "\"concatenated\"", NONE),
            ("string", "\"\"\"z doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"init doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"self.a doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"other.b: no\"\"\"", NONE),
            ("string", "\"\"\"m doc\"\"\"", DOC_STRING),
            ("string", "\"\"\"local: no\"\"\"", NONE),
            ("string", "\"\"\"discarded in the middle\"\"\"", NONE),
            ("string", "f\"\"\"fstring: no\"\"\"", NONE),
            ("string", "b\"\"\"bytes: no\"\"\"", NONE),
            ("string", "\"tuple\"", NONE),
            ("string", "\"element: no\"", NONE),
        ];
        check::<PythonCode>(
            source,
            answers!(crate::roles::python, PythonCode),
            expected,
            &declared!(crate::roles::python),
            &[],
        );
    }

    /// The run helper credits the rows the row says are documentation
    /// and no others — the exact `(first, last)` sequence, as it was
    /// before the row (#1699): one credit per run, at its final member,
    /// covering every member's rows. And the broad discarded-string
    /// classification (`python_is_bare_string`, #415) is untouched: the
    /// mid-body string is bare and not documentation.
    #[test]
    fn run_helper_credits_what_the_row_says() {
        use crate::lang_helpers::python::{python_documentation_string_run, python_is_bare_string};
        let source = b"\"\"\"a\"\"\"\n\"\"\"b\"\"\"\nx = 1\n\"\"\"c\"\"\"\ndef f():\n    y = 1\n    \"\"\"d: no\"\"\"\n    return y\n";
        let mut credited = Vec::new();
        let mut bare = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            if node.kind_id() != Python::String as u16 {
                return;
            }
            let ancestors = Ancestors::known(chain);
            bare.push((
                node.start_row(),
                python_is_bare_string(node, ancestors),
                crate::roles::python::docstring::owns(node, source, ancestors),
            ));
            python_documentation_string_run(node, source, ancestors, |first, last| {
                credited.push((node.start_row(), first, last));
            });
        });
        assert_eq!(
            bare,
            [
                (0, true, true),
                (1, true, true),
                (3, true, true),
                (6, true, false)
            ]
        );
        // `b` is the run's final member and credits both rows; `c` its own.
        assert_eq!(credited, [(1, 0, 0), (1, 1, 1), (3, 3, 3)]);
    }

    /// The run helper scans once per run, at its final member — the
    /// cost the row shares rather than doubles. Counted, since a scaling
    /// probe cannot tell one scan per run from two: on a run of five
    /// strings and a second run of one, two scans.
    #[test]
    fn one_scan_per_run() {
        use crate::lang_helpers::python::{
            docstring_runs_scanned, python_documentation_string_run,
        };
        let source = b"\"\"\"a\"\"\"\n\"\"\"b\"\"\"\n\"\"\"c\"\"\"\n\"\"\"d\"\"\"\n\"\"\"e\"\"\"\nx = 1\n\"\"\"f\"\"\"\n";
        let runs = 2;
        let before = docstring_runs_scanned::observed();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            if node.kind_id() == Python::String as u16 {
                python_documentation_string_run(node, source, Ancestors::known(chain), |_, _| {});
            }
        });
        assert_eq!(
            docstring_runs_scanned::observed() - before,
            runs,
            "one scan per run"
        );
    }
}
