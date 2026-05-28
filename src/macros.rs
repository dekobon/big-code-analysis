// `get_language!` is invoked only from feature-gated arms in `mk_lang!`
// (one arm per `LANG::*` variant whose per-language Cargo feature is
// enabled). A build with `--no-default-features` and no language
// feature has no remaining call sites; suppress the lint for that
// pathological-but-valid configuration.
#[allow(unused_macros)]
macro_rules! get_language {
    (tree_sitter_cpp) => {
        tree_sitter_mozcpp::LANGUAGE.into()
    };
    (tree_sitter_typescript) => {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    (tree_sitter_tsx) => {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    };
    (tree_sitter_php) => {
        tree_sitter_php::LANGUAGE_PHP.into()
    };
    ($name:ident) => {
        $name::LANGUAGE.into()
    };
}

// `implement_metric_trait!` emits no-op `compute` bodies for every
// metric / language pair listed. Every named-trait arm below
// (`Abc`, `Cognitive`, `Halstead`, `Exit`, `Cyclomatic`, `Npa`,
// `Npm`, `Loc`, `Wmc`) is silent: the metric will report 0 on every
// input. The bracketed-trait arm (`[Trait]`) is different — it
// emits an empty `impl Trait for X {}` and relies on the trait's
// own default method body, which is correct for `Tokens`, `Nom`,
// and `NArgs`.
//
// Audit: #188 walked every `(language, metric)` cell and classified
// each as either a real default (the language has no construct the
// metric measures) or a placeholder (the language HAS the construct
// but no impl exists yet). Each invocation site carries a comment
// recording the rationale and any follow-up issue number — keep
// those comments in sync when you add a new language or land a real
// impl.
macro_rules! implement_metric_trait {
    (Abc, $($code:ident),+) => (
        implement_metric_trait!(@code_taking Abc, $($code),+);
    );
    (Cognitive, $($code:ident),+) => (
        $(
           impl Cognitive for $code {
               fn compute<'a>(
                   _node: &Node<'a>,
                   _code: &'a [u8],
                   _stats: &mut Stats,
                   _nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
               ) {}
           }
        )+
    );
    (Halstead, $($code:ident),+) => (
        $(
           impl Halstead for $code {
               fn compute<'a>(_node: &Node<'a>, _code: &'a [u8], _halstead_maps: &mut HalsteadMaps<'a>) {}
           }
        )+
    );
    // Internal helper: shared no-op body for traits whose `compute`
    // signature is `<'a>(&Node<'a>, &'a [u8], &mut Stats)` (Exit,
    // Cyclomatic). Public arms below delegate here so the body is
    // written once.
    (@code_taking $trait:ident, $($code:ident),+) => (
        $(
           impl $trait for $code {
               fn compute<'a>(_node: &Node<'a>, _code: &'a [u8], _stats: &mut Stats) {}
           }
        )+
    );
    (Exit, $($code:ident),+) => (
        implement_metric_trait!(@code_taking Exit, $($code),+);
    );
    (Cyclomatic, $($code:ident),+) => (
        implement_metric_trait!(@code_taking Cyclomatic, $($code),+);
    );
    (Npa, $($code:ident),+) => (
        implement_metric_trait!(@code_taking Npa, $($code),+);
    );
    (Npm, $($code:ident),+) => (
        implement_metric_trait!(@code_taking Npm, $($code),+);
    );
    (Loc, $($code:ident),+) => (
        $(
           impl Loc for $code {
               fn compute(_node: &Node, _stats: &mut Stats, _is_func_space: bool, _is_unit: bool) {}
           }
        )+
    );
    (Wmc, $($code:ident),+) => (
        $(
           impl Wmc for $code {
               fn compute(_space_kind: SpaceKind, _cyclomatic: &cyclomatic::Stats, _stats: &mut Stats) {}
           }
        )+
    );
    ([$trait:ident], $($code:ident),+) => (
        $(
           impl $trait for $code {}
        )+
    );
    ($trait:ident, $($code:ident),+) => (
        $(
           impl $trait for $code {
               fn compute(_node: &Node, _stats: &mut Stats) {}
           }
        )+
    )
}

