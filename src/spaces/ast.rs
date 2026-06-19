//! Inherent and `Debug` impl blocks for [`super::Ast`].
//!
//! Split out of `spaces.rs` to keep that module focused on the public
//! API type definitions. The blocks are moved verbatim; method
//! resolution is by type, so `crate::spaces::Ast::parse` (and every
//! other `Ast` method) resolves unchanged.

use super::*;

impl fmt::Debug for Ast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The held parser owns a `tree_sitter::Tree` and a `Vec<u8>`;
        // neither has a meaningful `Debug` projection (one is an opaque
        // C handle, the other is raw source). Reporting language + name
        // keeps the public `Ast: Debug` promise without forcing `Debug`
        // onto every per-language `*Code` tag.
        f.debug_struct("Ast")
            .field("language", &self.language())
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Ast {
    /// Parse `source` into a reusable [`Ast`]. Equivalent to the parse half
    /// of [`analyze`]: every [`Ast::metrics`] call on the returned handle
    /// produces the same [`FuncSpace`] as a freshly-issued
    /// `analyze(source, options)` would.
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::LanguageDisabled`] when the source language's
    /// per-language Cargo feature is not enabled in this build.
    pub fn parse(source: Source<'_>) -> Result<Self, MetricsError> {
        let Source {
            lang,
            code,
            name,
            preproc_path,
            preproc,
        } = source;
        let inner =
            crate::langs::ast_parse_dispatch(lang, code.into_owned(), preproc_path, preproc)?;
        Ok(Self { inner, name })
    }

    /// Adopt a caller-built [`tree_sitter::Tree`], reusing it instead of
    /// running the bundled parser, with `name: Option<String>` carried
    /// end-to-end.
    ///
    /// The supplied `tree` must have been produced from `code` with the
    /// [`tree_sitter::Language`] returned by
    /// [`LANG::tree_sitter_language`] for `lang`; a mismatch is not
    /// `unsafe` but yields nonsensical metric values.
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
    /// per-language Cargo feature is not enabled in this build.
    pub fn from_tree_sitter(
        lang: LANG,
        tree: tree_sitter::Tree,
        code: Vec<u8>,
        name: Option<String>,
    ) -> Result<Self, MetricsError> {
        let inner = crate::langs::ast_from_tree_dispatch(lang, tree, code)?;
        Ok(Self { inner, name })
    }

    /// Read `path`, detect its language, and parse it into a reusable
    /// [`Ast`] — the file-backed counterpart to [`Ast::parse`].
    ///
    /// The file is read through the same text reader [`analyze`] uses, so
    /// the bytes (after end-of-line normalization and UTF-8 BOM stripping)
    /// are byte-identical and `from_path(p)?.metrics(opts)` equals
    /// `analyze` over the same file. The detected language follows the same
    /// extension / shebang / mode-line rules as the CLI and `analyze`.
    ///
    /// Unlike [`analyze`], `from_path` is *no-magic*: it does **not** skip
    /// generated files — the caller asked for *this* file's tree, so a
    /// generated file is parsed like any other. It also does not run the
    /// C/C++ preprocessor pass (matching the in-memory parse path); the
    /// tree reflects the source as written.
    ///
    /// # Errors
    ///
    /// Returns a [`FromPathError`](crate::FromPathError) variant for each
    /// distinct failure: a real I/O fault
    /// ([`Io`](crate::FromPathError::Io)), a non-UTF-8 path
    /// ([`NonUtf8Path`](crate::FromPathError::NonUtf8Path)), an empty /
    /// binary / non-UTF-8 file
    /// ([`Unreadable`](crate::FromPathError::Unreadable)), an unrecognized
    /// language ([`UnknownLanguage`](crate::FromPathError::UnknownLanguage)),
    /// or a disabled-language build
    /// ([`Parse`](crate::FromPathError::Parse)). `from_path` never silently
    /// returns a tree-less success.
    pub fn from_path(path: &std::path::Path) -> Result<Self, crate::FromPathError> {
        use crate::FromPathError;

        // The path doubles as the FuncSpace name (an identifier); reject a
        // lossy conversion rather than corrupting it, matching `analyze`'s
        // strict default.
        let name = path.to_str().ok_or(FromPathError::NonUtf8Path)?.to_owned();
        let code = crate::read_file_with_eol(path)
            .map_err(FromPathError::Io)?
            .ok_or(FromPathError::Unreadable)?;
        let lang = crate::guess_language(&code, path)
            .0
            .ok_or(FromPathError::UnknownLanguage)?;
        Ok(Self::parse(
            Source::from_bytes(lang, code).with_name(Some(name)),
        )?)
    }

    /// Run the metric walker against the held parse. Safe to call
    /// repeatedly — the tree is reused.
    ///
    /// Two `metrics` calls with different [`MetricsOptions::with_only`]
    /// selections walk the tree twice; the savings versus [`analyze`] come
    /// from not re-parsing the source.
    ///
    /// # Errors
    ///
    /// The return type carries [`MetricsError::EmptyRoot`] for forward
    /// compatibility, but the walker always pushes a synthetic top-level
    /// [`SpaceKind::Unit`] [`FuncSpace`] before walking, so this method
    /// does not return `Err` in practice today.
    pub fn metrics(&self, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
        self.inner.run_metrics(self.name.clone(), options)
    }

    /// Return every operator and operand of each space in the held parse.
    ///
    /// The top-level [`crate::Ops::name`] is the `Source::name` supplied
    /// to [`Ast::parse`] / [`Ast::from_tree_sitter`] — carried explicitly
    /// rather than derived from a filesystem path via lossy UTF-8
    /// conversion, so [`crate::Ops::name_was_lossy`] is never set on this
    /// path. Safe to call repeatedly; the tree is reused.
    ///
    /// # Errors
    ///
    /// The return type carries [`MetricsError::EmptyRoot`] for forward
    /// compatibility, but the walker always pushes a synthetic top-level
    /// space before walking, so this method does not return `Err` in
    /// practice today (see the variant doc).
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let ops = Ast::parse(
    ///     Source::new(LANG::Cpp, b"int a = 42;")
    ///         .with_name(Some("foo.c".to_owned())),
    /// )
    /// .expect("cpp feature enabled")
    /// .ops()
    /// .expect("walker succeeds");
    /// assert_eq!(ops.name.as_deref(), Some("foo.c"));
    /// assert!(!ops.name_was_lossy);
    /// ```
    pub fn ops(&self) -> Result<crate::ops::Ops, MetricsError> {
        self.inner.run_ops(self.name.clone())
    }

    /// Source language of the parsed tree.
    #[must_use]
    #[inline]
    pub fn language(&self) -> LANG {
        self.inner.language()
    }

    /// Source bytes the held tree was parsed from. For [`LANG::Cpp`] with
    /// preprocessor inputs supplied to [`Ast::parse`], these are the
    /// *expanded* bytes (see the type-level "C++ preprocessor" note).
    #[must_use]
    #[inline]
    pub fn source(&self) -> &[u8] {
        self.inner.code_bytes()
    }

    /// Display name carried through to [`FuncSpace::name`] by every
    /// [`Ast::metrics`] call.
    #[must_use]
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Borrow the underlying [`tree_sitter::Tree`] for callers that want
    /// to drive their own traversal alongside the metric walker.
    ///
    /// The returned reference is valid only while `self` lives; nodes
    /// obtained from it must be resolved against [`Ast::source`] (the
    /// `tree_sitter::Tree` is lazy and lifetime-bound to that byte
    /// buffer).
    #[must_use]
    #[inline]
    pub fn as_tree_sitter(&self) -> &tree_sitter::Tree {
        self.inner.ts_tree()
    }

    /// Strip non-doc comments from the held parse, returning the source
    /// with those byte ranges removed. `None` when there is nothing to
    /// strip. Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let stripped = Ast::parse(Source::new(LANG::Rust, b"// gone\nfn f() {}\n"))
    ///     .expect("rust feature enabled")
    ///     .strip_comments()
    ///     .expect("a comment was present");
    /// assert!(!stripped.windows(2).any(|w| w == b"//"));
    /// ```
    #[must_use]
    pub fn strip_comments(&self) -> Option<Vec<u8>> {
        self.inner.run_strip_comments()
    }

    /// Detect the span of every function in the held parse. Safe to call
    /// repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let funcs = Ast::parse(Source::new(LANG::Rust, b"fn a() {}\nfn b() {}\n"))
    ///     .expect("rust feature enabled")
    ///     .functions();
    /// assert_eq!(funcs.len(), 2);
    /// ```
    #[must_use]
    pub fn functions(&self) -> Vec<crate::FunctionSpan> {
        self.inner.run_functions()
    }

    /// Build the [`AstResponse`](crate::AstResponse) node tree for the held
    /// parse under `cfg`. The `Source`-based counterpart of the deprecated
    /// `AstCallback` dispatch. Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, AstCfg, LANG, Source};
    ///
    /// let resp = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
    ///     .expect("rust feature enabled")
    ///     .dump(AstCfg {
    ///         id: String::new(),
    ///         language: "rust".to_owned(),
    ///         comment: false,
    ///         span: false,
    ///     });
    /// assert_eq!(resp.language, "rust");
    /// assert_eq!(resp.root.expect("root node").r#type, "source_file");
    /// ```
    #[must_use]
    pub fn dump(&self, cfg: crate::AstCfg) -> crate::AstResponse {
        self.inner.run_dump(cfg)
    }

    /// Count `(matching, total)` nodes in the held parse, where a node
    /// matches when its kind is named in `filters` (the same vocabulary
    /// the `bca count` CLI accepts — `all`, `call`, `comment`, `error`,
    /// `string`, `function`, a numeric `kind_id`, or an exact
    /// `node.kind()`). Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let (matching, total) = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
    ///     .expect("rust feature enabled")
    ///     .count(&["function_item".to_owned()]);
    /// assert_eq!(matching, 1);
    /// assert!(total > matching);
    /// ```
    #[must_use]
    pub fn count(&self, filters: &[String]) -> (usize, usize) {
        self.inner.run_count(filters)
    }

    /// Find every node in the held parse whose kind is named in
    /// `filters`. The returned [`Node`]s borrow the held tree, so they
    /// must be resolved against [`Ast::source`]. Safe to call
    /// repeatedly; the tree is reused.
    ///
    /// # Errors
    ///
    /// Currently infallible; the [`Result`] wrapper is reserved for a
    /// future strict-parsing mode (matching the other `Ast` walkers).
    pub fn find(&self, filters: &[String]) -> Result<Vec<Node<'_>>, MetricsError> {
        self.inner.run_find(filters)
    }

    /// Collect every in-source suppression marker (`// bca: suppress …`)
    /// in the held parse, sorted by line. Safe to call repeatedly; the
    /// tree is reused.
    #[must_use]
    pub fn suppressions(&self) -> Vec<crate::SuppressionMarker> {
        self.inner.run_suppressions()
    }

    /// Borrow the root [`Node`] of the held parse for callers that drive
    /// their own traversal (e.g. rendering an AST dump). Nodes obtained
    /// from it must be resolved against [`Ast::source`].
    #[must_use]
    #[inline]
    pub fn root_node(&self) -> Node<'_> {
        self.inner.root_node()
    }
}
