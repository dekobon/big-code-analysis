//! The `DLOC` metric suite (#1597), in its own file for the same
//! reason `shared.rs` is (#976): `loc.rs` is past the self-scan size
//! limit, and the suite need not grow it further.

use super::line_set::LineSet;
use super::min_or_zero;

/// The `DLOC` metric suite: Documentation Lines Of Code, the rows a
/// language classifies as documentation (#1597).
///
/// A doc comment — by the grammar's mark (Rust, Groovy, Perl POD) or
/// by the opening marker its language's own tool reads, which
/// `Checker::is_doc_comment` answers per language — and a Python
/// docstring are the constructs recorded here, and their rows are
/// exactly the rows the same arms also record as comment lines, so
/// `dloc() <= cloc()` holds by construction — `add_comment_rows` inserts
/// into both, and Python's `add_sphinx_doc_comment` (`#:` comments)
/// re-bills only rows a comment arm already recorded. A language without a
/// documentation syntax this crate recognises reports `0`; Elixir's
/// `@doc` heredoc is an assignment the walk classes as code (#1260).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Dloc {
    pub(super) lines: LineSet,
    dloc_min: usize,
    dloc_max: usize,
}

impl Default for Dloc {
    fn default() -> Self {
        Self {
            lines: LineSet::default(),
            dloc_min: usize::MAX,
            dloc_max: 0,
        }
    }
}

impl Dloc {
    /// The `DLOC` metric value for this space (documentation rows; a
    /// subset of `cloc`).
    #[inline]
    #[must_use]
    pub(crate) fn dloc(&self) -> u64 {
        self.lines.len() as u64
    }

    /// The `Dloc` metric minimum value. See `min_or_zero` for the
    /// `usize::MAX` sentinel guard.
    #[inline]
    #[must_use]
    pub(crate) fn dloc_min(&self) -> u64 {
        min_or_zero(self.dloc_min)
    }

    /// The `Dloc` metric maximum value.
    #[inline]
    #[must_use]
    pub(crate) fn dloc_max(&self) -> u64 {
        self.dloc_max as u64
    }

    /// Folds `other` into `self`, unioning the line set and updating min/max.
    #[inline]
    pub(crate) fn merge(&mut self, other: &Dloc) {
        // Row union, as for `Ploc`: a documentation row is folded upward
        // once per enclosing space and must count once.
        self.lines.union_with(&other.lines);

        // Fold the child's own min/max so nested spaces propagate (#437).
        self.dloc_min = self.dloc_min.min(other.dloc_min);
        self.dloc_max = self.dloc_max.max(other.dloc_max);
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        // Fold this space's own value unconditionally so containers
        // participate, matching the sibling metrics' convention (#437).
        let dloc = self.dloc() as usize;
        self.dloc_min = self.dloc_min.min(dloc);
        self.dloc_max = self.dloc_max.max(dloc);
    }
}

// Test code, where the module-level `arithmetic_side_effects` warn
// (a `usize` underflow guard for the production walk) would only
// noise up counters. The attribute run must stay contiguous: a comment
// between the two breaks the `exclude_tests` prune.
#[cfg(test)]
#[allow(clippy::too_many_lines, clippy::arithmetic_side_effects)]
mod tests {
    use crate::LANG;
    use crate::Metric;
    use crate::MetricsOptions;
    use crate::checker::Checker;
    use crate::langs::AnyParser;
    use crate::macros::with_any_parser;
    use crate::test_support::metrics_verbatim;
    use crate::traits::ParserTrait;