macro_rules! mk_lang {
    ( $( ($feature:literal, $camel:ident, $name:ident, $display: expr, $description:expr) ),* ) => {
        /// The list of supported languages.
        ///
        /// Every variant is always defined regardless of the Cargo
        /// feature set: per-language features only gate the grammar
        /// crate references, never the enum surface itself. Disabled
        /// variants surface at runtime as
        /// [`crate::MetricsError::LanguageDisabled`] from every entry
        /// point that returns a `Result`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum LANG {
            $(
                #[doc = $description]
                $camel,
            )*
        }
        impl LANG {
            /// Return an iterator over the supported languages.
            ///
            /// # Examples
            ///
            /// ```
            /// use big_code_analysis::LANG;
            ///
            /// for lang in LANG::into_enum_iter() {
            ///     println!("{:?}", lang);
            /// }
            /// ```
            pub fn into_enum_iter() -> impl Iterator<Item=LANG> {
                use LANG::*;
                [$( $camel, )*].into_iter()
            }

            /// Returns the name of a language as a `&str`.
            ///
            /// # Examples
            ///
            /// ```
            /// use big_code_analysis::LANG;
            ///
            /// println!("{}", LANG::Rust.get_name());
            /// ```
            pub fn get_name(&self) -> &'static str {
                match self {
                    $(
                        LANG::$camel => $display,
                    )*
                }
            }

            /// Reports whether this variant's grammar crate is
            /// compiled into the current build.
            ///
            /// Returns `false` for variants whose per-language Cargo
            /// feature is disabled; calling
            /// [`Self::get_tree_sitter_language`], [`crate::analyze`],
            /// or any other dispatcher with such a variant will
            /// return [`crate::MetricsError::LanguageDisabled`].
            #[must_use]
            pub fn is_enabled(&self) -> bool {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        LANG::$camel => true,
                        #[cfg(not(feature = $feature))]
                        LANG::$camel => false,
                    )*
                }
            }

            // Returns a tree-sitter language paired with this variant,
            // or `Err(LanguageDisabled)` when the matching Cargo
            // feature is off. This is the internal entry point used
            // by `Tree::new` to construct a parser; the public
            // counterpart is `get_tree_sitter_language`.
            pub(crate) fn get_ts_language(&self) -> Result<Language, crate::MetricsError> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        LANG::$camel => Ok(get_language!($name)),
                        #[cfg(not(feature = $feature))]
                        LANG::$camel => Err(crate::MetricsError::LanguageDisabled(*self)),
                    )*
                }
            }

            /// Returns the [`tree_sitter::Language`] grammar used by
            /// this variant.
            ///
            /// Useful when feeding a caller-built
            /// [`tree_sitter::Parser`] into the
            /// [`crate::metrics_from_tree`] / [`crate::Parser::from_tree`]
            /// entry points — the language returned here is the one
            /// the metric walker expects for `kind_id` matching, so
            /// the trees agree structurally.
            ///
            /// This method is part of the value-not-stable surface:
            /// the underlying `tree-sitter-*` grammar pin may bump
            /// in any minor release, which can change `Language`
            /// equality on the caller side.
            ///
            /// # Errors
            ///
            /// Returns [`crate::MetricsError::LanguageDisabled`] when
            /// the variant's per-language Cargo feature is not
            /// enabled in the current build (see the `[features]`
            /// table in the root `Cargo.toml`).
            ///
            /// # Examples
            ///
            /// ```
            /// use big_code_analysis::LANG;
            ///
            /// let _lang = LANG::Rust.get_tree_sitter_language().expect("rust feature enabled");
            /// ```
            pub fn get_tree_sitter_language(&self) -> Result<::tree_sitter::Language, crate::MetricsError> {
                self.get_ts_language()
            }
        }
    };
}

