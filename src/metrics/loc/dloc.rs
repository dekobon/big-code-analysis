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
/// into both, and `add_leading_doc_run` (Go, Ruby, Lua),
/// `add_bound_doc_run` (the doc comments Rust, Java, Kotlin, Groovy, C#,
/// PHP and the JS family attach to the declaration after them, #1613)
/// and Python's
/// `add_sphinx_doc_comment` (`#:` comments) re-bill only rows a comment
/// arm already recorded. A language without a
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
        feature = "go",
        feature = "groovy",
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
        /// comment must follow `<?php`, and which is then the file's only
        /// DocBlock and so phpDocumentor's file DocBlock.
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
            (LANG::Php, "<?php"),
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
    /// row at once; `dloc_without_code` leaves it in the denominator of
    /// `mi.sei`'s comment ratio,
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
        // Java, JavaScript, PHP, C# and C bind a doc comment to the
        // declaration after it (#1613), so their fixtures put one there.
        let fixtures: &[(LANG, &[u8], u64)] = &[
            (LANG::C, b"int x; /** d */ int y;\n", 1),
            (LANG::Java, b"class C { int x; /** d */ int y; }\n", 1),
            (LANG::Javascript, b"let x = 1; /** d */ let y = 2;\n", 1),
            (LANG::Php, b"<?php $x = 1; /** d */ function f() {}\n", 1),
            // The `///` form consumes its newline into the `doc_comment`
            // child, the #1051 end-row discount path; the item after it
            // keeps the fixture valid Rust.
            (LANG::Rust, b"fn f() {} /// d\nfn g() {}\n", 2),
            (LANG::Csharp, b"class C { int x; /// d\nint y; }\n", 2),
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
                "{lang:?} {text:?}: the shared row stays a physical row"
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
        feature = "go",
        feature = "groovy",
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
        feature = "typescript",
    ))]
    fn assert_doc_rows(lang: LANG, cases: &[(&str, &[u8], u64, u64)]) {
        for (name, source, dloc, cloc) in cases {
            let loc = metrics_verbatim(lang, source, loc_only()).loc;
            assert_eq!((loc.dloc(), loc.cloc()), (*dloc, *cloc), "{lang:?}: {name}");
        }
    }

    /// go/doc's lead comment: a comment group on its own rows, directly
    /// above a declaration, with no blank row between. Each fixture
    /// pins the rows it credits and the rows it must not.
    #[cfg(feature = "go")]
    #[test]
    fn go_lead_comments_are_documentation() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            (
                "block above func",
                b"package p\n\n/* d */\nfunc f() {}\n",
                1,
                1,
            ),
            (
                "two-row group",
                b"package p\n\n// a\n// b\nfunc f() {}\n",
                2,
                2,
            ),
            (
                "package doc",
                b"// Package p does things.\npackage p\n",
                1,
                1,
            ),
            (
                "trailing then doc",
                b"package p\n\nvar x = 1 // t\n// d\nfunc f() {}\n",
                1,
                2,
            ),
            (
                "directive row not counted",
                b"package p\n\n//go:generate go run x\n// d\nfunc f() {}\n",
                1,
                2,
            ),
            (
                "cgo export directive not counted",
                b"package p\n\n// F doc\n//export F\nfunc F() {}\n",
                1,
                2,
            ),
            (
                "struct field",
                b"package p\n\ntype T struct {\n\t// F doc\n\tF int\n}\n",
                1,
                1,
            ),
            (
                "interface method",
                b"package p\n\ntype I interface {\n\t// M doc\n\tM()\n}\n",
                1,
                1,
            ),
            (
                "embedded interface element",
                b"package p\n\nimport \"io\"\n\ntype I interface {\n\t// R doc\n\tio.Reader\n}\n",
                1,
                1,
            ),
            (
                "tilde constraint element",
                b"package p\n\ntype C interface {\n\t// x\n\t~int | ~string\n}\n",
                0,
                1,
            ),
            (
                "method with receiver",
                b"package p\n\ntype T int\n\n// M doc\nfunc (t T) M() {}\n",
                1,
                1,
            ),
            (
                "grouped type spec",
                b"package p\n\ntype (\n\t// A doc\n\tA int\n\t// B doc\n\tB int\n)\n",
                2,
                2,
            ),
            (
                "grouped const spec",
                b"package p\n\nconst (\n\t// A doc\n\tA = 1\n)\n",
                1,
                1,
            ),
            (
                "grouped type alias",
                b"package p\n\ntype (\n\t// B doc\n\tB = int\n)\n",
                1,
                1,
            ),
            (
                // Statements go/doc never reads, whether the comment is
                // parked outside the body's `statement_list` or not.
                "local declarations",
                b"package p\n\nfunc f() int {\n\t// a\n\tvar x = 1\n\t// b\n\tconst y = 2\n\t// c\n\ttype t int\n\treturn x + y\n}\n",
                0,
                3,
            ),
            (
                "local grouped spec",
                b"package p\n\nfunc f() {\n\tvar (\n\t\t// a\n\t\tx = 1\n\t)\n\t_ = x\n}\n",
                0,
                1,
            ),
            (
                "field of a local type",
                b"package p\n\nfunc f() {\n\ttype t struct {\n\t\t// F\n\t\tF int\n\t}\n\t_ = t{}\n}\n",
                0,
                1,
            ),
            (
                "blank-separated",
                b"package p\n\n// not doc\n\nfunc f() {}\n",
                0,
                1,
            ),
            (
                "blank inside the group",
                b"package p\n\n// part 1\n\n// part 2\nfunc f() {}\n",
                1,
                2,
            ),
            (
                "trailing field comment",
                b"package p\n\ntype T struct {\n\tF int // t\n}\n",
                0,
                1,
            ),
            (
                "comment in a body",
                b"package p\n\nfunc f() {\n\t// c\n\treturn\n}\n",
                0,
                1,
            ),
            (
                "dangling before a brace",
                b"package p\n\nfunc f() {\n\treturn\n\t// c\n}\n",
                0,
                1,
            ),
            (
                "build constraint then blank",
                b"//go:build linux\n\npackage p\n",
                0,
                1,
            ),
            (
                "import doc",
                b"package p\n\n// not doc\nimport \"fmt\"\n\nvar _ = fmt.Sprint\n",
                0,
                1,
            ),
            (
                "doc and func on one row",
                b"package p\n\n/* d */ func f() {}\n",
                0,
                1,
            ),
            (
                // go/parser groups every comment on the run's rows.
                "two comments on the run's last row",
                b"package p\n\n// G does a thing.\n/* Since v2. */ /* Stable. */\nfunc G() {}\n",
                2,
                2,
            ),
            (
                "a trailing pair",
                b"package p\n\nvar x = 1 /* a */ /* b */\nfunc f() {}\n",
                0,
                1,
            ),
            (
                // A comment above an instantiation's type argument documents
                // nothing, though the grammar spells each one `type_elem`.
                "type arguments of an instantiation",
                b"package p\n\ntype Pair = Map[\n\t// key type\n\tstring,\n\t// value type\n\tint,\n]\n",
                0,
                2,
            ),
        ];
        assert_doc_rows(LANG::Go, cases);
    }

    /// YARD's convention: the `#` lines directly above a `def`, `class`,
    /// `module`, attribute or visibility-wrapped `def`. The first
    /// statement of a body has its comment outside the `body_statement`
    /// wrapper, which the target test looks through.
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_leading_comments_are_documentation() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            ("above def", b"# d\ndef m; end\n", 1, 1),
            ("two rows above def", b"# a\n# b\ndef m; end\n", 2, 2),
            ("above def self", b"# d\ndef self.m; end\n", 1, 1),
            ("above class", b"# d\nclass C; end\n", 1, 1),
            ("above module", b"# d\nmodule M; end\n", 1, 1),
            (
                "first method in a class body",
                b"class C\n  # d\n  def m; end\nend\n",
                1,
                1,
            ),
            (
                "second method in a class body",
                b"class C\n  def a; end\n  # d\n  def b; end\nend\n",
                1,
                1,
            ),
            (
                "nested class",
                b"module M\n  # d\n  class C; end\nend\n",
                1,
                1,
            ),
            (
                "attr_reader",
                b"class C\n  # d\n  attr_reader :x\nend\n",
                1,
                1,
            ),
            ("plain attr", b"class C\n  # d\n  attr :x\nend\n", 1, 1),
            ("top-level constant", b"# Max.\nMAX = 10\n", 1, 1),
            (
                "class constant",
                b"class C\n  # Lim.\n  LIMIT = 3\nend\n",
                1,
                1,
            ),
            ("scoped constant", b"# Bar.\nFoo::BAR = 1\n", 1, 1),
            (
                "alias",
                b"class C\n  def a; end\n  # Old name.\n  alias b a\nend\n",
                1,
                1,
            ),
            (
                "alias_method",
                b"class C\n  def a; end\n  # Old name.\n  alias_method :b, :a\nend\n",
                1,
                1,
            ),
            ("local variable is not a target", b"# c\nx = 1\n", 0, 1),
            (
                "instance variable is not a target",
                b"class C\n  def a\n    # c\n    @x = 1\n  end\nend\n",
                0,
                1,
            ),
            (
                "alias in a method body",
                b"class C\n  def a\n    # c\n    alias b a\n  end\nend\n",
                0,
                1,
            ),
            (
                "private def",
                b"class C\n  # d\n  private def m; end\nend\n",
                1,
                1,
            ),
            ("=begin block", b"=begin\nd\n=end\ndef m; end\n", 3, 3),
            ("blank-separated", b"# not doc\n\ndef m; end\n", 0, 1),
            (
                "bare private between",
                b"class C\n  # c\n  private\n  def m; end\nend\n",
                0,
                1,
            ),
            // `foo.()` is a call with no `method` field, so it names
            // nothing YARD documents.
            ("call shorthand", b"class C\n  # c\n  foo.()\nend\n", 0, 1),
            ("trailing", b"x = 1 # c\ndef m; end\n", 0, 1),
            ("dangling", b"def m\n  # c\nend\n", 0, 1),
            (
                "shebang and magic rows above a class",
                b"#!/usr/bin/env ruby\n# frozen_string_literal: true\n# d\nclass C; end\n",
                1,
                3,
            ),
            (
                "a doc row that mentions an encoding",
                b"class C\n  # Converts.\n  # Encoding: defaults to UTF-8.\n  def m; end\nend\n",
                2,
                2,
            ),
            (
                "visibility call on a symbol",
                b"class C\n  def a; end\n  # c\n  private :a\nend\n",
                0,
                1,
            ),
            (
                "visibility call on another object",
                b"class C\n  x = 1\n  # c\n  @cfg.private def m; end\nend\n",
                0,
                1,
            ),
            (
                "public_class_method def",
                b"class C\n  x = 1\n  # d\n  public_class_method def self.build; end\nend\n",
                1,
                1,
            ),
            ("comment above a statement", b"# c\nputs 1\n", 0, 1),
            // YARD's method handler is the one that runs in a method body
            // too: a nested `def` is documented, while a constant, alias,
            // attribute, visibility call, singleton class or conditional
            // there is not read at all.
            (
                "def nested in a method body",
                b"def bar\n  x = 1\n  # c\n  def helper; end\nend\n",
                1,
                1,
            ),
            (
                "def first in a method body",
                b"def bar\n  # c\n  def helper; end\nend\n",
                1,
                1,
            ),
            (
                "def in a conditional in a method body",
                b"def m\n  if true\n    x = 1\n    # c\n    def helper; end\n  end\nend\n",
                0,
                1,
            ),
            (
                "def in a singleton class opened in a method body",
                b"class Foo\n  def setup\n    class << self\n      # c\n      def m; end\n    end\n  end\nend\n",
                0,
                1,
            ),
            (
                "def in a class-level conditional",
                b"class C\n  if true\n    x = 1\n    # d\n    def m; end\n  end\nend\n",
                1,
                1,
            ),
            // YARD registers a class for `class << Const` only.
            (
                "singleton class of self",
                b"class C\n  # c\n  class << self\n    def m; end\n  end\nend\n",
                0,
                1,
            ),
            (
                "singleton class of a constant",
                b"# d\nclass << D\n  def m; end\nend\n",
                1,
                1,
            ),
            (
                "visibility call on self",
                b"class C\n  x = 1\n  # c\n  self.private def m; end\nend\n",
                0,
                1,
            ),
            (
                "constant guarded by a modifier",
                b"module M\n  # Version.\n  VERSION = \"1.0\" unless defined?(VERSION)\nend\n",
                1,
                1,
            ),
            (
                "def with a modifier",
                b"class C\n  x = 1\n  # d\n  def m; end if true\nend\n",
                1,
                1,
            ),
            (
                "global variable alias",
                b"class C\n  x = 1\n  # c\n  alias $new $old\nend\n",
                0,
                1,
            ),
            ("class variable", b"class C\n  # Count.\n  @@count = 0\nend\n", 1, 1),
        ];
        assert_doc_rows(LANG::Ruby, cases);
    }

    /// `BodyStatement2` is tree-sitter-ruby's hidden `_body_statement`:
    /// `ruby_documented_statement` (behind `is_doc_target`), `Npa` and
    /// `Npm` keep a defensive arm for it, and
    /// this pins that the grammar still never emits it, so the arms stay
    /// defensive rather than silently live (grammar-dispatch rule 2).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_hidden_body_statement_alias_is_not_emitted() {
        use crate::Ruby;
        use crate::test_support::ast_has_kind_id;
        let source = b"class C\n  # d\n  def m; end\nend\nmodule M\n  x = 1\nend\n";
        let any = AnyParser::parse(LANG::Ruby, source.to_vec(), None, None)
            .expect("the ruby feature parses");
        let (visible, hidden) = with_any_parser!(&any, |p| (
            ast_has_kind_id(p, Ruby::BodyStatement as u16),
            ast_has_kind_id(p, Ruby::BodyStatement2 as u16)
        ));
        assert!(
            visible,
            "the fixture no longer produces `body_statement`; the negative claim below is then vacuous"
        );
        assert!(
            !hidden,
            "tree-sitter-ruby now emits the hidden `_body_statement`; its arms are live"
        );
    }

    /// LDoc: a block opens on `---` (a dash-only row too) and runs over
    /// the `--` rows below it until a blank row; `--- x ---` is an end
    /// marker, not an opener, and a `--[[` long comment opens only when
    /// its body starts with a dash — and always ends the block it is in.
    #[cfg(feature = "lua")]
    #[test]
    fn lua_ldoc_blocks_are_documentation() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            (
                "opener then continuation",
                b"--- Summary.\n-- Description.\n-- @param x\nlocal function f(x) end\n",
                3,
                3,
            ),
            (
                "dash-only opener",
                b"-----\n-- Module summary\nlocal x = 1\n",
                2,
                2,
            ),
            ("single opener row", b"--- d\nlocal x = 1\n", 1, 1),
            ("long doc comment", b"--[[-- d ]]\nlocal x = 1\n", 1, 1),
            (
                "levelled long doc comment",
                b"--[==[- d ]==]\nlocal x = 1\n",
                1,
                1,
            ),
            ("plain rows", b"-- a\n-- b\nlocal x = 1\n", 0, 2),
            (
                "end marker is not an opener",
                b"--- x ---\nlocal x = 1\n",
                0,
                1,
            ),
            ("plain long comment", b"--[[ c ]]\nlocal x = 1\n", 0, 1),
            (
                "blank row ends the block",
                b"--- d\n\n-- c\nlocal x = 1\n",
                1,
                2,
            ),
            ("trailing opener", b"local y = 1 --- d\n", 0, 1),
            (
                "opener after a plain row",
                b"-- a\n--- b\nlocal x = 1\n",
                0,
                2,
            ),
            (
                "a long comment ends the block above it",
                b"--[[ License: MIT ]]\n--- Module doc.\n-- More.\nlocal M = {}\n",
                2,
                3,
            ),
            (
                "a long doc comment stands alone",
                b"--[[-- Summary. ]]\n-- not part of it\nlocal x = 1\n",
                1,
                2,
            ),
            (
                "two comments on one row",
                b"--- Doc for g.\n--[[ note ]] -- see\nfunction g() end\n",
                2,
                2,
            ),
        ];
        assert_doc_rows(LANG::Lua, cases);
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_deep_documented_assignment_uses_a_bounded_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let source = format!("--- Documentation.\n{}x = 1\n", "a.".repeat(8_000));
                let loc = metrics_verbatim(LANG::Lua, source.as_bytes(), loc_only()).loc;
                assert_eq!(
                    (loc.sloc(), loc.ploc(), loc.cloc(), loc.dloc()),
                    (2, 1, 1, 1)
                );
            })
            .expect("spawn the bounded-stack metric walk")
            .join()
            .expect("the documented assignment must not exhaust the stack");
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
                (
                    "property after an attribute",
                    b"<?php\nclass X {\n    #[Attr]\n    /** Doc. */\n    public int $p = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "class constant after an attribute",
                    b"<?php\nclass X {\n    #[Attr]\n    /** Doc. */\n    const C = 1;\n}\n",
                    1,
                    1,
                ),
                (
                    "enum case after an attribute",
                    b"<?php\nenum E {\n    #[Attr]\n    /** Doc. */\n    case A;\n}\n",
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

    /// A C# doc comment in a property's or indexer's accessor list, in the
    /// member's header, or after its attribute documents nothing: Roslyn
    /// reports each as CS1587, since an accessor takes no doc comment of its
    /// own (`DocumentationCommentCompiler.ShouldSkip`). #1612 counted the
    /// accessor-list one; #1613 reads it as `dotnet build` does.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_property_doc_comments_document_nothing_inside_the_member() {
        assert_doc_rows(
            LANG::Csharp,
            &[
                (
                    "accessor list",
                    b"class C {\n    int X {\n        /// a\n        get;\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "header, bodied",
                    b"class C {\n    int X\n    /// a\n    { get; }\n}\n",
                    0,
                    1,
                ),
                (
                    "after an attribute, bodied",
                    b"class C {\n    [A]\n    /// a\n    int X { get; }\n}\n",
                    0,
                    1,
                ),
                (
                    "header, expression-bodied",
                    b"class C {\n    int X\n    /// a\n    => 1;\n}\n",
                    0,
                    1,
                ),
                (
                    "indexer header",
                    b"class C {\n    int this[int i]\n    /// a\n    { get { return 1; } }\n}\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// A PHPDoc between a function's signature and its body, or between
    /// an arrow function's `=>` and its expression, is outside the body
    /// and documents; one inside the body does not (#1612).
    #[cfg(feature = "php")]
    #[test]
    fn php_doc_comments_before_the_body_document() {
        assert_doc_rows(
            LANG::Php,
            &[
                ("signature", b"<?php\nfunction f()\n/** s */\n{ }\n", 1, 1),
                (
                    "body",
                    b"<?php\nfunction f() {\n/** s */\nreturn 1; }\n",
                    0,
                    1,
                ),
                (
                    "arrow signature",
                    b"<?php\n$f = fn()\n/** t */\n=> 1;\n",
                    1,
                    1,
                ),
                (
                    "after an arrow's =>",
                    b"<?php\n$f = fn() => /** t */ 1;\n",
                    1,
                    1,
                ),
                (
                    "arrow body",
                    b"<?php\n$f = fn() => [\n/** t */\n1];\n",
                    0,
                    1,
                ),
                (
                    "before a missing arrow body",
                    b"<?php\n$f = fn() => /** t */;\n",
                    1,
                    1,
                ),
                (
                    "closure use clause",
                    b"<?php\n$k = function($x)\nuse ($a) /** w */ { };\n",
                    1,
                    1,
                ),
            ],
        );
    }

    /// A KDoc between a function's or accessor's signature and its body
    /// documents; one in a block or expression body does not (#1612).
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_doc_comments_before_the_body_document() {
        assert_doc_rows(
            LANG::Kotlin,
            &[
                ("signature", b"fun f()\n/** s */\n{ }\n", 1, 1),
                ("block body", b"fun f() {\n/** s */\n}\n", 0, 1),
                ("expression body", b"fun g() = /** s */ 1\n", 0, 1),
                (
                    "getter head",
                    b"val x: Int\n/** z */ get() { return 1 }\n",
                    1,
                    1,
                ),
                (
                    "secondary constructor",
                    b"class C {\n    constructor(x: Int)\n    /** s */\n    { }\n}\n",
                    1,
                    1,
                ),
                (
                    "secondary constructor body",
                    b"class C {\n    constructor(x: Int) {\n        /** s */\n    }\n}\n",
                    0,
                    1,
                ),
            ],
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
                // A closure body has a `body` field too; only a
                // `function_item` takes an inner doc comment. Outside any
                // function, so no enclosing body holds it regardless.
                (
                    "closure body",
                    b"const G: fn() = || {\n    //! Docs.\n};\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// rustc makes every outer doc comment above an item one of its
    /// attributes — across blank rows, plain comments and its other
    /// attributes — and lints one on anything else as unused: an extern
    /// block, a macro call, a statement, a generic parameter, or nothing
    /// at the end of the file (#1613). The stretch's rows are pinned
    /// against what `rustdoc` renders and `rustc -W unused_doc_comments`
    /// reports. `//!` and a doc comment in a macro's token tree keep the
    /// unbound reading.
    #[cfg(feature = "rust")]
    #[test]
    fn rust_doc_comments_bind_to_the_item_after_them() {
        assert_doc_rows(
            LANG::Rust,
            &[
                ("direct", b"/// d\nfn f() {}\n", 1, 1),
                (
                    "through an attribute",
                    b"/// d\n#[inline]\n/// e\nfn f() {}\n",
                    2,
                    2,
                ),
                ("across a blank row", b"/// d\n\nfn f() {}\n", 1, 1),
                ("two runs, both kept", b"/// a\n\n/// b\nfn f() {}\n", 2, 2),
                ("across a plain comment", b"/// a\n// p\nfn f() {}\n", 1, 2),
                ("block form", b"/** a */\nfn f() {}\n", 1, 1),
                ("use declaration", b"/// d\nuse std::fmt;\n", 1, 1),
                (
                    "tuple field",
                    b"struct T(\n    /// d\n    pub u8,\n);\n",
                    1,
                    1,
                ),
                ("enum variant", b"enum E {\n    /// d\n    A,\n}\n", 1, 1),
                ("at the end of the file", b"fn f() {}\n/// d\n", 0, 1),
                ("before a closing brace", b"mod m {\n    /// d\n}\n", 0, 1),
                ("macro call", b"/// d\nm!();\n", 0, 1),
                (
                    "item in a function body",
                    b"fn f() {\n    /// d\n    fn g() {}\n}\n",
                    0,
                    1,
                ),
                ("macro item", b"/// d\nm! { x }\n", 0, 1),
                ("extern block", b"/// d\nunsafe extern \"C\" {}\n", 0, 1),
                // tree-sitter-rust parses Rust 2024's `safe` as an error
                // before the rest of the item, which the stretch reads past.
                (
                    "safe foreign fn",
                    b"unsafe extern \"C\" {\n    /// d\n    safe fn g();\n}\n",
                    1,
                    1,
                ),
                (
                    "generic parameter",
                    b"struct P<\n    /// d\n    T,\n>(T);\n",
                    0,
                    1,
                ),
                ("inner doc comment", b"//! d\nfn f() {}\n", 1, 1),
                (
                    "in a macro rule",
                    b"macro_rules! m {\n    () => {\n        /// d\n        fn g() {}\n    };\n}\n",
                    1,
                    1,
                ),
            ],
        );
    }

    /// javac gives a declaration the last doc comment before its first
    /// token, annotations and modifiers included, and its
    /// dangling-doc-comments lint names every other one (#1613): an earlier
    /// doc comment in the same stretch, one above an import or a closing
    /// brace, one after an annotation. The package declaration takes the
    /// file's first doc comment, which javadoc reads only from
    /// `package-info.java`, and which javac calls dangling once a type
    /// follows. Each row was checked with `javac -Xlint:dangling-doc-comments`
    /// and javac's own `DocTrees::getDocComment`.
    #[cfg(feature = "java")]
    #[test]
    fn java_doc_comments_bind_to_the_declaration_after_them() {
        assert_doc_rows(
            LANG::Java,
            &[
                ("direct", b"/** d */\nclass C {}\n", 1, 1),
                (
                    "through an annotation",
                    b"/** d */\n@Deprecated\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "across a blank row",
                    b"class C {\n    /** d */\n\n    int x;\n}\n",
                    1,
                    1,
                ),
                (
                    "across a plain comment",
                    b"class C {\n    /** d */\n    // p\n    int x;\n}\n",
                    1,
                    2,
                ),
                ("superseded", b"/** a */\n/** b */\nclass C {}\n", 1, 2),
                ("markdown lines join", b"/// a\n/// b\nclass C {}\n", 2, 2),
                ("markdown runs split", b"/// a\n\n/// b\nclass C {}\n", 1, 2),
                (
                    "after an annotation",
                    b"class C {\n    @Deprecated\n    /** d */\n    int x;\n}\n",
                    0,
                    1,
                ),
                ("enum constant", b"enum E {\n    /** d */\n    A\n}\n", 1, 1),
                (
                    "import",
                    b"/** d */\nimport java.util.List;\nclass C {}\n",
                    0,
                    1,
                ),
                ("at the end of the file", b"class C {}\n/** d */\n", 0, 1),
                (
                    "before a closing brace",
                    b"class C {\n    int x;\n    /** d */\n}\n",
                    0,
                    1,
                ),
                (
                    "before an initializer",
                    b"class C {\n    /** d */\n    static {}\n}\n",
                    0,
                    1,
                ),
                (
                    "local class in a method body",
                    b"class C {\n    void m() {\n        /** d */\n        class L {}\n    }\n}\n",
                    0,
                    1,
                ),
                (
                    "package with a type after it",
                    b"/** d */\npackage p;\nclass C {}\n",
                    0,
                    1,
                ),
                (
                    "package with a type after its imports",
                    b"/** d */\npackage p;\nimport a.B;\n;\nclass C {}\n",
                    0,
                    1,
                ),
                (
                    "package-info.java",
                    b"/** a */\n/** d */\npackage p;\n",
                    1,
                    2,
                ),
                // A parse error between two doc comments is read past,
                // not read as the member's first token: javac takes `b`,
                // whichever of the two spans more rows.
                (
                    "error between a multi-row and a one-row doc",
                    b"class C {\n/**\n * a\n */\n%%%\n/** b */\nvoid f() {}\n}\n",
                    1,
                    4,
                ),
                (
                    "error between a one-row and a multi-row doc",
                    b"class C {\n/** a */\n%%%\n/**\n * b\n */\nvoid f() {}\n}\n",
                    3,
                    4,
                ),
            ],
        );
    }

    /// Kotlin's parser closes each declaration with
    /// `PrecedingDocCommentsBinder`, which takes the last KDoc of the
    /// comments before it; an earlier one, one above the package or an
    /// import, and one at the end of a body bind to nothing (#1613). A KDoc
    /// among a declaration's own children — its modifier list's children
    /// included, which is why one after an annotation binds — is
    /// `findDocComment`'s and stays as #1612 decided it; one nested deeper,
    /// in an argument list, a type parameter list or a `when` body, binds
    /// forward like any other. tree-sitter-kotlin parses a top-level
    /// annotation with arguments as an expression beside its declaration,
    /// and a top-level `context(…)` receiver as a call, so neither KDoc
    /// reaches the declaration.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_doc_comments_bind_to_the_declaration_after_them() {
        assert_doc_rows(
            LANG::Kotlin,
            &[
                ("direct", b"/** d */\nfun f() {}\n", 1, 1),
                // After a declaration the grammar splits the annotation off
                // as an expression; at the top of the file it does not.
                (
                    "through a split annotation",
                    b"fun g() {}\n/** d */\n@Suppress(\"x\")\nfun f() {}\n",
                    1,
                    1,
                ),
                // `findDocComment` reads a KDoc in the modifier list when
                // none precedes the declaration. The grammar splits the
                // annotation off only with code after the declaration.
                (
                    "after a split annotation",
                    b"fun g() {}\n@Suppress(\"x\")\n/** d */\n\nfun f() {}\nval y = 2\n",
                    1,
                    1,
                ),
                (
                    "before and after a split annotation",
                    b"fun g() {}\n/** a */\n@Suppress(\"x\")\n/** b */\n\nfun f() {}\nval y = 2\n",
                    1,
                    2,
                ),
                ("across a blank row", b"/** d */\n\nfun f() {}\n", 1, 1),
                (
                    "across a plain comment",
                    b"/** d */\n// p\nfun f() {}\n",
                    1,
                    2,
                ),
                ("superseded", b"/** a */\n/** b */\nfun f() {}\n", 1, 2),
                (
                    "enum entry",
                    b"enum class E {\n    /** d */\n    A\n}\n",
                    1,
                    1,
                ),
                ("package", b"/** d */\npackage p\n", 0, 1),
                ("import", b"/** d */\nimport a.B\n", 0, 1),
                ("at the end of the file", b"fun f() {}\n/** d */\n", 0, 1),
                (
                    "before a closing brace",
                    b"class C {\n    fun f() {}\n    /** d */\n}\n",
                    0,
                    1,
                ),
                (
                    "before an init block",
                    b"class C {\n    /** d */\n    init {}\n}\n",
                    0,
                    1,
                ),
                (
                    "local function in a body",
                    b"fun f() {\n    /** d */\n    fun g() {}\n}\n",
                    0,
                    1,
                ),
                (
                    "after an annotation",
                    b"@Deprecated(\"x\")\n/** d */\nfun f() {}\n",
                    1,
                    1,
                ),
                // Nested below a declaration's own children, where
                // `findDocComment` never looks.
                (
                    "call argument in an initializer",
                    b"val x = listOf(\n    /** d */\n    1,\n)\n",
                    0,
                    1,
                ),
                (
                    "annotation argument",
                    b"@Suppress(/** d */ \"a\")\nfun f() {}\n",
                    0,
                    1,
                ),
                ("type parameter", b"fun </** d */ T> f() {}\n", 0, 1),
                (
                    "when branch",
                    b"fun f(x: Int) = when (x) {\n    /** d */\n    1 -> 2\n    else -> 3\n}\n",
                    0,
                    1,
                ),
                ("named argument", b"val y = foo(/** d */ a = 1)\n", 0, 1),
                // In a class body the grammar parses a context receiver as
                // an error around an enum entry, which stands in for the
                // declaration it begins; at the top level it is a call.
                (
                    "context receiver in a class body",
                    b"class C {\n    /** d */\n    context(Foo)\n    fun f() {}\n}\n",
                    1,
                    1,
                ),
                (
                    "context receiver at the top level",
                    b"/** d */\ncontext(Foo)\nfun f() {}\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// Groovy's `GroovydocManager` reads the last comment token before a
    /// declaration and keeps it only if it opens `/**`: a plain comment in
    /// between detaches the Groovydoc, but a block comment that code
    /// follows on its row is on the lexer's hidden channel and is read
    /// past, and a package declaration resets nothing (#1613). Each row
    /// was checked against Groovy 4.0.32's own `AstBuilder`.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_doc_comments_bind_to_the_declaration_after_them() {
        assert_doc_rows(
            LANG::Groovy,
            &[
                ("direct", b"/** d */\nclass C {}\n", 1, 1),
                (
                    "across a blank row",
                    b"class C {\n    /** d */\n\n    int x\n}\n",
                    1,
                    1,
                ),
                (
                    "across a plain comment",
                    b"class C {\n    /** d */\n    // p\n    def f() {}\n}\n",
                    0,
                    2,
                ),
                (
                    "across a hidden block",
                    b"class C {\n    /** d */\n    /* h */ def f() {}\n}\n",
                    1,
                    2,
                ),
                (
                    "across a block on its own row",
                    b"class C {\n    /** d */\n    /* h */\n    def f() {}\n}\n",
                    0,
                    2,
                ),
                ("superseded", b"/** a */\n/** b */\nclass C {}\n", 1, 2),
                (
                    "through the package",
                    b"/** d */\npackage p\nclass C {}\n",
                    1,
                    1,
                ),
                ("enum constant", b"enum E {\n    /** d */\n    A\n}\n", 1, 1),
                ("import", b"/** d */\nimport java.util.List\n", 0, 1),
                ("at the end of the file", b"class C {}\n/** d */\n", 0, 1),
                (
                    "before a closing brace",
                    b"class C {\n    def f() {}\n    /** d */\n}\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// Roslyn's `GetDocumentationCommentTriviaFromSyntaxNode` reads a
    /// member's leading trivia back past plain comments and directives to
    /// the nearest doc comment, then keeps every doc comment before it
    /// that only blank rows separate; everything else is CS1587 (#1613). A
    /// namespace, a `using` and a statement take none. Each row was checked
    /// with `dotnet build` (SDK 9.0) and the XML documentation file.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_doc_comments_bind_to_the_member_after_them() {
        assert_doc_rows(
            LANG::Csharp,
            &[
                ("direct", b"/// d\nclass C {}\n", 1, 1),
                (
                    "through an attribute",
                    b"/// d\n[System.Obsolete]\nclass C {}\n",
                    1,
                    1,
                ),
                ("across a blank row", b"/// d\n\nclass C {}\n", 1, 1),
                ("two runs, both kept", b"/// a\n\n/// b\nclass C {}\n", 2, 2),
                ("block and line", b"/** a */\n/// b\nclass C {}\n", 2, 2),
                ("past a plain comment", b"/// a\n// p\nclass C {}\n", 1, 2),
                (
                    "superseded across a plain comment",
                    b"/// a\n// p\n/// b\nclass C {}\n",
                    1,
                    3,
                ),
                (
                    "past a directive",
                    b"/// d\n#nullable enable\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "superseded across a directive",
                    b"/// a\n#pragma warning disable CS0169\n/// b\nclass C {}\n",
                    1,
                    2,
                ),
                ("enum member", b"enum E {\n    /// d\n    A,\n}\n", 1, 1),
                (
                    "after code on its row",
                    b"class C {\n    int a; /// d\n    int b;\n}\n",
                    1,
                    1,
                ),
                ("at the end of the file", b"class C {}\n/// d\n", 0, 1),
                (
                    "before a closing brace",
                    b"class C {\n    void F() {}\n    /// d\n}\n",
                    0,
                    1,
                ),
                ("using", b"/// d\nusing System;\nclass C {}\n", 0, 1),
                ("block namespace", b"/// d\nnamespace N { }\n", 0, 1),
                (
                    "file-scoped namespace",
                    b"/// d\nnamespace N;\nclass C {}\n",
                    0,
                    1,
                ),
                (
                    "assembly attribute",
                    b"/// d\n[assembly: System.CLSCompliant(false)]\nclass C {}\n",
                    0,
                    1,
                ),
                (
                    "top-level statement",
                    b"/// d\nSystem.Console.WriteLine();\n",
                    0,
                    1,
                ),
            ],
        );
    }

    /// phpDocumentor gives an element PHP-Parser's `getDocComment`, the
    /// last DocBlock before its first token, plain comments read past; a
    /// DocBlock above a statement documents nothing. `createFileDocBlock`
    /// makes the file's first DocBlock the file's own when a non-element
    /// (a `namespace`, a `use`, a statement, nothing) follows the leading
    /// DocBlocks, or when exactly two precede the first element (#1613).
    /// Each row was checked with phpDocumentor 3's reflection.
    #[cfg(feature = "php")]
    #[test]
    fn php_doc_comments_bind_to_the_element_after_them() {
        assert_doc_rows(
            LANG::Php,
            &[
                (
                    "direct",
                    b"<?php\nfunction z() {}\n/** d */\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "through an attribute",
                    b"<?php\nfunction z() {}\n/** d */\n#[Attr]\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "across a blank row",
                    b"<?php\nfunction z() {}\n/** d */\n\nfunction f() {}\n",
                    1,
                    1,
                ),
                (
                    "past a plain comment",
                    b"<?php\nfunction z() {}\n/** d */\n// p\nfunction f() {}\n",
                    1,
                    2,
                ),
                (
                    "superseded",
                    b"<?php\nfunction z() {}\n/** a */\n/** b */\nfunction f() {}\n",
                    1,
                    2,
                ),
                (
                    "class member",
                    b"<?php\nclass C {\n    /** d */\n    public $p;\n    /** e */\n}\n",
                    1,
                    2,
                ),
                (
                    "above a statement",
                    b"<?php\nfunction z() {}\n/** d */\n$x = 1;\n",
                    0,
                    1,
                ),
                (
                    "at the end of the file",
                    b"<?php\nfunction z() {}\n/** d */\n",
                    0,
                    1,
                ),
                ("file DocBlock alone", b"<?php\n/** f */\n", 1, 1),
                (
                    "file DocBlock and the class's",
                    b"<?php\n/** f */\n\n/** c */\nclass C {}\n",
                    2,
                    2,
                ),
                (
                    "one DocBlock above the first class is the class's",
                    b"<?php\n/** c */\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "three DocBlocks above the first function",
                    b"<?php\n/** a */\n/** b */\n/** c */\nfunction f() {}\n",
                    1,
                    3,
                ),
                (
                    "file DocBlock above a namespace",
                    b"<?php\n/** f */\nnamespace N;\nclass C {}\n",
                    1,
                    1,
                ),
                (
                    "file DocBlock above a declare",
                    b"<?php\n/** f */\ndeclare(strict_types=1);\n/** u */\nuse A\\B;\n",
                    1,
                    2,
                ),
                (
                    "file DocBlock above a statement",
                    b"<?php\n/** f */\n$x = 1;\n",
                    1,
                    1,
                ),
            ],
        );
    }

    /// JSDoc's `getLeadingJsdocComment` takes the nearest block comment
    /// before a node, line comments filtered out, and documents a
    /// declaration or an assignment; a call or another statement takes
    /// none. A virtual comment — `@file`, `@fileoverview`, `@module`, or a
    /// tag naming a symbol — documents that symbol wherever it sits, and
    /// `@license` is not one (#1613). Each row was checked with
    /// `jsdoc -X` (4.x). Mozilla's JavaScript shares the rule.
    #[cfg(all(feature = "javascript", feature = "mozjs"))]
    #[test]
    fn javascript_doc_comments_bind_to_the_symbol_after_them() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            ("direct", b"/** d */\nfunction f() {}\n", 1, 1),
            ("through a decorator", b"/** d */\n@dec\nclass C {}\n", 1, 1),
            (
                "after a member's decorator",
                b"class C {\n  @dec\n  /** d */\n  m() {}\n}\n",
                0,
                1,
            ),
            (
                "across a blank row",
                b"x;\n/** d */\n\nfunction f() {}\n",
                1,
                1,
            ),
            (
                "past a line comment",
                b"/** d */\n// p\nfunction f() {}\n",
                1,
                2,
            ),
            (
                "detached by a block comment",
                b"/** d */\n/* p */\nfunction f() {}\n",
                0,
                2,
            ),
            ("superseded", b"/** a */\n/** b */\nfunction f() {}\n", 1, 2),
            (
                "after code on its row",
                b"x; /** d */\nfunction f() {}\n",
                1,
                1,
            ),
            ("assignment", b"x;\n/** d */\nx = 1;\n", 1, 1),
            ("member assignment", b"x;\n/** d */\na.b += 1;\n", 1, 1),
            (
                "parenthesized assignment",
                b"x;\n/** d */\n(a = 1);\n",
                1,
                1,
            ),
            (
                "comment inside the parentheses",
                b"x;\n/** d */\n(\n  // c\n  a = 1\n);\n",
                1,
                2,
            ),
            ("call", b"x;\n/** d */\nfoo();\n", 0, 1),
            ("destructuring", b"x;\n/** d */\nlet { q } = o;\n", 0, 1),
            ("export list", b"x;\n/** d */\nexport { o };\n", 0, 1),
            // A parse error between two doc comments is read past, not
            // read as the declaration's first token: JSDoc documents `f`
            // with `b`, whichever of the two spans more rows.
            (
                "error between a multi-row and a one-row doc",
                b"/**\n * a\n */\n%%%\n/** b */\nfunction f() {}\n",
                1,
                4,
            ),
            (
                "error between a one-row and a multi-row doc",
                b"/** a */\n%%%\n/**\n * b\n */\nfunction f() {}\n",
                3,
                4,
            ),
            (
                "at the end of the file",
                b"function f() {}\n/** d */\n",
                0,
                1,
            ),
            ("@file", b"/**\n * @file F\n */\nfunction f() {}\n", 3, 3),
            (
                "@file above a call",
                b"/**\n * @file F\n */\nfoo();\n",
                3,
                3,
            ),
            ("@module", b"/**\n * @module m\n */\nfoo();\n", 3, 3),
            (
                "named @typedef",
                b"/**\n * @typedef {Object} T\n */\nfoo();\n",
                3,
                3,
            ),
            (
                "unnamed @typedef",
                b"/**\n * @typedef {Object}\n */\nfoo();\n",
                0,
                3,
            ),
            (
                "@license above a call",
                b"/**\n * @license MIT\n */\nfoo();\n",
                0,
                3,
            ),
            (
                "@license above a function",
                b"/**\n * @license MIT\n */\nfunction f() {}\n",
                3,
                3,
            ),
            (
                "in a body, above a call",
                b"function f() {\n  /** d */\n  foo();\n}\n",
                1,
                1,
            ),
        ];
        assert_doc_rows(LANG::Javascript, cases);
        assert_doc_rows(LANG::Mozjs, cases);
    }

    /// tsc's `getJSDocCommentsAndTags` gives a declaration the last `/**`
    /// before its first token, past every plain comment, and declares no
    /// symbol for an assignment statement; a comment after a decorator is
    /// in the declaration and documents nothing. TypeDoc reads a `@module`
    /// or `@packageDocumentation` comment as the file's, and `@license`
    /// stays a tag of the declaration (#1613). Each row was checked with
    /// tsc's quick info and TypeDoc 0.28. TSX shares the rule.
    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_doc_comments_bind_to_the_declaration_after_them() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            ("direct", b"/** d */\nfunction f() {}\n", 1, 1),
            (
                "past a block comment",
                b"/** d */\n/* p */\nfunction f() {}\n",
                1,
                2,
            ),
            ("superseded", b"/** a */\n/** b */\nfunction f() {}\n", 1, 2),
            (
                "before a member's decorator",
                b"class C {\n  /** d */\n  @dec\n  m() {}\n}\n",
                1,
                1,
            ),
            (
                "after a member's decorator",
                b"class C {\n  @dec\n  /** d */\n  m() {}\n}\n",
                0,
                1,
            ),
            (
                "interface member",
                b"interface I {\n  /** d */\n  p: number;\n}\n",
                1,
                1,
            ),
            (
                "enum member",
                b"enum E {\n  /** a */\n  A,\n  /** b */\n  B = 1,\n}\n",
                2,
                2,
            ),
            (
                "string-named enum member",
                b"enum E {\n  /** a */\n  'a',\n  /** b */\n  [\"b\"],\n}\n",
                2,
                2,
            ),
            ("parameter", b"function f(/** d */ x: number) {}\n", 1, 1),
            // tsc's leading-comment scan starts at the line break after the
            // token before a declaration; a parameter reads its trailing
            // comments too.
            (
                "trailing code on its row",
                b"x; /** d */\nfunction f() {}\n",
                0,
                1,
            ),
            (
                "below code's row",
                b"x; /** a */\n/** b */\nfunction f() {}\n",
                1,
                2,
            ),
            (
                "trailing a member",
                b"interface I {\n  a: number; /** d */\n  b: string;\n}\n",
                0,
                1,
            ),
            (
                "multi-row block trailing code",
                b"x; /**\n * d\n */\nfunction f() {}\n",
                0,
                3,
            ),
            (
                "plain comment below the trailing doc",
                b"x; /** d */\n/* p */\nfunction f() {}\n",
                0,
                2,
            ),
            // A parse error between two doc comments is read past, not
            // read as the declaration's first token: tsc attaches `b` to
            // `f`, whichever of the two spans more rows.
            (
                "error between a multi-row and a one-row doc",
                b"/**\n * a\n */\n%%%\n/** b */\nfunction f() {}\n",
                1,
                4,
            ),
            (
                "error between a one-row and a multi-row doc",
                b"/** a */\n%%%\n/**\n * b\n */\nfunction f() {}\n",
                3,
                4,
            ),
            (
                "after a parameter's comma",
                b"function f(\n  a: number, /** d */\n  b: string,\n) {}\n",
                1,
                1,
            ),
            ("member name", b"a./** d */b;\n", 0, 1),
            (
                "after a modifier",
                b"class C { static /** d */ x = 1 }\n",
                0,
                1,
            ),
            ("after export", b"export /** d */ function f() {}\n", 0, 1),
            ("before export", b"/** d */\nexport function f() {}\n", 1, 1),
            (
                "anonymous default function",
                b"/** d */\nexport default function () {}\n",
                1,
                1,
            ),
            (
                "anonymous default generator",
                b"/** d */\nexport default function* () {}\n",
                1,
                1,
            ),
            (
                "anonymous default class",
                b"/** d */\nexport default class {}\n",
                1,
                1,
            ),
            (
                "default expression",
                b"/** d */\nexport default foo;\n",
                0,
                1,
            ),
            (
                "after declare",
                b"declare /** d */ function f(): void;\n",
                0,
                1,
            ),
            ("assignment", b"x;\n/** d */\nx = 1;\n", 0, 1),
            ("using declaration", b"/** d */\nusing h = open();\n", 1, 1),
            (
                "await using declaration",
                b"/** d */\nawait using h = open();\n",
                1,
                1,
            ),
            (
                "at the end of the file",
                b"function f() {}\n/** d */\n",
                0,
                1,
            ),
            ("@module", b"/**\n * @module m\n */\nfoo();\n", 3, 3),
            (
                "@packageDocumentation",
                b"/**\n * @packageDocumentation\n */\nfoo();\n",
                3,
                3,
            ),
            (
                "@license",
                b"/**\n * @license MIT\n */\nexport function f() {}\n",
                3,
                3,
            ),
        ];
        assert_doc_rows(LANG::Typescript, cases);
        assert_doc_rows(LANG::Tsx, cases);
    }

    /// Doxygen keeps every doc block it meets until the next entry and
    /// gives them all to it, reading past `#include`, `#pragma`, an empty
    /// conditional and plain comments; a block before a closing brace, at
    /// the end of the file or before a forward declaration documents
    /// nothing. A structural block (`@file`, `@def X`, `@class X`) and a
    /// member-after `/**<` document where they sit (#1613). Each row was
    /// checked with Doxygen 1.13.2's XML output, `EXTRACT_ALL=NO`.
    #[cfg(all(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
    #[test]
    fn doxygen_doc_comments_bind_to_the_entry_after_them() {
        // Shared by C, C++, Mozilla's C++ and Objective-C: C syntax only.
        let c_family: &[(&str, &[u8], u64, u64)] = &[
            ("direct", b"/** d */\nint f(void);\n", 1, 1),
            (
                "qt and line forms",
                b"/*! a */\n//! b\n/// c\nint f(void);\n",
                3,
                3,
            ),
            ("across a blank row", b"/** d */\n\nint f(void);\n", 1, 1),
            (
                "through static inline",
                b"/** d */\nstatic inline int f(void) { return 0; }\n",
                1,
                1,
            ),
            (
                "through __attribute__",
                b"/** d */\n__attribute__((unused)) static int x;\n",
                1,
                1,
            ),
            (
                "superseded: both kept",
                b"/** a */\n/** b */\nint f(void);\n",
                2,
                2,
            ),
            (
                "past a plain comment",
                b"/** a */\n// p\nint f(void);\n",
                1,
                2,
            ),
            (
                "past #include",
                b"/** d */\n#include <x.h>\nint f(void);\n",
                1,
                1,
            ),
            (
                "past #pragma",
                b"/** d */\n#pragma once\nint f(void);\n",
                1,
                1,
            ),
            (
                "past an empty statement",
                b"/** d */\n;\nint f(void);\n",
                1,
                1,
            ),
            (
                "past an empty conditional",
                b"/** d */\n#ifdef X\n#endif\nint f(void);\n",
                1,
                1,
            ),
            ("a macro", b"/** d */\n#define M 1\n", 1, 1),
            (
                "a function-like macro",
                b"/** d */\n#define M(a) (a)\n",
                1,
                1,
            ),
            ("a typedef", b"/** d */\ntypedef int t;\n", 1, 1),
            ("a struct", b"/** d */\nstruct s { int a; };\n", 1, 1),
            (
                "a field",
                b"struct s {\n    /** d */\n    int a;\n};\n",
                1,
                1,
            ),
            (
                "an enumerator",
                b"enum e {\n    /** d */\n    A,\n};\n",
                1,
                1,
            ),
            ("a macro call", b"/** d */\nFOO(bar);\n", 1, 1),
            (
                "a conditional holding a declaration",
                b"/** d */\n#ifdef X\nint f(void);\n#endif\n",
                1,
                1,
            ),
            (
                "into an include guard",
                b"/** d */\n#ifndef G\n#define G\nint f(void);\n#endif\n",
                1,
                1,
            ),
            ("a forward declaration", b"/** d */\nstruct s;\n", 0, 1),
            ("above #include alone", b"/** d */\n#include <x.h>\n", 0, 1),
            ("at the end of the file", b"int f(void);\n/** d */\n", 0, 1),
            (
                "before a closing brace",
                b"struct s {\n    int a;\n    /** d */\n};\n",
                0,
                1,
            ),
            (
                "after a function body",
                b"void f(void) {\n    return;\n}\n/** d */\n",
                0,
                1,
            ),
            (
                "an initializer list",
                b"int a[] = {\n    /** d */\n    1,\n};\n",
                1,
                1,
            ),
            ("member after", b"int x; /**< d */\nint y;\n", 1, 1),
            ("member after on its own row", b"int x;\n///< d\n", 1, 1),
            ("@file alone", b"/** @file\n * d\n */\n", 3, 3),
            (
                "@file before a declaration",
                b"/** @file */\nint f(void);\n",
                1,
                1,
            ),
            (
                "a licence block above @file",
                b"/** licence */\n/** @file */\n#include <x.h>\n",
                2,
                2,
            ),
            ("a doc block after @file", b"/** @file */\n/** d */\n", 1, 2),
            ("@def naming its macro", b"/** @def M\n * d\n */\n", 3, 3),
            ("@defgroup", b"/// @defgroup g Group\n/// @{\n", 2, 2),
            ("@} alone", b"int f(void);\n/** @} */\n", 0, 1),
            (
                "@brief is not structural",
                b"/** @brief d */\n#include <x.h>\n",
                0,
                1,
            ),
            ("@fn with no name", b"/** @fn */\n#include <x.h>\n", 0, 1),
            (
                "escaped command",
                b"/** \\\\file x */\n#include <x.h>\n",
                0,
                1,
            ),
            ("in a code span", b"/** `@file` */\n#include <x.h>\n", 0, 1),
            (
                "in a mail address",
                b"/** a@file.com */\n#include <x.h>\n",
                0,
                1,
            ),
            (
                "in a code block",
                b"/**\n * @code\n * @file\n * @endcode\n */\n#include <x.h>\n",
                0,
                5,
            ),
            (
                "in a function body",
                b"int f(void) {\n    /** note */\n    return 0;\n}\n",
                1,
                1,
            ),
            (
                "in a local struct, before its closing brace",
                b"void f(void) {\n    struct s {\n        int a;\n        /** d */\n    };\n}\n",
                1,
                1,
            ),
            (
                "a field of a body-less struct type",
                b"struct s {\n    /** d */\n    struct t *p;\n};\n",
                1,
                1,
            ),
            (
                "before an enum's closing brace",
                b"enum e {\n    A,\n    /** d */\n};\n",
                0,
                1,
            ),
            (
                "a conditional holding only an empty statement",
                b"/** d */\n#ifndef G\n;\n#endif\n",
                0,
                1,
            ),
            ("into #if", b"/** d */\n#if 1\nint f(void);\n#endif\n", 1, 1),
            (
                "into #else",
                b"/** d */\n#ifdef X\n#else\nint f(void);\n#endif\n",
                1,
                1,
            ),
            (
                "at the end of an #if branch",
                b"#if 1\nint f(void);\n/** d */\n#endif\n",
                0,
                1,
            ),
            (
                "at the end of an #else branch",
                b"#ifdef X\n#else\nint f(void);\n/** d */\n#endif\n",
                0,
                1,
            ),
            (
                "at the end of an #elif branch",
                b"#if 0\n#elif 1\nint f(void);\n/** d */\n#endif\n",
                0,
                1,
            ),
            (
                "at the end of an #elifndef branch",
                b"#ifdef X\n#elifndef Y\nint f(void);\n/** d */\n#endif\n",
                0,
                1,
            ),
            (
                "between two gathering blocks",
                b"/** @file */\n/** a */\n/** @defgroup g G */\n/** b */\n",
                3,
                4,
            ),
            (
                "a line block below a gathering block",
                b"/** @file */\n/// d\n",
                1,
                2,
            ),
            (
                "a block below a gathering line",
                b"/// @file\n/** d */\n",
                1,
                2,
            ),
            (
                "a line below a plain comment below a gathering line",
                b"/// @file\n// p\n/// d\n",
                1,
                3,
            ),
            (
                "a plain comment above a gathering block",
                b"// p\n/** @file */\n",
                1,
                2,
            ),
            (
                "in an unclosed fence",
                b"/**\n * ```\n * @file\n */\n#include <x.h>\n",
                0,
                4,
            ),
            (
                "in an unclosed code block",
                b"/**\n * @code\n * @file\n */\n#include <x.h>\n",
                0,
                4,
            ),
            (
                "before a stray closing brace",
                b"int f(void);\n/** d */\n}\n",
                0,
                1,
            ),
            (
                "past a stray closing brace",
                b"int f(void);\n/** d */\n}\nint g(void);\n",
                1,
                1,
            ),
            (
                "past a stray closing parenthesis",
                b"int f(void);\n/** d */\n)\nint g(void);\n",
                1,
                1,
            ),
            (
                "a declaration after a macro call missing its semicolon",
                b"/** d */\nFOO(bar)\nvoid g(void);\n",
                1,
                1,
            ),
            (
                "before a lone identifier at the start",
                b"/** d */\nFOO\n",
                0,
                1,
            ),
            ("before a stray literal", b"int x;\n/** d */\n42\n", 0, 1),
            (
                "before stray parentheses around a plain comment",
                b"int f(void);\n/** d */\n)\n// p\n)\n",
                0,
                2,
            ),
            (
                "before two stray closing braces",
                b"int f(void);\n/** d */\n}\n}\n",
                0,
                1,
            ),
            (
                "a struct before a stray closing brace",
                b"/** d */\nstruct s { int a; } }\n",
                1,
                1,
            ),
            (
                "before a lone identifier and a plain comment",
                b"int x;\n/** d */\nend // p\n",
                0,
                2,
            ),
            (
                "past a lone identifier and #include",
                b"int x;\n/** d */\nFOO\n#include <a.h>\nint y;\n",
                1,
                1,
            ),
            (
                "past an empty conditional inside #elif",
                b"#if 0\n#elif 1\n/** d */\n#ifdef Z\n#endif\nint f(void);\n#endif\n",
                1,
                1,
            ),
            (
                "into #elif from outside",
                b"/** d */\n#if 0\n#elif 1\nint f(void);\n#endif\n",
                1,
                1,
            ),
            (
                "before a parse error holding only comments",
                b"int a;\n/** d */\n) /* p */ /* q */ }\n",
                0,
                2,
            ),
            (
                "into #elif",
                b"#if 0\n#elif 1\n/** d */\nint f(void);\n#endif\n",
                1,
                1,
            ),
            (
                "a terminated lone identifier",
                b"int x;\n/** d */\nFOO;\n",
                1,
                1,
            ),
            (
                "a conditional holding only @end",
                b"/** d */\n#ifdef __OBJC__\n@end\n#endif\n",
                0,
                1,
            ),
            ("before a lone identifier", b"int x;\n/** d */\nFOO\n", 0, 1),
            (
                "in an htmlonly block",
                b"/**\n * @htmlonly @file @endhtmlonly\n */\n#include <x.h>\n",
                0,
                3,
            ),
            (
                "in an HTML comment",
                b"/**\n * <!-- @file -->\n */\n#include <x.h>\n",
                0,
                3,
            ),
            (
                "after an htmlonly block",
                b"/**\n * @htmlonly x @endhtmlonly\n * @file\n */\n#include <x.h>\n",
                4,
                4,
            ),
        ];
        for lang in [LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc] {
            assert_doc_rows(lang, c_family);
        }

        let cpp: &[(&str, &[u8], u64, u64)] = &[
            ("through [[nodiscard]]", b"/** d */\n[[nodiscard]] int f();\n", 1, 1),
            (
                "through a template head",
                b"/** d */\ntemplate <class T>\nT f(T t) { return t; }\n",
                1,
                1,
            ),
            (
                "through requires",
                b"/** d */\ntemplate <class T>\n    requires (sizeof(T) > 1)\nT f(T t) { return t; }\n",
                1,
                1,
            ),
            ("a template forward declaration", b"/** d */\ntemplate <class T> class F;\n", 1, 1),
            ("a namespace", b"/** d */\nnamespace n {\nint f();\n}\n", 1, 1),
            ("a using directive", b"/** d */\nusing namespace std;\n", 1, 1),
            ("an alias", b"/** d */\nusing T = int;\n", 1, 1),
            ("extern C", b"/** d */\nextern \"C\" {\nint f();\n}\n", 1, 1),
            (
                "past static_assert",
                b"/** d */\nstatic_assert(true, \"x\");\nint f();\n",
                1,
                1,
            ),
            (
                "past an access specifier",
                b"class C {\n    /** d */\npublic:\n    int m();\n};\n",
                1,
                1,
            ),
            (
                "a member function definition",
                b"class C {\n    /** d */\n    int m() { return 0; }\n};\n",
                1,
                1,
            ),
            (
                "a nested forward declaration",
                b"class C {\n    /** d */\n    struct In;\n    int m();\n};\n",
                0,
                1,
            ),
            ("a class forward declaration", b"/** d */\nclass C;\n", 0, 1),
            (
                "an explicit instantiation",
                b"template <class T> struct S {};\n/** d */\ntemplate struct S<int>;\n",
                0,
                1,
            ),
            (
                "before a namespace's closing brace",
                b"namespace n {\nint f();\n/** d */\n}\n",
                0,
                1,
            ),
            ("@class naming its class", b"/** @class X\n * d\n */\n", 3, 3),
            (
                "@class with no name",
                b"/** @class */\nclass C;\n",
                0,
                1,
            ),
            (
                "after an attribute, in the declaration",
                b"[[nodiscard]]\n/// d\nint f();\n",
                1,
                1,
            ),
            (
                "in a template parameter list",
                b"template </** d */ class T> void f(T);\n",
                0,
                1,
            ),
            (
                "past a namespace alias",
                b"namespace b {}\n/** d */\nnamespace a = b;\nint f();\n",
                1,
                1,
            ),
            (
                "past an empty member",
                b"class C {\n    /** d */\n    ;\n    int m();\n};\n",
                1,
                1,
            ),
            (
                "a friend",
                b"class D;\nclass C {\n    /** d */\n    friend class D;\n};\n",
                1,
                1,
            ),
            (
                "a nested struct with a body",
                b"struct S {\n    /** d */\n    struct In { int a; };\n    int b;\n};\n",
                1,
                1,
            ),
        ];
        assert_doc_rows(LANG::Cpp, cpp);
        assert_doc_rows(LANG::Mozcpp, cpp);

        // A `.h` is read as C++, so Objective-C's `@end` there parses as a
        // parse error holding `@` and a statement holding `end`.
        assert_doc_rows(
            LANG::Cpp,
            &[
                ("@end in a header", b"int x;\n/** d */\n@end\n", 0, 1),
                ("@end alone in a header", b"/** d */\n@end\n", 0, 1),
                (
                    "@interface in a header",
                    b"/** d */\n@interface O : NSObject\n- (void)m;\n@end\n",
                    1,
                    1,
                ),
                (
                    "before @end and a plain comment in a header",
                    b"int x;\n/** d */\n@end\n// p\n",
                    0,
                    2,
                ),
                (
                    "before a class macro",
                    b"class C {\n    /** d */\n    Q_OBJECT\n};\n",
                    0,
                    1,
                ),
                (
                    "past @end in a header",
                    b"int x;\n/** d */\n@end\nint y;\n",
                    1,
                    1,
                ),
            ],
        );

        assert_doc_rows(
            LANG::Objc,
            &[
                (
                    "@interface, @property and a method",
                    b"/** a */\n@interface O : NSObject\n/** b */\n@property int x;\n/** c */\n- (void)m;\n@end\n",
                    3,
                    3,
                ),
                (
                    "@implementation and a method",
                    b"/** a */\n@implementation O\n/** b */\n- (void)m {}\n@end\n",
                    2,
                    2,
                ),
                (
                    "@protocol and an instance variable",
                    b"/** a */\n@protocol P\n- (void)m;\n@end\n@interface O : NSObject {\n    /** b */\n    int iv;\n}\n@end\n",
                    2,
                    2,
                ),
                ("before @end", b"@interface O : NSObject\n/** d */\n@end\n", 0, 1),
                ("a forward @class", b"/** d */\n@class C;\n", 0, 1),
                ("a forward @protocol", b"/** d */\n@protocol P;\n", 0, 1),
                (
                    "@synthesize",
                    b"@implementation O\n/** d */\n@synthesize x;\n@end\n",
                    0,
                    1,
                ),
                ("past #import", b"/** d */\n#import <F/F.h>\nint f(void);\n", 1, 1),
                (
                    "before a protocol's @end",
                    b"@protocol P\n- (void)m;\n/** d */\n@end\n",
                    0,
                    1,
                ),
                (
                    "before an @optional section's @end",
                    b"@protocol P\n@optional\n- (void)m;\n/** d */\n@end\n",
                    0,
                    1,
                ),
                (
                    "before an @optional section's @end, past a #pragma",
                    b"@protocol P\n@optional\n/** d */\n#pragma mark x\n@end\n",
                    0,
                    1,
                ),
                (
                    "a conditional holding #include and @end",
                    b"/** d */\n#ifdef X\n#include <a.h>\n@end\n#endif\n",
                    0,
                    1,
                ),
                (
                    "an @optional section",
                    b"@protocol P\n/** d */\n@optional\n- (void)m;\n@end\n",
                    1,
                    1,
                ),
                (
                    "before an instance-variable block's closing brace",
                    b"@interface O : NSObject {\n    int a;\n    /** d */\n}\n@end\n",
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

    /// LDoc documents what a module declares: a `---` block inside a
    /// function body annotates the code there — LuaLS's `---@type` on a
    /// local, an LDoc `@fixme` — and stays a comment, as PHP's in-body
    /// `@var` does (#1597).
    #[cfg(feature = "lua")]
    #[test]
    fn lua_ldoc_blocks_inside_a_function_body_are_comments() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("module level", b"--- Doc.\nlocal function f() end\n", 1, 1),
            (
                "annotation in a body",
                b"local function f()\n  ---@type integer\n  local x = 1\n  return x\nend\n",
                0,
                1,
            ),
            (
                "block in a body",
                b"function f()\n  --- Note,\n  -- more.\n  return 1\nend\n",
                0,
                2,
            ),
            (
                "in an anonymous function",
                b"local g = function()\n  --- Note.\n  return 1\nend\n",
                0,
                1,
            ),
        ];
        assert_doc_rows(LANG::Lua, cases);
    }

    /// LDoc discards a `---` block with no text, keeps one that is the
    /// first of a file opening with a comment (the module's
    /// documentation, whatever follows it), that has a line opening with a
    /// tag, or that an item follows — a `function`, an assignment to one
    /// name, a named table field, or a `return` of a function or table —
    /// and drops any other (`parse.lua`, `lang.lua`). Most sources open
    /// with a module block, which counts one `dloc` and one `cloc` row of
    /// its own (#1597).
    #[cfg(feature = "lua")]
    #[test]
    fn lua_ldoc_blocks_follow_ldocs_item_rule() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            (
                "first block, code after a blank",
                b"--- Module.\n\nprint(1)\n",
                1,
                1,
            ),
            // LDoc reads nothing from a file whose first token is code.
            (
                "first block after code",
                b"local M = {}\n--- Module.\nprint(1)\n",
                0,
                1,
            ),
            (
                "divider before the module block",
                b"-----\n\n--- Utilities.\n-- More.\n\nlocal M = {}\n",
                2,
                3,
            ),
            // A `---` row continuing a plain `--` block opens no block, so
            // the module block below it is still the first.
            (
                "--- continuing a plain block",
                b"-- Copyright.\n--- Licensed MIT.\n\n--- Module.\n\nlocal M = {}\n",
                1,
                3,
            ),
            // The first block sits directly under code, and still opens
            // its run, so the later tagless one is not the first.
            (
                "first block directly under code",
                b"-- Header.\nlocal M = {}\n--- First.\nprint(1)\n--- Second.\nprint(2)\n",
                1,
                3,
            ),
            // LDoc lexes a line comment with its newline and groups every
            // consecutive comment token, so a `---` block directly under a
            // row ending in a trailing `-- note` belongs to that `--`
            // group: no block, however an item follows it (real LDoc
            // 1.4.6 documents nothing here). The later block is then the
            // file's first, and LDoc takes it as the module's.
            (
                "block under a trailing comment",
                b"-- Header.\nlocal M = {} -- module table\n--- Helper.\nlocal function helper() end\n",
                0,
                3,
            ),
            (
                "first block after a swallowed one",
                b"-- Header.\nlocal M = {} -- module table\n--- Helper.\nlocal function helper() end\n\n--- Later note.\n\nprint(2)\n",
                1,
                4,
            ),
            // LDoc skips a `#!` line to the first comment, so the file
            // still opens with its module block.
            (
                "module block after a shebang",
                b"#!/usr/bin/env lua\n--- Tool that greets.\n-- Run it from a shell.\n\nlocal x = 1\n",
                2,
                2,
            ),
            // The grammar parks a comment above the first statement of an
            // `if` body outside the `block`, which stands for it, so the
            // first item of the body counts as a later one does.
            (
                "first item in a chunk-level if body",
                b"local M = {}\nif true then\n  --- Doc for f.\n  function M.f() end\n  --- Doc for g.\n  function M.g() end\nend\nreturn M\n",
                2,
                2,
            ),
            (
                "in a nested block",
                b"if x then\n  --- Note.\n  print(1)\nend\n",
                0,
                1,
            ),
            (
                "divider above a function",
                b"--- M.\nlocal M = {}\n-----\nfunction f() end\n",
                1,
                2,
            ),
            (
                "address is no tag",
                b"--- M.\nlocal M = {}\n--- Ask dev@example.com first.\nprint(1)\n",
                1,
                2,
            ),
            (
                "LuaLS tag",
                b"--- M.\nlocal M = {}\n---@class Foo\nprint(1)\n",
                2,
                2,
            ),
            (
                "tag after the run",
                b"--- M.\nlocal M = {}\n--- Note.\nprint(1)\n-- @todo later\n",
                1,
                3,
            ),
            (
                "above a dotted name",
                b"--- M.\nlocal M = {}\n--- B.\nM.a.b = 1\n",
                2,
                2,
            ),
            (
                "above two names",
                b"--- M.\nlocal M = {}\n--- AB.\nlocal a, b = 1, 2\n",
                1,
                2,
            ),
            (
                "above an attributed local",
                b"--- M.\nlocal M = {}\n--- X.\nlocal x <const> = 1\n",
                1,
                2,
            ),
            (
                "above a bracket index",
                b"--- M.\nlocal M = {}\n--- I.\nM[1] = 2\n",
                1,
                2,
            ),
            // The class-as-table idiom: a named field is an item, a keyed
            // or positional one is not.
            (
                "table fields",
                b"--- M.\nlocal M = {}\nM.C = {\n  --- New.\n  new = function(self) end,\n  --- Size.\n  size = 1,\n  --- Keyed.\n  [1] = 2,\n  --- Positional.\n  x,\n}\n",
                3,
                5,
            ),
            (
                "tagless above a call",
                b"--- M.\nlocal M = {}\n--- Note.\nprint(1)\n",
                1,
                2,
            ),
            (
                "tag above a call",
                b"--- M.\nlocal M = {}\n--- Note.\n-- @param x\nprint(1)\n",
                3,
                3,
            ),
            (
                "above a function",
                b"--- M.\nlocal M = {}\n--- F.\nfunction f() end\n",
                2,
                2,
            ),
            (
                "above a local function",
                b"--- M.\nlocal M = {}\n--- F.\nlocal function f() end\n",
                2,
                2,
            ),
            (
                "above a local assignment",
                b"--- M.\nlocal M = {}\n--- X.\nlocal x = 1\n",
                2,
                2,
            ),
            (
                "above a bare local",
                b"--- M.\nlocal M = {}\n--- X.\nlocal x\n",
                1,
                2,
            ),
            (
                "above a field assignment",
                b"--- M.\nlocal M = {}\n--- Y.\nM.y = 2\n",
                2,
                2,
            ),
            (
                "above return of a table",
                b"--- M.\nlocal M = {}\n--- R.\nreturn {}\n",
                2,
                2,
            ),
            (
                "above return of a function",
                b"--- M.\nlocal M = {}\n--- R.\nreturn function() end\n",
                2,
                2,
            ),
            (
                "above return of a number",
                b"--- M.\nlocal M = {}\n--- R.\nreturn 1\n",
                1,
                2,
            ),
            (
                "blank row before the item",
                b"--- M.\nlocal M = {}\n--- F.\n\nfunction f() end\n",
                1,
                2,
            ),
        ];
        assert_doc_rows(LANG::Lua, cases);
    }

    /// RDoc's `##` and YARD's `@!` directives document a method that has
    /// no `def` to sit above, so the block counts whatever follows it — in
    /// a namespace body, never in a method's (#1597).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_rdoc_and_yard_directive_blocks_are_documentation() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("RDoc ## above define_method", b"class C\n  ##\n  # :method: ghost\n  # Does ghost things.\n  define_method(:ghost) {}\nend\n", 3, 3),
            ("YARD @!method above a DSL call", b"class C\n  # @!method foo(a)\n  #   Does foo.\n  dsl :foo\nend\n", 2, 2),
            ("YARD @!attribute", b"class C\n  # @!attribute [r] name\n  property :name\nend\n", 1, 1),
            ("free-standing @!parse at the top level", b"# @!parse\n#   class Foo; end\nx = 1\n", 2, 2),
            ("## inside a method body", b"class C\n  def m\n    ##\n    # note\n    run\n  end\nend\n", 0, 2),
            ("plain block above a call", b"class C\n  # just a note\n  dsl :foo\nend\n", 0, 1),
        ];
        assert_doc_rows(LANG::Ruby, cases);
    }

    /// The grammar parks a comment that precedes the first statement of an
    /// `if`, `elsif` or `unless` body ahead of the `then` wrapper, which
    /// starts on the comment's own row; the wrapper stands for that
    /// statement, so its doc comment counts as YARD reads it (#1597).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_doc_comment_above_a_conditionals_first_statement_counts() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("if", b"if a\n  # Doc.\n  MAX = 1\nend\n", 1, 1),
            (
                "elsif",
                b"if a\n  x = 1\nelsif b\n  # Doc.\n  E = 1\nend\n",
                1,
                1,
            ),
            ("unless", b"unless c\n  # Doc.\n  U = 1\nend\n", 1, 1),
            (
                "a plain statement is no target",
                b"if a\n  # c\n  puts 1\nend\n",
                0,
                1,
            ),
            (
                "in a method body",
                b"def m\n  if a\n    # c\n    MAX = 1\n  end\nend\n",
                0,
                1,
            ),
        ];
        assert_doc_rows(LANG::Ruby, cases);
    }

    /// YARD's processor descends only into the namespace bodies, the
    /// branches of an `if` / `unless` / `elsif` there and method bodies; no
    /// handler claims a `case`, a `begin` or a loop, so nothing in their
    /// bodies — nor a module opened in one — is documented. Each of these
    /// was dloc 1 while the scope fold passed every unlisted kind through
    /// (#1597); YARD 0.9.45 registers none of them.
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_case_begin_and_loop_bodies_are_not_read_by_yard() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            ("when", b"case d\nwhen 1\n  # Doc.\n  W = 1\nend\n", 0, 1),
            (
                "case else",
                b"case d\nwhen 1\n  x = 1\nelse\n  # Doc.\n  W = 1\nend\n",
                0,
                1,
            ),
            ("case in", b"case d\nin 1\n  # Doc.\n  W = 1\nend\n", 0, 1),
            (
                "begin",
                b"begin\n  # Doc.\n  B = 1\nrescue\n  nil\nend\n",
                0,
                1,
            ),
            (
                "rescue",
                b"begin\n  x = 1\nrescue\n  # Doc.\n  R = 1\nend\n",
                0,
                1,
            ),
            (
                "ensure",
                b"begin\n  x = 1\nensure\n  # Doc.\n  E = 1\nend\n",
                0,
                1,
            ),
            (
                "a loop's second statement",
                b"while a\n  x = 1\n  # Doc.\n  L = 1\nend\n",
                0,
                1,
            ),
            (
                "a module opened in a rescue",
                b"begin\n  require 'json'\nrescue LoadError\n  # Fallback.\n  module JSONShim; end\nend\n",
                0,
                1,
            ),
            (
                "a def in a method's case",
                b"def m\n  case x\n  when 1\n    # Doc.\n    def helper; end\n  end\nend\n",
                0,
                1,
            ),
        ];
        assert_doc_rows(LANG::Ruby, cases);
    }

    /// A brace block parks the comment above its first statement outside
    /// the `block_body` wrapper, as a class body does outside its
    /// `body_statement`; the wrapper stands for that statement, so the
    /// first statement of `included { … }` counts as `included do … end`'s
    /// does (#1597).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_doc_comment_above_a_brace_blocks_first_statement_counts() {
        let cases: &[(&str, &[u8], u64, u64)] = &[
            // (name, source, dloc, cloc)
            (
                "first statement",
                b"included {\n  # Doc.\n  def x; end\n}\n",
                1,
                1,
            ),
            (
                "second statement",
                b"included {\n  def x; end\n  # Doc.\n  def y; end\n}\n",
                1,
                1,
            ),
            (
                "do block, first statement",
                b"included do\n  # Doc.\n  def x; end\nend\n",
                1,
                1,
            ),
        ];
        assert_doc_rows(LANG::Ruby, cases);
    }

    /// The `then` wrapper of a conditional body and the `do` wrapper of a
    /// loop body without the keyword start where a comment parked ahead of
    /// the body's first statement ends. Crediting that start row to PLOC
    /// made the comment row code, so a YARD doc row there stayed in MI's
    /// size term and a working comment's row read as code (#1597).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_body_wrappers_leave_a_parked_comment_row_out_of_ploc() {
        // (source, ploc, dloc): four rows each, one of them the comment.
        for (source, ploc, dloc) in [
            (&b"if a\n  # Doc.\n  MAX = 1\nend\n"[..], 3, 1),
            (b"if a\n  # note\n  puts 1\nend\n", 3, 0),
            (b"while a\n  # note\n  x = 1\nend\n", 3, 0),
        ] {
            let loc = metrics_verbatim(LANG::Ruby, source, loc_only()).loc;
            assert_eq!(
                (loc.sloc(), loc.ploc(), loc.cloc(), loc.dloc()),
                (4, ploc, 1, dloc),
                "{}",
                String::from_utf8_lossy(source)
            );
        }
    }

    /// A doc comment above a chain of statement modifiers is decided by
    /// unwrapping the chain in a loop: a recursive descent overflowed the
    /// walker's stack and aborted the process on valid Ruby (`ruby -c`
    /// accepts tens of thousands of chained modifiers). Run on a small
    /// stack, as the Lua dotted-target test is (#1597).
    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_long_modifier_chain_uses_a_bounded_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let source = format!("# Documentation.\nX = 1{}\n", " if a".repeat(8_000));
                let loc = metrics_verbatim(LANG::Ruby, source.as_bytes(), loc_only()).loc;
                assert_eq!((loc.sloc(), loc.cloc(), loc.dloc()), (2, 1, 1));
            })
            .expect("spawn the bounded-stack metric walk")
            .join()
            .expect("the documented modifier chain must not exhaust the stack");
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