    /// The walk restricted to the one metric these tests read (#1127).
    #[cfg(any(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "groovy",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "perl",
        feature = "php",
        feature = "python",
        feature = "rust",
        feature = "typescript",
    ))]
    fn loc_only() -> MetricsOptions {
        MetricsOptions::default().with_only(&[Metric::Loc])
    }

    /// A doc marker bills its row as `dloc` and `cloc`; a plain comment
    /// bills `cloc` alone. Both halves per spelling, so a predicate that
    /// is too wide and one that is too narrow each fail a row here.
    #[cfg(all(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "groovy",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "php",
        feature = "rust",
        feature = "typescript",
    ))]
    #[test]
    fn doc_markers_follow_each_languages_own_tool() {
        /// One single-row code fixture per language whose grammar emits one
        /// comment kind, or marks documentation structurally. A fixture is
        /// the comment row above this code row — after it for PHP, whose
        /// comment must follow `<?php`.
        const CODE_ROW: &[(LANG, &str)] = &[
            (LANG::C, "int x;"),
            (LANG::Cpp, "int x;"),
            (LANG::Mozcpp, "int x;"),
            (LANG::Objc, "int x;"),
            (LANG::Java, "class C {}"),
            (LANG::Kotlin, "val x = 1"),
            (LANG::Javascript, "let x = 1;"),
            (LANG::Mozjs, "let x = 1;"),
            (LANG::Typescript, "let x = 1;"),
            (LANG::Tsx, "let x = 1;"),
            (LANG::Php, "<?php $x = 1;"),
            (LANG::Csharp, "class C {}"),
            (LANG::Rust, "fn f() {}"),
            (LANG::Groovy, "def f() {}"),
        ];

        /// Every comment spelling worth pinning per language, with whether
        /// that language's own tool treats it as documentation: Doxygen for
        /// the C family, javac, Kotlin's lexer, JSDoc, PHP's scanner, Roslyn,
        /// tree-sitter-rust's `doc_comment` child, Groovy's kind as
        /// Groovydoc reads it. The pitfalls are the edges where those tools
        /// disagree (`/***`, `////`, `/**/`, `//!`), which is why the table
        /// is per language rather than one list.
        fn marker_table(lang: LANG) -> &'static [(&'static str, bool)] {
            use LANG::*;
            match lang {
                C | Cpp | Mozcpp | Objc => &[
                    ("/** d */", true),
                    ("/*! d */", true),
                    ("/// d", true),
                    ("//! d", true),
                    ("///< d", true),
                    ("/* d */", false),
                    ("// d", false),
                    ("/**/", false),
                    ("/*** d */", false),
                    ("//// d", false),
                ],
                Java => &[
                    ("/** d */", true),
                    ("/*** d */", true),
                    ("/// d", true),
                    ("/* d */", false),
                    ("// d", false),
                    ("/**/", false),
                    ("//// d", false),
                ],
                Kotlin => &[
                    ("/** d */", true),
                    ("/*** d */", true),
                    ("/* d */", false),
                    ("// d", false),
                    ("/**/", false),
                    ("/// d", false),
                ],
                // JSDoc ignores `/***`; tsc reads it, so TypeScript and TSX
                // follow tsc.
                Javascript | Mozjs => &[
                    ("/** d */", true),
                    ("/* d */", false),
                    ("/*** d */", false),
                    ("/**/", false),
                    ("// d", false),
                ],
                Typescript | Tsx => &[
                    ("/** d */", true),
                    ("/* d */", false),
                    ("/*** d */", true),
                    ("/**/", false),
                    ("// d", false),
                ],
                Php => &[
                    ("/** d */", true),
                    ("/**/", false),
                    ("/*** d */", false),
                    ("/**d */", false),
                    ("# d", false),
                    ("// d", false),
                ],
                Csharp => &[
                    ("/// d", true),
                    ("/** d */", true),
                    ("//! d", false),
                    ("/*! d */", false),
                    ("//// d", false),
                    ("/**/", false),
                    ("/*** d */", false),
                    ("// d", false),
                ],
                Rust => &[
                    ("/// d", true),
                    ("//! d", true),
                    ("/** d */", true),
                    ("/*! d */", true),
                    ("// d", false),
                    ("//// d", false),
                    ("/**/", false),
                ],
                Groovy => &[
                    ("/** d */", true),
                    ("/*** d */", true),
                    ("/* d */", false),
                    ("// d", false),
                    ("/**/", false),
                ],
                _ => &[],
            }
        }

        let mut ran = 0;
        for (lang, code) in CODE_ROW {
            let table = marker_table(*lang);
            assert!(!table.is_empty(), "{lang:?} has no marker table");
            for (spelling, doc) in table {
                let source = if *lang == LANG::Php {
                    format!("{code}\n{spelling}\n")
                } else {
                    format!("{spelling}\n{code}\n")
                };
                let loc = metrics_verbatim(*lang, source.as_bytes(), loc_only()).loc;
                // The second axis: the comment row stays a comment row and
                // the code row stays code, whichever way the marker goes.
                assert_eq!(
                    (loc.sloc(), loc.ploc(), loc.cloc()),
                    (2, 1, 1),
                    "{lang:?} {spelling:?}: one code row and one comment row"
                );
                assert_eq!(
                    loc.dloc(),
                    u64::from(*doc),
                    "{lang:?} {spelling:?}: documentation iff the tool says so"
                );
                ran += 1;
            }
        }
        assert!(ran > 40, "the table drove {ran} fixtures");
    }

    /// Perl's documentation is POD, a multi-row statement the grammar
    /// marks; every row of the block is a `dloc` row and a `cloc` row,
    /// unless the block is nothing but `comment` regions, Perl's idiom for
    /// commenting code out. POD after a region's end documents, so a
    /// block carrying any is documentation, its region rows included.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_pod_block_is_documentation() {
        // (source, [sloc, ploc, cloc, dloc, blank])
        let cases: &[(&[u8], [u64; 5])] = &[
            (
                b"my $x = 1;\n\n=pod\n\nText.\n\n=cut\n\nmy $y = 2;\n",
                [9, 2, 5, 5, 2],
            ),
            (
                b"my $x = 1;\n\n=begin comment\n\nold();\n\n=end comment\n\n=cut\n\nmy $y = 2;\n",
                [11, 2, 7, 0, 2],
            ),
            (
                b"my $x = 1;\n\n=for comment\nold();\n\n=cut\n\nmy $y = 2;\n",
                [8, 2, 4, 0, 2],
            ),
            (
                b"my $x = 1;\n\n=for comment\nInternal note.\n\n=head1 NAME\n\nFoo - bar\n\n=cut\n\nmy $y = 2;\n",
                [12, 2, 8, 8, 2],
            ),
            (
                b"my $x = 1;\n\n=begin comment\n\nold();\n\n=end comment\n\n=head1 NAME\n\nFoo - bar\n\n=cut\n\nmy $y = 2;\n",
                [15, 2, 11, 11, 2],
            ),
            (b"my $x = 1;\n# c\n", [2, 1, 1, 0, 0]),
        ];
        for (source, expected) in cases {
            let loc = metrics_verbatim(LANG::Perl, source, loc_only()).loc;
            assert_eq!(
                [loc.sloc(), loc.ploc(), loc.cloc(), loc.dloc(), loc.blank()],
                *expected,
                "{}",
                String::from_utf8_lossy(source)
            );
        }
    }

    /// A doc comment sharing its row with code is a `dloc` row and a code
    /// row at once; `dloc_without_code` leaves it to the MI size term,
    /// as it does for `def f(): """doc"""` (#1597).
    #[cfg(all(
        feature = "c",
        feature = "csharp",
        feature = "java",
        feature = "javascript",
        feature = "php",
        feature = "rust",
    ))]
    #[test]
    fn a_doc_comment_sharing_a_code_row_keeps_the_row() {
        let fixtures: &[(LANG, &[u8], u64)] = &[
            (LANG::C, b"int x; /** d */\n", 1),
            (LANG::Java, b"class C { int x; /** d */ }\n", 1),
            (LANG::Javascript, b"let x = 1; /** d */\n", 1),
            (LANG::Php, b"<?php $x = 1; /** d */\n", 1),
            // The `///` form consumes its newline into the `doc_comment`
            // child, the #1051 end-row discount path; the item after it
            // keeps the fixture valid Rust.
            (LANG::Rust, b"fn f() {} /// d\nfn g() {}\n", 2),
            (LANG::Csharp, b"class C { int x; /// d\n}\n", 2),
        ];
        for (lang, source, rows) in fixtures {
            let loc = metrics_verbatim(*lang, source, loc_only()).loc;
            let text = String::from_utf8_lossy(source);
            assert_eq!(
                (loc.sloc(), loc.ploc()),
                (*rows, *rows),
                "{lang:?} {text:?}: all rows are code"
            );
            assert_eq!(
                (loc.cloc(), loc.dloc()),
                (1, 1),
                "{lang:?} {text:?}: one doc row"
            );
            assert_eq!(
                loc.dloc_without_code(),
                0,
                "{lang:?} {text:?}: the shared row stays in the size term"
            );
        }
    }

    /// Counts `(doc comments, comments)` over every node of a parse,
    /// asserting the cross-walk obligation of grammar-dispatch rule 7
    /// on each: a documentation comment is a comment.
    #[cfg(any(
        feature = "bash",
        feature = "c",
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
    fn count_doc_and_plain_comments<P: ParserTrait>(parser: &P) -> (usize, usize) {
        let code = parser.code();
        parser
            .root()
            .preorder()
            .fold((0, 0), |(docs, comments), node| {
                let doc = P::Checker::is_doc_comment(&node, code);
                let comment = P::Checker::is_comment(&node);
                assert!(
                    !doc || comment,
                    "is_doc_comment accepted a non-comment `{}`",
                    node.kind()
                );
                (docs + usize::from(doc), comments + usize::from(comment))
            })
    }

    /// Every grammar's `is_doc_comment` implies its `is_comment`, and
    /// every language with a documentation syntax recognises at least
    /// one instance of it — so the implication is not satisfied by a
    /// predicate that is `false` everywhere. The fixtures carry one doc
    /// spelling and one plain comment where the language has both.
    #[cfg(any(
        feature = "bash",
        feature = "c",
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
    #[test]
    fn is_doc_comment_implies_is_comment_in_every_language() {
        use LANG::*;
        // (source, has a documentation syntax the predicate must find)
        let fixture = |lang: LANG| -> Option<(&'static str, bool)> {
            Some(match lang {
                C | Cpp | Mozcpp | Objc => ("/** d */\n// c\nint x;\n", true),
                Java => ("/** d */\n// c\nclass C {}\n", true),
                Kotlin => ("/** d */\n// c\nval x = 1\n", true),
                Javascript | Mozjs | Typescript | Tsx => ("/** d */\n// c\nlet x = 1;\n", true),
                Php => ("<?php $x = 1;\n/** d */\n// c\n", true),
                Csharp => ("/// d\n// c\nclass C {}\n", true),
                Rust => ("/// d\n// c\nfn f() {}\n", true),
                Groovy => ("/** d */\n// c\ndef f() {}\n", true),
                Perl => ("=pod\n\nd\n\n=cut\n# c\nmy $x = 1;\n", true),
                // No documentation marker on a comment: Python documents
                // with strings, Elixir with `@doc` attributes the walk
                // classes as code (#1260), and Go, Ruby and Lua by a
                // comment's position, which a lone comment does not have.
                Python | Elixir | Bash | Tcl | Irules | Ruby => ("# c\n", false),
                Go => ("package p\n// c\n", false),
                Lua => ("-- c\n", false),
                Preproc | Ccomment => return None,
            })
        };
        let mut ran = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            let Some((source, has_docs)) = fixture(lang) else {
                continue;
            };
            let any = AnyParser::parse(lang, source.as_bytes().to_vec(), None, None)
                .expect("an enabled language parses");
            let (docs, comments) = with_any_parser!(&any, |p| count_doc_and_plain_comments(p));
            assert!(comments >= 1, "{lang:?}: the fixture must carry a comment");
            if has_docs {
                assert!(docs >= 1, "{lang:?}: the doc spelling was not recognised");
                assert!(
                    docs < comments,
                    "{lang:?}: the plain comment was taken for documentation"
                );
            } else {
                assert_eq!(docs, 0, "{lang:?}: a plain comment is not documentation");
            }
            ran += 1;
        }
        assert!(
            ran > 0,
            "at least one language feature must be enabled for this test to mean anything"
        );
    }

    /// Asserts each `(name, source, dloc, cloc)` row of one language's
    /// table.
    #[cfg(any(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "groovy",
        feature = "java",
        feature = "kotlin",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "perl",
        feature = "php",
        feature = "python",
        feature = "rust",
        feature = "typescript",
    ))]
    fn assert_doc_rows(lang: LANG, cases: &[(&str, &[u8], u64, u64)]) {
        for (name, source, dloc, cloc) in cases {
            let loc = metrics_verbatim(lang, source, loc_only()).loc;
            assert_eq!((loc.dloc(), loc.cloc()), (*dloc, *cloc), "{lang:?}: {name}");
        }
    }

    /// A doc-marker comment inside a function or closure body is a working
    /// note where the tool ignores it — rustc's `unused_doc_comments`,
    /// Roslyn CS1587, javadoc and Dokka reading declarations only, PHPDoc
    /// type hints — so it bills `cloc` and not `dloc`. The C family keeps
    /// it, as Doxygen publishes it as the function's in-body description,
    /// and the JS family keeps in-body JSDoc (inner symbols) (#1597).
    #[cfg(all(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "php",
        feature = "rust",
        feature = "typescript",
    ))]
    #[test]
    fn in_body_doc_comments_follow_each_tool() {
        let cases: &[(LANG, &[u8], u64, u64)] = &[
            // (lang, source, dloc, cloc)
            (LANG::Cpp, b"/// Doc.\nvoid f() {\n    /// note\n    int x = 1;\n}\n", 2, 2),
            (LANG::C, b"/** Doc. */\nvoid f(void) {\n    /** note */\n    int x = 1;\n}\n", 2, 2),
            (LANG::Rust, b"/// Doc.\nfn f() {\n    /// note\n    let x = 1;\n}\n", 1, 2),
            // Only the closure encloses the note: a `const` item is no function.
            (LANG::Rust, b"const G: fn() -> i32 = || {\n    /// note\n    1\n};\n", 0, 1),
            (LANG::Java, b"class C {\n    /** Doc. */\n    void f() {\n        /** note */\n        int x = 1;\n    }\n}\n", 1, 2),
            (LANG::Kotlin, b"/** Doc. */\nfun f() {\n    /** note */\n    val x = 1\n}\n", 1, 2),
            (LANG::Csharp, b"class C {\n    /// Doc.\n    void F() {\n        /// note\n        int x = 1;\n    }\n}\n", 1, 2),
            (LANG::Php, b"<?php\n/** Doc. */\nfunction f() {\n    /** @var int $x */\n    $x = 1;\n}\n", 1, 2),
            (LANG::Javascript, b"/** Doc. */\nfunction f() {\n    /** Inner. */\n    function g() {}\n}\n", 2, 2),
            (LANG::Typescript, b"/** Doc. */\nfunction f() {\n    /** Inner. */\n    const x = 1;\n}\n", 2, 2),
        ];
        for (lang, source, dloc, cloc) in cases {
            let loc = metrics_verbatim(*lang, source, loc_only()).loc;
            let text = String::from_utf8_lossy(source);
            assert_eq!(
                (loc.dloc(), loc.cloc()),
                (*dloc, *cloc),
                "{lang:?} {text:?}"
            );
        }
    }

    /// The Rust row discount (#1051) follows a doc comment's spelling, not
    /// its place: an in-body `///` still swallows its newline, so the row
    /// below it is not credited as a comment row. The in-body table above
    /// pins the same row, but only in a build with all nine of its
    /// languages; this is the guard a Rust-only build runs.
    #[cfg(feature = "rust")]
    #[test]
    fn rust_in_body_triple_slash_keeps_the_row_discount() {
        let loc = metrics_verbatim(
            LANG::Rust,
            b"fn f() {\n    /// note\n    let x = 1;\n}\n",
            loc_only(),
        )
        .loc;
        assert_eq!(
            (loc.sloc(), loc.ploc(), loc.cloc(), loc.dloc()),
            (4, 3, 1, 0)
        );
    }

    /// A doc comment in a function's signature — on a parameter, after an
    /// attribute or annotation — is outside its body, and documents where
    /// the language's tool reads it there: Doxygen's `/**<` parameter
    /// blocks and blocks after `[[attributes]]`, PHP's docblock after
    /// `#[…]` and on a promoted constructor property, Kotlin's KDoc after
    /// an annotation. javac and Roslyn bind a doc comment only before a
    /// declaration's first token, so the same placement documents nothing
    /// in Java or C# (#1597).
    #[cfg(all(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "java",
        feature = "kotlin",
        feature = "objc",
        feature = "php",
    ))]
    #[test]
    fn doc_comments_in_a_signature_follow_each_tool() {
        assert_doc_rows(
            LANG::C,
            &[(
                "parameter of a definition",
                b"void f(int a /**< the a */) {\n    return;\n}\n",
                1,
                1,
            )],
        );
        assert_doc_rows(
            LANG::Cpp,
            &[
                (
                    "after an attribute",
                    b"[[nodiscard]]\n/// Doc.\nint f() { return 1; }\n",
                    1,
                    1,
                ),
                (
                    "in the body: Doxygen's in-body description",
                    b"int f() {\n    /// note\n    return 1;\n}\n",
                    1,
                    1,
                ),
            ],
        );
        assert_doc_rows(
            LANG::Objc,
            &[
                (
                    "parameter of a definition",
                    b"void f(int a /**< the a */) {\n    return;\n}\n",
                    1,
                    1,
                ),
                (
                    "in a method body: Doxygen's in-body description",
                    b"@implementation C\n- (void)m {\n    /** note */\n    int x = 1;\n}\n@end\n",
                    1,
                    1,
                ),
            ],
        );
        assert_doc_rows(
            LANG::Php,
            &[
                (
                    "promoted property",
                    b"<?php\nclass P {\n    public function __construct(\n        /** The x. */\n        public int $x = 0,\n    ) {}\n}\n",
                    1,
                    1,
                ),
                (
                    "after an attribute",
                    b"<?php\n#[Pure]\n/** Doc. */\nfunction f() {}\n",
                    1,
                    1,
                ),
            ],
        );
        // A bodiless declaration holds no doc comment in its body: an
        // interface or `abstract` fun and a delegating secondary
        // constructor read their KDoc after an annotation as a bodied
        // function does (Kotlin's `findDocComment` reads past the
        // modifier list whatever follows).
        assert_doc_rows(
            LANG::Kotlin,
            &[
                (
                    "after an annotation",
                    b"@Deprecated(\"x\")\n/** Doc. */\nfun f() {}\n",
                    1,
                    1,
                ),
                (
                    "after an annotation, interface fun",
                    b"interface I {\n    @Deprecated(\"x\")\n    /** Doc. */\n    fun f(): Int\n}\n",
                    1,
                    1,
                ),
                (
                    "after an annotation, abstract fun",
                    b"abstract class A {\n    @Deprecated(\"x\")\n    /** Doc. */\n    abstract fun h(): Int\n}\n",
                    1,
                    1,
                ),
                (
                    "after an annotation, delegating secondary constructor",
                    b"class C(val x: Int, val y: Int) {\n    @Deprecated(\"x\")\n    /** Doc. */\n    constructor(x: Int) : this(x, 0)\n}\n",
                    1,
                    1,
                ),
            ],
        );
        assert_doc_rows(
            LANG::Java,
            &[(
                "after an annotation",
                b"class C {\n    @Override\n    /** Doc. */\n    public String toString() { return \"\"; }\n}\n",
                0,
                1,
            )],
        );
        assert_doc_rows(
            LANG::Csharp,
            &[(
                "after an attribute",
                b"class C {\n    [System.Obsolete]\n    /// <summary>x</summary>\n    void F() {}\n}\n",
                0,
                1,
            )],
        );
    }

    /// An inner doc comment (`//!`, `/*! */`) opening a function's body is
    /// the function's own `#![doc]`: rustdoc renders it and rustc does not
    /// lint it, so it documents as an outer `///` above the function does.
    /// Anywhere else in a body it does not — rustc lints it on a loop or a
    /// block, and rustdoc renders no item nested in a body (#1597). A `//!`
    /// after a statement, an item or an outer doc comment in the body is a
    /// hard error (E0753), so no row pins one: only rustc-rejected input
    /// could tell the body-block rule from a prefix scan (grammar-dispatch
    /// rule 6).
    #[cfg(feature = "rust")]
    #[test]
    fn rust_inner_doc_comment_opening_a_fn_body_documents_it() {
        assert_doc_rows(
            LANG::Rust,
            &[
                (
                    "outer twin",
                    b"/// Docs.\nfn f() {\n    let x = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "line form",
                    b"fn f() {\n    //! Docs.\n    let x = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "block form",
                    b"fn f() {\n    /*! Docs. */\n    let x = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "after an inner attribute",
                    b"fn f() {\n    #![allow(unused)]\n    //! Docs.\n    let x = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "method",
                    b"struct S;\nimpl S {\n    fn m(&self) {\n        //! Docs.\n    }\n}\n",
                    1,
                    1,
                ),
                (
                    "nested function",
                    b"fn outer() {\n    fn inner() {\n        //! Docs.\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "loop body",
                    b"fn f() {\n    loop {\n        //! Docs.\n        break;\n    }\n}\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// Bodies that open a function space without being a function — a
    /// Java or Groovy `static { … }`, a Java instance initializer, a Kotlin
    /// accessor or `init` block — hold a doc comment as a method body
    /// does: javadoc, Groovydoc and Dokka read none of them (#1597).
    #[cfg(all(feature = "groovy", feature = "java", feature = "kotlin"))]
    #[test]
    fn initializer_and_accessor_bodies_hold_their_doc_comments() {
        assert_doc_rows(
            LANG::Java,
            &[
                (
                    "static initializer",
                    b"class C {\n    static {\n        /** note */\n        int x = 1;\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "instance initializer",
                    b"class C {\n    {\n        /** note */\n        int x = 1;\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "enum instance initializer",
                    b"enum E {\n    A;\n    {\n        /** note */\n        int x = 1;\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "switch rule in a field initializer",
                    b"class S {\n    int v = switch (1) {\n        case 1 -> {\n            /** note */\n            yield 1;\n        }\n        default -> 0;\n    };\n}\n",
                    0,
                    1,
                ),
            ],
        );
        assert_doc_rows(
            LANG::Groovy,
            &[(
                "static initializer",
                b"class C {\n    static {\n        /** note */\n        def x = 1\n    }\n}\n",
                0,
                1,
            )],
        );
        assert_doc_rows(
            LANG::Kotlin,
            &[
                (
                    "init block",
                    b"class K {\n    init {\n        /** note */\n        println(1)\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "getter",
                    b"class K {\n    val x: Int\n        get() {\n            /** note */\n            return 1\n        }\n}\n",
                    0,
                    1,
                ),
                (
                    "setter",
                    b"class K {\n    var x: Int = 0\n        set(value) {\n            /** note */\n            field = value\n        }\n}\n",
                    0,
                    1,
                ),
                (
                    "lambda literal, all body",
                    b"val l = {\n    /** note */\n    1\n}\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// The in-body rule for the marker languages the tables above leave
    /// out: Groovy holds an in-body doc comment as Java does; Objective-C
    /// and Mozilla's C++ keep it as C and C++ do (Doxygen's in-body
    /// description); Mozilla's JavaScript and TSX keep in-body JSDoc, and
    /// Perl keeps POD wherever it sits (#1597).
    #[cfg(all(
        feature = "groovy",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "perl",
        feature = "typescript",
    ))]
    #[test]
    fn in_body_rule_reaches_every_marker_language() {
        assert_doc_rows(
            LANG::Groovy,
            &[(
                "method body",
                b"class C {\n    /** Doc. */\n    def f() {\n        /** note */\n        def x = 1\n    }\n}\n",
                1,
                2,
            )],
        );
        assert_doc_rows(
            LANG::Objc,
            &[(
                "function body",
                b"/** Doc. */\nvoid f(void) {\n    /** note */\n    int x = 1;\n}\n",
                2,
                2,
            )],
        );
        assert_doc_rows(
            LANG::Mozcpp,
            &[(
                "function body",
                b"/// Doc.\nvoid f() {\n    /// note\n    int x = 1;\n}\n",
                2,
                2,
            )],
        );
        assert_doc_rows(
            LANG::Mozjs,
            &[(
                "function body",
                b"/** Doc. */\nfunction f() {\n    /** Inner. */\n    function g() {}\n}\n",
                2,
                2,
            )],
        );
        assert_doc_rows(
            LANG::Tsx,
            &[(
                "function body",
                b"/** Doc. */\nfunction f() {\n    /** Inner. */\n    const x = 1;\n}\n",
                2,
                2,
            )],
        );
        assert_doc_rows(
            LANG::Perl,
            &[(
                "sub body",
                b"sub f {\n    my $x = 1;\n\n=pod\n\nDoc.\n\n=cut\n\n    return $x;\n}\n",
                5,
                5,
            )],
        );
    }

    /// Sphinx autodoc's `#:` doc comments (#1597): consecutive `#:` lines
    /// directly above an assignment an attribute docstring may follow, or
    /// a `#:` after the code on its line. A plain `#` line breaks a block;
    /// a method other than `__init__` documents no attribute.
    #[cfg(feature = "python")]
    #[test]
    fn python_sphinx_doc_comments_document_attributes() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("module attribute", b"#: Max size.\nMAX = 10\n", 1, 1),
            (
                "two-line block",
                b"#: Max size,\n#: in bytes.\nMAX = 10\n",
                2,
                2,
            ),
            ("same line", b"MAX = 10  #: Max size.\n", 1, 1),
            (
                "class attribute as the first statement",
                b"class C:\n    #: Doc.\n    a = 1\n",
                1,
                1,
            ),
            (
                "class attribute later in the body",
                b"class C:\n    a = 1\n    #: Doc.\n    b = 2\n",
                1,
                1,
            ),
            (
                "self attribute in __init__",
                b"class C:\n    def __init__(self):\n        #: Doc.\n        self.x = 1\n",
                1,
                1,
            ),
            (
                "plain line breaks the block",
                b"#: Lost.\n# plain\nMAX = 10\n",
                0,
                2,
            ),
            (
                "plain line above the block",
                b"# plain\n#: Doc.\nMAX = 10\n",
                1,
                2,
            ),
            (
                "blank row before the assignment",
                b"#: Doc.\n\nMAX = 10\n",
                0,
                1,
            ),
            ("above a call", b"#: Doc.\nprint(1)\n", 0, 1),
            ("same line after a call", b"print(1)  #: Doc.\n", 0, 1),
            // PEP 257 reads the top level only; Sphinx also reads an `if`,
            // `with`, loop or `try` suite there, and this follows the PEP.
            (
                "in an if suite",
                b"if True:\n    #: Not read.\n    X = 1\n",
                0,
                1,
            ),
            (
                "docstring in an if suite",
                b"if True:\n    X = 1\n    \"\"\"Not read.\"\"\"\n",
                0,
                1,
            ),
            (
                "in a method other than __init__",
                b"class C:\n    def f(self):\n        #: Doc.\n        self.x = 1\n",
                0,
                1,
            ),
            (
                "local in a function",
                b"def f():\n    #: Doc.\n    x = 1\n",
                0,
                1,
            ),
            (
                "plain comment above an assignment",
                b"# plain\nMAX = 10\n",
                0,
                1,
            ),
            // Nothing follows the block, so it documents nothing.
            ("last line of the file", b"X = 1\n#: Dangling.\n", 0, 1),
        ];
        assert_doc_rows(LANG::Python, cases);
    }

    /// PEP 257's attribute docstring after `self.<attr> = …` at the top of
    /// `__init__`, which Sphinx's own autodoc example documents — alone or
    /// in a tuple or list of targets, as Sphinx's `get_lvar_names` expands
    /// one, and whatever comment a formatter puts after the `(` of an
    /// exploded signature (#1597). An assignment in an `if` suite is not at
    /// the top level PEP 257 reads; Sphinx reads those suites too, and this
    /// follows the PEP.
    #[cfg(feature = "python")]
    #[test]
    fn python_init_attribute_docstring_is_documentation() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            (
                "single target",
                b"class C:\n    def __init__(self):\n        self.x = 1\n        \"\"\"X doc.\"\"\"\n",
                1,
                1,
            ),
            (
                "tuple of targets",
                b"class C:\n    def __init__(self):\n        self.a, self.b = 1, 2\n        \"\"\"A and b.\"\"\"\n",
                1,
                1,
            ),
            (
                "list of targets",
                b"class C:\n    def __init__(self):\n        [self.a, self.b] = 1, 2\n        \"\"\"A and b.\"\"\"\n",
                1,
                1,
            ),
            // The receiver is read through a `typed_parameter`.
            (
                "typed self",
                b"class C:\n    def __init__(self: \"C\", x: int):\n        self.x = x\n        \"\"\"X doc.\"\"\"\n",
                1,
                1,
            ),
            (
                "#: above a tuple of targets",
                b"class C:\n    def __init__(self):\n        #: A and b.\n        self.a, self.b = 1, 2\n",
                1,
                1,
            ),
            (
                "a comment after the open paren",
                b"class C:\n    def __init__(  # noqa: PLR0913\n        self, x\n    ):\n        self.q = x\n        \"\"\"Q doc.\"\"\"\n",
                1,
                2,
            ),
            (
                "in an if suite",
                b"class C:\n    def __init__(self, debug):\n        if debug:\n            self.level = 1\n            \"\"\"Not read.\"\"\"\n",
                0,
                1,
            ),
        ];
        assert_doc_rows(LANG::Python, cases);
    }

    /// A PEP 695 `type` alias is documented like an assignment, as Sphinx's
    /// `VariableCommentPicker` reads `ast.TypeAlias`: by a docstring after
    /// it or a `#:` comment on or above it, at module or class level only
    /// (#1597).
    #[cfg(feature = "python")]
    #[test]
    fn python_type_alias_documentation_follows_sphinx() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("docstring after", b"type X = int\n\"\"\"X doc.\"\"\"\n", 1, 1),
            ("#: above a generic alias", b"#: Y doc.\ntype Y[T] = list[T]\n", 1, 1),
            ("#: on its line", b"type X = int  #: X doc.\n", 1, 1),
            (
                "class level",
                b"class C:\n    type Z = int\n    \"\"\"Z doc.\"\"\"\n",
                1,
                1,
            ),
            (
                "function body",
                b"def f():\n    x = 1\n    type X = int\n    \"\"\"Not read.\"\"\"\n",
                0,
                1,
            ),
            (
                "__init__ body",
                b"class C:\n    def __init__(self):\n        type X = int\n        \"\"\"Not read.\"\"\"\n",
                0,
                1,
            ),
        ];
        assert_doc_rows(LANG::Python, cases);
    }

    /// An attribute docstring's target test walks the assignment target
    /// with an explicit stack: a target nested thousands of brackets deep
    /// (rejected by CPython past 200, parsed by the grammar) overflowed the
    /// walker's stack and aborted the process (#1597).
    #[cfg(feature = "python")]
    #[test]
    fn python_deep_attribute_target_uses_a_bounded_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let depth = 8_000;
                let target = format!("{}a{}", "(".repeat(depth), ",)".repeat(depth));
                for source in [
                    format!("{target} = 1\n\"\"\"Documentation.\"\"\"\n"),
                    format!("#: Documentation.\n{target} = 1\n"),
                ] {
                    let loc = metrics_verbatim(LANG::Python, source.as_bytes(), loc_only()).loc;
                    assert_eq!((loc.sloc(), loc.cloc(), loc.dloc()), (2, 1, 1));
                }
            })
            .expect("spawn the bounded-stack metric walk")
            .join()
            .expect("the documented deep target must not exhaust the stack");
    }
}