macro_rules! mk_action {
    ( $( ($feature:literal, $camel:ident, $parser:ident) ),* ) => {
        /// Runs a function, which implements the [`Callback`] trait,
        /// on a code written in one of the supported languages.
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::LanguageDisabled`] when `lang`
        /// names a language whose per-language Cargo feature is not
        /// enabled in the current build (see the `[features]` table
        /// in the root `Cargo.toml`). All other failure modes are
        /// reported through the callback's own `T::Res`.
        ///
        /// # Examples
        ///
        /// The following example dumps to shell every metric computed using
        /// the dummy source code.
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// use big_code_analysis::{action, Callback, LANG, Metrics, MetricsCfg};
        ///
        /// let source_code = "int a = 42;";
        /// let language = LANG::Cpp;
        ///
        /// // The path to a dummy file used to contain the source code
        /// let path = PathBuf::from("foo.c");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        ///
        /// // Configuration options used by the function which computes the metrics
        /// let cfg = MetricsCfg::new(path);
        ///
        /// action::<Metrics>(&language, source_as_vec, &cfg.path.clone(), None, cfg)
        ///     .expect("cpp feature enabled");
        /// ```
        ///
        /// [`Callback`]: trait.Callback.html
        #[inline]
        pub fn action<T: Callback>(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>, cfg: T::Cfg) -> Result<T::Res, MetricsError> {
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => {
                        let parser = $parser::new(source, path, pr);
                        Ok(T::call(cfg, &parser))
                    },
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (source, path, pr, cfg);
                        Err(MetricsError::LanguageDisabled(*lang))
                    },
                )*
            }
        }

        /// Returns all function spaces data of a code.
        ///
        /// # Deprecated
        ///
        /// Prefer [`analyze`], which accepts a [`Source`] carrying an
        /// explicit display name distinct from any on-disk path. This
        /// shim derives [`FuncSpace::name`] from `path` via lossy
        /// UTF-8 conversion and remains for backwards compatibility
        /// for one minor release.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// # #[allow(deprecated)]
        /// use big_code_analysis::{get_function_spaces, LANG};
        ///
        /// let source_code = "int a = 42;";
        /// let language = LANG::Cpp;
        ///
        /// // The path to a dummy file used to contain the source code
        /// let path = PathBuf::from("foo.c");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        ///
        /// # #[allow(deprecated)]
        /// get_function_spaces(&language, source_as_vec, &path, None).unwrap();
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
        /// per-language Cargo feature is not enabled in this build.
        /// The return type also carries [`MetricsError::EmptyRoot`]
        /// for forward compatibility, but the walker does not produce
        /// it today — see the variant doc.
        #[deprecated(
            since = "0.0.26",
            note = "Use `analyze(Source::new(lang, &code).with_name(Some(name)), MetricsOptions::default())` instead — the path-positional shim derives the top-level FuncSpace name via lossy UTF-8 conversion."
        )]
        #[inline]
        pub fn get_function_spaces(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Result<FuncSpace, MetricsError> {
            #[allow(deprecated)]
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        metrics(&parser, &path)
                    },
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (source, path, pr);
                        Err(MetricsError::LanguageDisabled(*lang))
                    },
                )*
            }
        }

        /// Language-dispatched bundle of a parsed tree plus its
        /// source bytes, one variant per Cargo-feature-enabled
        /// language. The public seam is [`crate::Ast`]; this enum is
        /// the macro-generated internal carrier it wraps.
        ///
        /// With every per-language feature disabled this enum is a
        /// 0-variant uninhabited type. Each method below therefore
        /// terminates its `match self` with a
        /// `#[cfg(not(any(feature = …)))] _ => match *self {}` arm:
        /// stable Rust treats `&UninhabitedType` as inhabited (E0004),
        /// so the outer match needs a wildcard, and `match *self {}`
        /// is exhaustive over the uninhabited dereferenced value —
        /// divergent, no panic, no `unsafe`, statically unreachable in
        /// safe code because the public seam `crate::Ast` has only
        /// fallible constructors that return `Err(LanguageDisabled)`
        /// for every `LANG` variant under that build.
        ///
        /// When a method takes by-value parameters (see
        /// [`Self::run_metrics`]), prefix the divergent arm with
        /// `let _ = (param1, param2, …);` to silence
        /// `unused_variables` under `RUSTFLAGS=-D warnings` — the
        /// `match *self {}` body is `!`, so the consumed values are
        /// never actually dropped at runtime.
        pub(crate) enum AstInner {
            $(
                #[cfg(feature = $feature)]
                $camel($parser),
            )*
        }

        impl AstInner {
            /// Run the metric walker against the held parse. The
            /// caller passes `name` and `options` per call so a
            /// single `AstInner` can be reused with different metric
            /// subsets.
            pub(crate) fn run_metrics(
                &self,
                name: Option<String>,
                options: MetricsOptions,
            ) -> Result<FuncSpace, MetricsError> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => metrics_inner(parser, name, options),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => {
                        let _ = (name, options);
                        match *self {}
                    },
                }
            }

            pub(crate) fn language(&self) -> LANG {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(_) => LANG::$camel,
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            pub(crate) fn code_bytes(&self) -> &[u8] {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => parser.get_code(),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            pub(crate) fn ts_tree(&self) -> &::tree_sitter::Tree {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => parser.get_ts_tree(),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }
        }

        /// Internal parse-dispatch shim that backs [`crate::Ast::parse`].
        /// Lives in the `mk_action!` macro so each new language only
        /// has to declare its parser tag once.
        pub(crate) fn ast_parse_dispatch(
            lang: LANG,
            source: &[u8],
            preproc_path: Option<&Path>,
            preproc: Option<Arc<PreprocResults>>,
        ) -> Result<AstInner, MetricsError> {
            // `Parser::new` keys the C++ macro-expansion lookup off the
            // caller-supplied path; for callers analysing in-memory
            // snippets with no preprocessor path, fall back to an
            // empty `Path` ("") which the lookup ignores. The empty
            // path is *not* leaked into `FuncSpace::name` — that
            // is carried separately on `Ast`.
            let preproc_path = preproc_path.unwrap_or(Path::new(""));
            let source = source.to_vec();
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => Ok(AstInner::$camel($parser::new(source, preproc_path, preproc))),
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (source, preproc_path, preproc);
                        Err(MetricsError::LanguageDisabled(lang))
                    },
                )*
            }
        }

        /// Internal tree-adoption dispatch that backs
        /// [`crate::Ast::from_tree_sitter`].
        pub(crate) fn ast_from_tree_dispatch(
            lang: LANG,
            tree: ::tree_sitter::Tree,
            source: Vec<u8>,
        ) -> Result<AstInner, MetricsError> {
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => Ok(AstInner::$camel($parser::from_tree(tree, source))),
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (tree, source);
                        Err(MetricsError::LanguageDisabled(lang))
                    },
                )*
            }
        }

        /// Internal language-dispatch shim that backs [`analyze`].
        /// Delegates to [`ast_parse_dispatch`] + [`AstInner::run_metrics`]
        /// so the per-`LANG` match arm lives in exactly one place.
        #[doc(hidden)]
        pub fn analyze_dispatch(
            lang: LANG,
            source: &[u8],
            name: Option<String>,
            preproc_path: Option<&Path>,
            preproc: Option<Arc<PreprocResults>>,
            options: MetricsOptions,
        ) -> Result<FuncSpace, MetricsError> {
            ast_parse_dispatch(lang, source, preproc_path, preproc)?.run_metrics(name, options)
        }

        /// Returns all function spaces data of a code, applying the
        /// per-traversal flags in `options` (e.g.
        /// `exclude_tests: true` to elide Rust `#[cfg(test)]` /
        /// `#[test]` subtrees from every metric). Equivalent to
        /// [`get_function_spaces`] when `options` is the default.
        ///
        /// # Deprecated
        ///
        /// Prefer [`analyze`], which accepts a [`Source`] carrying an
        /// explicit display name distinct from any on-disk path.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// # #[allow(deprecated)]
        /// use big_code_analysis::{get_function_spaces_with_options, LANG, MetricsOptions};
        ///
        /// let source_code = "fn main() {}\n#[test] fn t() {}";
        /// let language = LANG::Rust;
        ///
        /// let path = PathBuf::from("foo.rs");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        /// let options = MetricsOptions::default().with_exclude_tests(true);
        ///
        /// # #[allow(deprecated)]
        /// get_function_spaces_with_options(&language, source_as_vec, &path, None, options).unwrap();
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
        /// per-language Cargo feature is not enabled in this build.
        /// The return type also carries [`MetricsError::EmptyRoot`]
        /// for forward compatibility, but the walker does not produce
        /// it today — see the variant doc.
        #[deprecated(
            since = "0.0.26",
            note = "Use `analyze(Source::new(lang, &code).with_name(Some(name)), options)` instead — the path-positional shim derives the top-level FuncSpace name via lossy UTF-8 conversion."
        )]
        #[inline]
        pub fn get_function_spaces_with_options(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
            #[allow(deprecated)]
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        metrics_with_options(&parser, &path, options)
                    },
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (source, path, pr, options);
                        Err(MetricsError::LanguageDisabled(*lang))
                    },
                )*
            }
        }

        /// Returns all function spaces data of a code, reusing a
        /// caller-supplied [`tree_sitter::Tree`] instead of running
        /// the bundled parser.
        ///
        /// Use this when the caller already drives `tree-sitter` for
        /// other purposes (e.g. an editor doing incremental
        /// reparsing) and wants the metric walker to share that
        /// parse. The supplied `tree` must have been produced from
        /// `source` with the [`tree_sitter::Language`] returned by
        /// [`LANG::get_tree_sitter_language`] for `lang`; a mismatch
        /// is not `unsafe` but yields nonsensical metric values.
        ///
        /// Equivalent to [`get_function_spaces_with_options`] on the
        /// same `(lang, source, path)` triple when the same tree is
        /// reproduced internally.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// use big_code_analysis::{
        ///     get_function_spaces, metrics_from_tree, tree_sitter, LANG,
        ///     MetricsOptions,
        /// };
        ///
        /// let source_code = "fn main() { if true { 1 } else { 2 }; }";
        /// let path = PathBuf::from("foo.rs");
        /// let source = source_code.as_bytes().to_vec();
        ///
        /// let mut parser = tree_sitter::Parser::new();
        /// parser
        ///     .set_language(
        ///         &LANG::Rust
        ///             .get_tree_sitter_language()
        ///             .expect("rust feature enabled"),
        ///     )
        ///     .expect("rust grammar pinned to a compatible version");
        /// let tree = parser
        ///     .parse(&source, None)
        ///     .expect("parser has a language set");
        ///
        /// let from_tree = metrics_from_tree(
        ///     &LANG::Rust,
        ///     tree,
        ///     source.clone(),
        ///     &path,
        ///     None,
        ///     MetricsOptions::default(),
        /// )
        /// .unwrap();
        /// # #[allow(deprecated)]
        /// let from_bytes =
        ///     get_function_spaces(&LANG::Rust, source, &path, None).unwrap();
        ///
        /// assert_eq!(
        ///     from_tree.metrics.cyclomatic.cyclomatic_sum(),
        ///     from_bytes.metrics.cyclomatic.cyclomatic_sum(),
        /// );
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
        /// per-language Cargo feature is not enabled in this build.
        /// The return type also carries [`MetricsError::EmptyRoot`]
        /// for forward compatibility, but the walker does not produce
        /// it today — see the variant doc.
        #[inline]
        pub fn metrics_from_tree(
            lang: &LANG,
            tree: ::tree_sitter::Tree,
            source: Vec<u8>,
            path: &Path,
            pr: Option<Arc<PreprocResults>>,
            options: MetricsOptions,
        ) -> Result<FuncSpace, MetricsError> {
            // `pr` is accepted for parity with the byte-based entry
            // points so callers can swap one for the other without
            // changing call shape. Today only the C/C++ pre-pass uses
            // it, and that pre-pass runs before parsing — if the
            // caller built the tree themselves, they have already
            // accepted whatever macro expansion (or lack thereof) the
            // tree reflects, so the parameter is currently a no-op.
            let _ = pr;
            // Same path-name handling as the deprecated entry points
            // so existing callers see no behaviour change. Callers
            // who want to drop the lossy round-trip should use
            // [`crate::Ast::from_tree_sitter`], which carries an
            // explicit `name: Option<String>` end-to-end.
            let name = Some(path.to_string_lossy().into_owned());
            ast_from_tree_dispatch(*lang, tree, source)?.run_metrics(name, options)
        }

        /// Returns all operators and operands of each space in a code.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// use big_code_analysis::{get_ops, LANG};
        ///
        /// # fn main() {
        /// let source_code = "int a = 42;";
        /// let language = LANG::Cpp;
        ///
        /// // The path to a dummy file used to contain the source code
        /// let path = PathBuf::from("foo.c");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        ///
        /// get_ops(&language, source_as_vec, &path, None).unwrap();
        /// # }
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
        /// per-language Cargo feature is not enabled in this build.
        /// The return type also carries [`MetricsError::EmptyRoot`]
        /// for forward compatibility, but the walker does not produce
        /// it today — see the variant doc.
        #[inline]
        pub fn get_ops(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Result<Ops, MetricsError> {
            match lang {
                $(
                    #[cfg(feature = $feature)]
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        operands_and_operators(&parser, &path)
                    },
                    #[cfg(not(feature = $feature))]
                    LANG::$camel => {
                        let _ = (source, path, pr);
                        Err(MetricsError::LanguageDisabled(*lang))
                    },
                )*
            }
        }
    };
}

