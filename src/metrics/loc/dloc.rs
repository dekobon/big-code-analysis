//! The `DLOC` metric suite (#1597), in its own file for the same
//! reason `shared.rs` is (#976): `loc.rs` is past the self-scan size
//! limit, and the suite need not grow it further.

use super::line_set::LineSet;
use super::min_or_zero;

/// The `DLOC` metric suite: Documentation Lines Of Code, the rows a
/// language classifies as documentation (#1597).
///
/// A Python docstring is the construct recorded here, and its rows are
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
    use crate::test_support::metrics_verbatim;

    /// The walk restricted to the one metric these tests read (#1127).
    #[cfg(feature = "python")]
    fn loc_only() -> MetricsOptions {
        MetricsOptions::default().with_only(&[Metric::Loc])
    }

    /// Asserts each `(name, source, dloc, cloc)` row of one language's
    /// table.
    #[cfg(feature = "python")]
    fn assert_doc_rows(lang: LANG, cases: &[(&str, &[u8], u64, u64)]) {
        for (name, source, dloc, cloc) in cases {
            let loc = metrics_verbatim(lang, source, loc_only()).loc;
            assert_eq!((loc.dloc(), loc.cloc()), (*dloc, *cloc), "{lang:?}: {name}");
        }
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