macro_rules! mk_extensions {
    ( $( ($camel:ident, [ $( $ext:ident ),* ]) ),* ) => {
        /// Detects the language associated to the input file extension.
        ///
        /// # Examples
        ///
        /// ```
        /// use big_code_analysis::get_from_ext;
        ///
        /// let ext = "rs";
        ///
        /// get_from_ext(ext).unwrap();
        /// ```
        pub fn get_from_ext(ext: &str) -> Option<LANG>{
            match ext {
                $(
                    $(
                        stringify!($ext) => Some(LANG::$camel),
                    )*
                )*
                _ => None,
            }
        }

        impl LANG {
            /// Returns the file extensions recognised for this language.
            ///
            /// The returned list is the same one consulted by
            /// [`get_from_ext`] and [`crate::get_language_for_file`].
            /// Helper variants without user-facing files (`Ccomment`,
            /// `Preproc`) return an empty slice.
            ///
            /// # Examples
            ///
            /// ```
            /// use big_code_analysis::LANG;
            ///
            /// assert!(LANG::Rust.get_extensions().contains(&"rs"));
            /// ```
            #[must_use]
            pub fn get_extensions(&self) -> &'static [&'static str] {
                match self {
                    $(
                        LANG::$camel => &[ $( stringify!($ext), )* ],
                    )*
                }
            }
        }
    };
}

macro_rules! mk_emacs_mode {
    ( $( ($camel:ident, [ $( $emacs_mode:expr ),* ]) ),* ) => {
        /// Detects the language associated to the input `Emacs` mode.
        ///
        /// An `Emacs` mode is used to detect a language according to
        /// particular text-information contained in a file.
        ///
        /// # Examples
        ///
        /// ```
        /// use big_code_analysis::get_from_emacs_mode;
        ///
        /// let emacs_mode = "rust";
        ///
        /// get_from_emacs_mode(emacs_mode).unwrap();
        /// ```
        pub fn get_from_emacs_mode(mode: &str) -> Option<LANG>{
            match mode {
                $(
                    $(
                        $emacs_mode => Some(LANG::$camel),
                    )*
                )*
                _ => None,
            }
        }
    };
}

macro_rules! mk_code {
    ( $( ($camel:ident, $code:ident, $parser:ident, $name:ident, $docname:expr) ),* ) => {
        $(
            #[doc = concat!("Per-language code type tag for ", $docname, "; carries no data.")]
            pub struct $code { _guard: (), }

            impl LanguageInfo for $code {
                type BaseLang = $camel;

                fn get_lang() -> LANG {
                    LANG::$camel
                }

                fn get_lang_name() -> &'static str {
                    $docname
                }
            }

            #[doc = "The `"]
            #[doc = $docname]
            #[doc = "` language parser."]
            pub type $parser = Parser<$code>;
        )*
    };
}

macro_rules! mk_langs {
    ( $( ($feature:literal, $camel:ident, $description: expr, $display: expr, $code:ident, $parser:ident, $name:ident, [ $( $ext:ident ),* ], [ $( $emacs_mode:expr ),* ]) ),* ) => {
        mk_lang!($( ($feature, $camel, $name, $display, $description) ),*);
        mk_action!($( ($feature, $camel, $parser) ),*);
        mk_extensions!($( ($camel, [ $( $ext ),* ]) ),*);
        mk_emacs_mode!($( ($camel, [ $( $emacs_mode ),* ]) ),*);
        mk_code!($( ($camel, $code, $parser, $name, stringify!($camel)) ),*);
    };
}

// Aliased C# `kind_id` unions. The C# tree-sitter grammar emits multiple
// numbered variants for several rules (lesson #2 in
// `docs/development/lessons_learned.md`); centralizing the alias sets
// here keeps every match site in lockstep, so a future grammar bump that
// adds another numbered variant is a one-line edit instead of a scatter
// of 4-5 sites.
macro_rules! csharp_invocation_expr_kinds {
    () => {
        $crate::Csharp::InvocationExpression
            | $crate::Csharp::InvocationExpression2
            | $crate::Csharp::InvocationExpression3
    };
}

macro_rules! csharp_paren_expr_kinds {
    () => {
        $crate::Csharp::ParenthesizedExpression
            | $crate::Csharp::ParenthesizedExpression2
            | $crate::Csharp::ParenthesizedExpression3
    };
}

macro_rules! csharp_prefix_unary_expr_kinds {
    () => {
        $crate::Csharp::PrefixUnaryExpression | $crate::Csharp::PrefixUnaryExpression2
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the C# grammar. Anything in this set, when it appears in a known-
// boolean context (if / while / do / for / ternary / binary), counts
// as one condition. The set bundles `csharp_invocation_expr_kinds!()`
// with the bare `Identifier` / `BooleanLiteral` leaves *and* the five
// expression kinds whose evaluated value is implicitly boolean in any
// idiomatic codebase:
//
// - `MemberAccessExpression` — `cfg.Enabled`, `Request.IsHttps`
// - `AwaitExpression`        — `await CheckAsync()`
// - `CastExpression`         — `(bool)v`, `(IDisposable)x is not null`
// - `IsPatternExpression`    — `x is null`, `x is not Foo f`
// - `ElementAccessExpression` — `flags[0]`, `dict["key"]`
//
// Before #372 only the first three (invocation / identifier /
// boolean) were recognised, so all five kinds above silently scored
// zero conditions in `if` / `while` / `do` / ternary contexts.
macro_rules! csharp_bool_terminal_kinds {
    () => {
        $crate::Csharp::InvocationExpression
            | $crate::Csharp::InvocationExpression2
            | $crate::Csharp::InvocationExpression3
            | $crate::Csharp::Identifier
            | $crate::Csharp::BooleanLiteral
            | $crate::Csharp::MemberAccessExpression
            | $crate::Csharp::AwaitExpression
            | $crate::Csharp::CastExpression
            | $crate::Csharp::IsPatternExpression
            | $crate::Csharp::ElementAccessExpression
    };
}

macro_rules! csharp_var_decl_kinds {
    () => {
        $crate::Csharp::VariableDeclaration | $crate::Csharp::VariableDeclaration2
    };
}

macro_rules! csharp_var_declarator_kinds {
    () => {
        $crate::Csharp::VariableDeclarator | $crate::Csharp::VariableDeclarator2
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the Java grammar. Sister of `csharp_bool_terminal_kinds!()` — bundles
// the four "bare boolean leaf" kinds (`MethodInvocation`, `Identifier`,
// `True`, `False`) with the four bool-evaluating expression kinds
// surfaced by #372 / lesson #19:
//
// - `FieldAccess`          — `cfg.flag`
// - `CastExpression`       — `(boolean) v`
// - `ArrayAccess`          — `flags[0]`
// - `InstanceofExpression` — `x instanceof Foo`
//
// Used by `java_inspect_container`, `java_count_unary_conditions`,
// `java_walk_ternary`, and the two branches of `java_walk_for_statement`
// (the latter ORs in `SEMI | RPAREN` at the call site to also recognise
// the empty-condition `for (;;)` form).
macro_rules! java_bool_terminal_kinds {
    () => {
        $crate::Java::MethodInvocation
            | $crate::Java::Identifier
            | $crate::Java::True
            | $crate::Java::False
            | $crate::Java::FieldAccess
            | $crate::Java::CastExpression
            | $crate::Java::ArrayAccess
            | $crate::Java::InstanceofExpression
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the dekobon Groovy grammar. Sister of `java_bool_terminal_kinds!()`,
// with Groovy-specific replacements: `CommandChain` for the parens-less
// call form `println foo`, `BooleanLiteral` (the named wrapper around
// the leaf `True` / `False` tokens, see `groovy_count_condition`), and
// `ParenthesizedTypeCast` for the Java-style `(boolean) v` form (the
// grammar represents it as its own kind rather than nesting
// `cast_expression` inside `parenthesized_expression`). The set bundles
// the bool-evaluating terminals added by #372 (`FieldAccess`,
// `CastExpression`, `ParenthesizedTypeCast`, `InstanceofExpression`);
// the dekobon Groovy grammar has no `await` or `array_access`
// analogues, so those collapse out of the C# set.
macro_rules! groovy_bool_terminal_kinds {
    () => {
        $crate::Groovy::MethodInvocation
            | $crate::Groovy::CommandChain
            | $crate::Groovy::Identifier
            | $crate::Groovy::BooleanLiteral
            | $crate::Groovy::FieldAccess
            | $crate::Groovy::CastExpression
            | $crate::Groovy::ParenthesizedTypeCast
            | $crate::Groovy::InstanceofExpression
    };
}

// Terminal-bool operand kinds for the Phase-2 unary-conditional walker
// (issue #403). Each `<lang>_bool_terminal_kinds!()` macro lists the
// expression kinds whose evaluated value is implicitly boolean in an
// `if` / `while` / `&&` / `||` operand slot for that language. Each
// per-language walker pair (`<lang>_inspect_container` +
// `<lang>_count_unary_conditions`) consumes the same set in both
// helpers, so hoisting to a macro removes the literal duplication.

macro_rules! rust_bool_terminal_kinds {
    // `ScopedIdentifier` (`crate::FLAG`, `ns::flag`) and
    // `AwaitExpression` (`ready().await`) are both idiomatic shapes
    // for a boolean-valued condition operand. Adding them mirrors
    // the C# fix in #372 (lesson 19), which closed the same gap
    // for `CastExpression`, `MemberAccessExpression`, and
    // `AwaitExpression` on the C# side.
    () => {
        $crate::Rust::Identifier
            | $crate::Rust::BooleanLiteral
            | $crate::Rust::CallExpression
            | $crate::Rust::FieldExpression
            | $crate::Rust::IndexExpression
            | $crate::Rust::ScopedIdentifier
            | $crate::Rust::AwaitExpression
    };
}

macro_rules! go_bool_terminal_kinds {
    // Aliased Identifier kind_ids (lesson #2): tree-sitter-go emits
    // `identifier` under three numeric ids (1, 60, 61) depending on
    // the production rule path. Halstead's getter already matches
    // all three at `src/getter.rs:881`.
    () => {
        $crate::Go::Identifier
            | $crate::Go::Identifier2
            | $crate::Go::Identifier3
            | $crate::Go::True
            | $crate::Go::False
            | $crate::Go::CallExpression
            | $crate::Go::SelectorExpression
            | $crate::Go::IndexExpression
            | $crate::Go::TypeAssertionExpression
    };
}

macro_rules! cpp_bool_terminal_kinds {
    // `QualifiedIdentifier` has four numeric kind_ids (573..576) per
    // tree-sitter-cpp's production-rule path. Halstead's getter
    // already matches all four; the ABC walker needs them too so
    // `if (ns::flag) {}` reaches the terminal-bool count.
    //
    // `CastExpression` (`(bool)v`) evaluates to a boolean in
    // idiomatic C++ — mirrors the C# fix in #372 (lesson 19).
    () => {
        $crate::Cpp::Identifier
            | $crate::Cpp::True
            | $crate::Cpp::False
            | $crate::Cpp::CallExpression
            | $crate::Cpp::CallExpression2
            | $crate::Cpp::FieldExpression
            | $crate::Cpp::SubscriptExpression
            | $crate::Cpp::CastExpression
            | $crate::Cpp::QualifiedIdentifier
            | $crate::Cpp::QualifiedIdentifier2
            | $crate::Cpp::QualifiedIdentifier3
            | $crate::Cpp::QualifiedIdentifier4
    };
}

macro_rules! php_bool_terminal_kinds {
    // Aliased kind_ids (lesson 2):
    //   - `name` has two ids (1, 211)
    //   - `member_access_expression` has three (328, 329, 360)
    //   - `nullsafe_member_access_expression` has two (330, 331)
    //   - `scoped_property_access_expression` has two (332, 333)
    //   - `subscript_expression` has three (351, 352, 363)
    // The matching `*_call_expression` kinds remain singular at the
    // pinned grammar version. Including the property-access form
    // (`$x?->y`, `$x->y`, and `Cls::$x`) closes the bool-typed-
    // property-access gap that the call-form alone left open.
    () => {
        $crate::Php::Name
            | $crate::Php::Name2
            | $crate::Php::VariableName
            | $crate::Php::Boolean
            | $crate::Php::FunctionCallExpression
            | $crate::Php::MemberCallExpression
            | $crate::Php::ScopedCallExpression
            | $crate::Php::NullsafeMemberCallExpression
            | $crate::Php::ObjectCreationExpression
            | $crate::Php::MemberAccessExpression
            | $crate::Php::MemberAccessExpression2
            | $crate::Php::MemberAccessExpression3
            | $crate::Php::NullsafeMemberAccessExpression
            | $crate::Php::NullsafeMemberAccessExpression2
            | $crate::Php::ScopedPropertyAccessExpression
            | $crate::Php::ScopedPropertyAccessExpression2
            | $crate::Php::SubscriptExpression
            | $crate::Php::SubscriptExpression2
            | $crate::Php::SubscriptExpression3
    };
}

macro_rules! python_bool_terminal_kinds {
    // `Await` (`await ready()`) evaluates to a boolean in idiomatic
    // async Python — mirrors the C# fix in #372 (lesson 19) which
    // closed the same gap for `AwaitExpression`.
    () => {
        $crate::Python::Identifier
            | $crate::Python::True
            | $crate::Python::False
            | $crate::Python::Call
            | $crate::Python::Attribute
            | $crate::Python::Subscript
            | $crate::Python::Await
    };
}

macro_rules! perl_bool_terminal_kinds {
    () => {
        $crate::Perl::Identifier
            | $crate::Perl::Boolean
            | $crate::Perl::True
            | $crate::Perl::False
            | $crate::Perl::ScalarVariable
            | $crate::Perl::ArrayVariable
            | $crate::Perl::HashVariable
            | $crate::Perl::ArrayAccessVariable
            | $crate::Perl::HashAccessVariable
            | $crate::Perl::HashAccessVariableSimple
            | $crate::Perl::CallExpressionWithSpacedArgs
            | $crate::Perl::CallExpressionWithSub
            | $crate::Perl::CallExpressionWithArgsWithBrackets
            | $crate::Perl::CallExpressionWithVariable
            | $crate::Perl::CallExpressionRecursive
            | $crate::Perl::CallExpressionWithBareword
            | $crate::Perl::MethodInvocation
    };
}

macro_rules! lua_bool_terminal_kinds {
    () => {
        $crate::Lua::Identifier
            | $crate::Lua::True
            | $crate::Lua::False
            | $crate::Lua::Nil
            | $crate::Lua::Number
            | $crate::Lua::FunctionCall
            | $crate::Lua::DotIndexExpression
            | $crate::Lua::DotIndexExpression2
            | $crate::Lua::BracketIndexExpression
            | $crate::Lua::MethodIndexExpression
            | $crate::Lua::MethodIndexExpression2
    };
}

macro_rules! tcl_bool_terminal_kinds {
    () => {
        $crate::Tcl::SimpleWord
            | $crate::Tcl::BracedWord
            | $crate::Tcl::BracedWordSimple
            | $crate::Tcl::QuotedWord
            | $crate::Tcl::VariableSubstitution
            | $crate::Tcl::CommandSubstitution
            | $crate::Tcl::Boolean
            | $crate::Tcl::Number
    };
}

// The JS-family languages diverge on which aliased `kind_id`s the
// grammar emits — JavaScript, Mozjs, and Tsx have `Identifier2`,
// TypeScript does not; TypeScript has `MemberExpression4` /
// `CallExpression4` / `SubscriptExpression2` that the others do not.
// Per lesson #2, every alias the grammar emits at runtime must be
// matched at compile time. Four per-language macros below replace
// the original single `js_family_bool_terminal_kinds!($Lang)`
// generic, which silently dropped `MemberExpression2` (the kind
// runtime emits for `obj.foo`) for all four languages.

macro_rules! javascript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Javascript::Identifier
            | $crate::Javascript::Identifier2
            | $crate::Javascript::True
            | $crate::Javascript::False
            | $crate::Javascript::CallExpression
            | $crate::Javascript::CallExpression2
            | $crate::Javascript::NewExpression
            | $crate::Javascript::MemberExpression
            | $crate::Javascript::MemberExpression2
            | $crate::Javascript::MemberExpression3
            | $crate::Javascript::SubscriptExpression
            | $crate::Javascript::AwaitExpression
    };
}

macro_rules! mozjs_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Mozjs::Identifier
            | $crate::Mozjs::Identifier2
            | $crate::Mozjs::True
            | $crate::Mozjs::False
            | $crate::Mozjs::CallExpression
            | $crate::Mozjs::CallExpression2
            | $crate::Mozjs::NewExpression
            | $crate::Mozjs::MemberExpression
            | $crate::Mozjs::MemberExpression2
            | $crate::Mozjs::MemberExpression3
            | $crate::Mozjs::SubscriptExpression
            | $crate::Mozjs::AwaitExpression
    };
}

macro_rules! typescript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Typescript::Identifier
            | $crate::Typescript::True
            | $crate::Typescript::False
            | $crate::Typescript::CallExpression
            | $crate::Typescript::CallExpression2
            | $crate::Typescript::CallExpression3
            | $crate::Typescript::CallExpression4
            | $crate::Typescript::NewExpression
            | $crate::Typescript::MemberExpression
            | $crate::Typescript::MemberExpression2
            | $crate::Typescript::MemberExpression3
            | $crate::Typescript::MemberExpression4
            | $crate::Typescript::SubscriptExpression
            | $crate::Typescript::SubscriptExpression2
            | $crate::Typescript::AwaitExpression
    };
}

macro_rules! tsx_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Tsx::Identifier
            | $crate::Tsx::Identifier2
            | $crate::Tsx::True
            | $crate::Tsx::False
            | $crate::Tsx::CallExpression
            | $crate::Tsx::CallExpression2
            | $crate::Tsx::CallExpression3
            | $crate::Tsx::CallExpression4
            | $crate::Tsx::NewExpression
            | $crate::Tsx::MemberExpression
            | $crate::Tsx::MemberExpression2
            | $crate::Tsx::MemberExpression3
            | $crate::Tsx::MemberExpression4
            | $crate::Tsx::SubscriptExpression
            | $crate::Tsx::SubscriptExpression2
            | $crate::Tsx::AwaitExpression
    };
}

// Legacy single-macro form, no longer consumed by the walker after
// the per-language split above. Kept here strictly for documentation
// of the former (Identifier|True|False|CallExpression|NewExpression|
// MemberExpression|SubscriptExpression) intersection that all four
// JS-family languages share — every per-language macro above is a
// strict superset.
#[allow(unused_macros)]
macro_rules! js_family_bool_terminal_kinds {
    ($Lang:ident) => {
        $crate::$Lang::Identifier
            | $crate::$Lang::True
            | $crate::$Lang::False
            | $crate::$Lang::CallExpression
            | $crate::$Lang::NewExpression
            | $crate::$Lang::MemberExpression
            | $crate::$Lang::SubscriptExpression
    };
}

pub(crate) use implement_metric_trait;
pub(crate) use {
    cpp_bool_terminal_kinds, csharp_bool_terminal_kinds, csharp_invocation_expr_kinds,
    csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds, csharp_var_decl_kinds,
    csharp_var_declarator_kinds, get_language, go_bool_terminal_kinds, groovy_bool_terminal_kinds,
    java_bool_terminal_kinds, javascript_bool_terminal_kinds, lua_bool_terminal_kinds, mk_action,
    mk_code, mk_emacs_mode, mk_extensions, mk_lang, mk_langs, mozjs_bool_terminal_kinds,
    perl_bool_terminal_kinds, php_bool_terminal_kinds, python_bool_terminal_kinds,
    rust_bool_terminal_kinds, tcl_bool_terminal_kinds, tsx_bool_terminal_kinds,
    typescript_bool_terminal_kinds,
};
