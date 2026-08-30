// `get_language!` is invoked only from feature-gated arms in `mk_lang!`
// (one arm per `LANG::*` variant whose per-language Cargo feature is
// enabled). A build with `--no-default-features` and no language
// feature has no remaining call sites; suppress the lint for that
// pathological-but-valid configuration.
#[allow(unused_macros)]
macro_rules! get_language {
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
// own default method body, which is correct for `Mi`, `Tokens`,
// `Nom`, and `NArgs`.
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
        implement_metric_trait!(@code_and_chain_taking Abc, $($code),+);
    );
    (Cognitive, $($code:ident),+) => (
        $(
           impl Cognitive for $code {
               fn compute<'a>(
                   _node: &Node<'a>,
                   _code: &'a [u8],
                   _ancestors: crate::Ancestors<'a, '_>,
                   _stats: &mut Stats,
                   _nesting_map: &mut crate::spaces::NestingMap,
               ) {}
           }
        )+
    );
    (Halstead, $($code:ident),+) => (
        $(
           impl Halstead for $code {
               fn compute<'a>(
                   _node: &Node<'a>,
                   _code: &'a [u8],
                   _ancestors: crate::Ancestors<'a, '_>,
                   _halstead_maps: &mut HalsteadMaps<'a>,
               ) {}
           }
        )+
    );
    // Internal helper: shared no-op body for traits whose `compute`
    // signature is `<'a>(&Node<'a>, &'a [u8], Ancestors<'a, '_>,
    // &mut Stats)` (Abc, Cyclomatic). Public arms below delegate here
    // so the body is written once. `Npa` and `Npm` share the signature
    // but need `HAS_MEMBERS = false` as well, so they route through
    // `@code_and_chain_taking_memberless` instead — reaching for this
    // arm for a new no-op `Npa` / `Npm` impl would silently restore
    // the all-zero file-root block #1203 removed.
    (@code_and_chain_taking $trait:ident, $($code:ident),+) => (
        $(
           impl $trait for $code {
               fn compute<'a>(
                   _node: &Node<'a>,
                   _code: &'a [u8],
                   _ancestors: crate::Ancestors<'a, '_>,
                   _stats: &mut Stats,
               ) {}
           }
        )+
    );
    // `Exit` is the one metric whose `compute` still takes no ancestor
    // chain: no language's exit rule asks what encloses the node.
    (Exit, $($code:ident),+) => (
        $(
           impl Exit for $code {
               fn compute<'a>(_node: &Node<'a>, _code: &'a [u8], _stats: &mut Stats) {}
           }
        )+
    );
    (Cyclomatic, $($code:ident),+) => (
        implement_metric_trait!(@code_and_chain_taking Cyclomatic, $($code),+);
    );
    // `Npa` and `Npm` take the same shape as the arm above plus one
    // thing: the no-op impl must also opt the language out of
    // *emitting* the block, which `HAS_MEMBERS` does. Without it a shell
    // script would report `class_npa_sum: 0`, because the file unit is a
    // member scope like any other and the walker would record its kind
    // (#1203). `wmc` reaches the same place by different means — its
    // no-op `compute` simply never records a kind.
    (@code_and_chain_taking_memberless $trait:ident, $($code:ident),+) => (
        $(
           impl $trait for $code {
               const HAS_MEMBERS: bool = false;

               fn compute<'a>(
                   _node: &Node<'a>,
                   _code: &'a [u8],
                   _ancestors: crate::Ancestors<'a, '_>,
                   _stats: &mut Stats,
               ) {}
           }
        )+
    );
    (Npa, $($code:ident),+) => (
        implement_metric_trait!(@code_and_chain_taking_memberless Npa, $($code),+);
    );
    (Npm, $($code:ident),+) => (
        implement_metric_trait!(@code_and_chain_taking_memberless Npm, $($code),+);
    );
    (Loc, $($code:ident),+) => (
        $(
           impl Loc for $code {
               fn compute(
                   _node: &Node,
                   _ancestors: crate::Ancestors<'_, '_>,
                   _stats: &mut Stats,
                   _is_func_space: bool,
               ) {}
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
    ( $( ($feature:literal, $camel:ident, $name:ident, $display: expr, $description:expr, $version:literal) ),* ) => {
        /// The list of supported languages.
        ///
        /// Every variant is always defined regardless of the Cargo
        /// feature set: per-language features only gate the grammar
        /// crate references, never the enum surface itself. Disabled
        /// variants surface at runtime as
        /// [`crate::MetricsError::LanguageDisabled`] from every entry
        /// point that returns a `Result`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            /// println!("{}", LANG::Rust.name());
            /// ```
            pub fn name(&self) -> &'static str {
                match self {
                    $(
                        LANG::$camel => $display,
                    )*
                }
            }

            /// Returns the pinned tree-sitter grammar crate version that
            /// backs this variant (e.g. `"0.25.1"` for [`LANG::Bash`]).
            ///
            /// The value mirrors the `=X.Y.Z` pin in the workspace
            /// `Cargo.toml` and is independent of the per-language Cargo
            /// feature: it is returned even for a variant whose feature is
            /// disabled in the current build (a build-time constant, no
            /// grammar crate reference). A drift test in `src/langs.rs`
            /// asserts every value here matches the manifest pin.
            ///
            /// # Grammars vs. forks
            ///
            /// For languages backed by an upstream crates.io grammar
            /// (`bash`, `rust`, `python`, `typescript`, …) this is the
            /// exact upstream grammar version, so a consumer migrating
            /// matchers off py-tree-sitter can line node-kind vocabularies
            /// up against the same pin. For the vendored big-code-analysis
            /// forks (`mozcpp`, `mozjs`, `tcl`, `ccomment`, `preproc`,
            /// `kotlin`) the value is the **fork crate's** version
            /// (published as `bca-tree-sitter-*` / `tree-sitter-kotlin-ng`),
            /// not an upstream tree-sitter grammar semver — there is no
            /// upstream release to compare against.
            ///
            /// This is part of the value-not-stable surface: the returned
            /// version changes whenever the grammar pin is bumped.
            #[must_use]
            pub fn grammar_version(&self) -> &'static str {
                match self {
                    $(
                        LANG::$camel => $version,
                    )*
                }
            }

            /// Reports whether this variant's grammar crate is
            /// compiled into the current build.
            ///
            /// Returns `false` for variants whose per-language Cargo
            /// feature is disabled; calling
            /// [`Self::tree_sitter_language`], [`crate::analyze`],
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
            // counterpart is `tree_sitter_language`.
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
            /// [`crate::Ast::from_tree_sitter`] entry point — the
            /// language returned here is the one the metric walker
            /// expects for `kind_id` matching, so the trees agree
            /// structurally.
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
            /// let _lang = LANG::Rust.tree_sitter_language().expect("rust feature enabled");
            /// ```
            pub fn tree_sitter_language(&self) -> Result<::tree_sitter::Language, crate::MetricsError> {
                self.get_ts_language()
            }
        }

        /// Renders the language's canonical lowercase slug, identical to
        /// [`LANG::name`].
        ///
        /// Every variant has a distinct slug, so `Display` is injective
        /// and a `Display` → [`FromStr`](std::str::FromStr) round-trip
        /// returns the original variant (see the round-trip test in
        /// `src/langs.rs`). The slug is the single canonical identifier
        /// used across every surface (CLI JSON, web `/metrics`, the
        /// Python bindings): it contains no punctuation and is always a
        /// valid `FromStr` lookup token.
        impl ::std::fmt::Display for LANG {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.name())
            }
        }

        /// Parses a [`LANG`] from its [`Display`](std::fmt::Display)
        /// spelling (the canonical lowercase [`LANG::name`] slug, e.g.
        /// `"rust"`, `"cpp"`, `"csharp"`, `"tsx"`).
        ///
        /// Matching is case-sensitive and exact, mirroring
        /// [`Metric`](crate::Metric)'s `FromStr`: only the canonical
        /// lowercase slug is accepted. File extensions and emacs modes
        /// are deliberately *not* accepted here — use
        /// [`get_from_ext`](crate::get_from_ext) /
        /// [`get_from_emacs_mode`](crate::get_from_emacs_mode) for those.
        ///
        /// Every variant has a distinct slug, so this is the exact
        /// inverse of [`Display`](std::fmt::Display): the round-trip
        /// `LANG::from_str(&lang.to_string())` returns the original
        /// variant for every `LANG`.
        impl ::std::str::FromStr for LANG {
            type Err = $crate::macros::ParseLangError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                LANG::into_enum_iter()
                    .find(|lang| lang.name() == s)
                    .ok_or_else(|| $crate::macros::ParseLangError::new(s))
            }
        }
    };
}

/// Error returned by [`LANG`](crate::LANG)'s
/// [`FromStr`](std::str::FromStr) impl when the input is not a
/// recognised language name.
///
/// Holds the offending input verbatim so wrapper layers can format
/// their own user-facing message; mirrors
/// [`ParseMetricError`](crate::ParseMetricError).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLangError(String);

impl ParseLangError {
    // Constructor kept `pub(crate)` so the macro-generated `FromStr`
    // impl in `src/langs.rs` can build the error without exposing the
    // private field across module boundaries.
    pub(crate) fn new(input: &str) -> Self {
        Self(input.to_owned())
    }

    /// The rejected input that failed to parse as a language name.
    ///
    /// Lets callers recover the offending string programmatically
    /// rather than scraping it out of the [`Display`](std::fmt::Display)
    /// output. Mirrors
    /// [`ParseMetricError::input`](crate::ParseMetricError::input).
    #[must_use]
    pub fn input(&self) -> &str {
        &self.0
    }
}

impl ::std::fmt::Display for ParseLangError {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        write!(f, "unknown language: {}", self.0)
    }
}

impl ::std::error::Error for ParseLangError {}

macro_rules! mk_action {
    ( $( ($feature:literal, $camel:ident, $parser:ident) ),* ) => {
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

            /// Run the operator/operand walk against the held parse,
            /// carrying an explicit `name` end-to-end. Backs
            /// [`crate::Ast::ops`]; the ops analogue of [`Self::run_metrics`].
            pub(crate) fn run_ops(
                &self,
                name: Option<String>,
            ) -> Result<Ops, MetricsError> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => ops_inner(parser, name),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => {
                        let _ = name;
                        match *self {}
                    },
                }
            }

            /// Strip comments from the held parse. Backs
            /// [`crate::Ast::strip_comments`]; the comment-removal analogue
            /// of [`Self::run_ops`].
            pub(crate) fn run_strip_comments(&self) -> Option<Vec<u8>> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::comment_rm::rm_comments(parser),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            /// Detect the span of every function in the held parse. Backs
            /// [`crate::Ast::functions`].
            pub(crate) fn run_functions(&self) -> Vec<crate::FunctionSpan> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::function::function(parser),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            /// Build the AST dump for the held parse under `cfg`. Backs
            /// [`crate::Ast::dump`].
            pub(crate) fn run_dump(&self, cfg: crate::AstCfg) -> crate::AstResponse {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::ast::dump_inner(parser, cfg),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => {
                        let _ = cfg;
                        match *self {}
                    },
                }
            }

            /// Count `(matching, total)` nodes for `filters` in the held
            /// parse. Backs [`crate::Ast::count`].
            pub(crate) fn run_count(&self, filters: &[String]) -> (usize, usize) {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::count::count(parser, filters),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => {
                        let _ = filters;
                        match *self {}
                    },
                }
            }

            /// Find every node matching `filters` in the held parse. Backs
            /// [`crate::Ast::find`]; the returned nodes borrow the held tree.
            pub(crate) fn run_find(
                &self,
                filters: &[String],
            ) -> Result<Vec<crate::Node<'_>>, MetricsError> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::find::find(parser, filters),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => {
                        let _ = filters;
                        match *self {}
                    },
                }
            }

            /// Collect every in-source suppression marker in the held parse.
            /// Backs [`crate::Ast::suppressions`].
            pub(crate) fn run_suppressions(&self) -> Vec<crate::SuppressionMarker> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => crate::suppression::suppression_markers(parser),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            /// Borrow the root [`crate::Node`] of the held parse. Backs
            /// [`crate::Ast::root_node`].
            pub(crate) fn root_node(&self) -> crate::Node<'_> {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => parser.root(),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
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
                        AstInner::$camel(parser) => parser.code(),
                    )*
                    #[cfg(not(any( $( feature = $feature ),* )))]
                    _ => match *self {},
                }
            }

            pub(crate) fn ts_tree(&self) -> &::tree_sitter::Tree {
                match self {
                    $(
                        #[cfg(feature = $feature)]
                        AstInner::$camel(parser) => parser.ts_tree(),
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
            source: Vec<u8>,
            preproc_path: Option<&Path>,
            preproc: Option<Arc<PreprocResults>>,
        ) -> Result<AstInner, MetricsError> {
            // `Parser::new` keys the C++ macro-expansion lookup off the
            // caller-supplied path; for callers analysing in-memory
            // snippets with no preprocessor path, fall back to an
            // empty `Path` ("") which the lookup ignores. The empty
            // path is *not* leaked into `FuncSpace::name` — that
            // is carried separately on `Ast`. `source` is taken by value
            // so an owned `Source` (`Source::from_bytes`) moves its
            // buffer straight into the parser instead of copying it.
            let preproc_path = preproc_path.unwrap_or(Path::new(""));
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
            /// assert!(LANG::Rust.extensions().contains(&"rs"));
            /// ```
            #[must_use]
            pub fn extensions(&self) -> &'static [&'static str] {
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
            pub(crate) struct $code { _guard: (), }

            impl LanguageInfo for $code {
                type BaseLang = $camel;

                fn lang() -> LANG {
                    LANG::$camel
                }
            }

            #[doc = "The `"]
            #[doc = $docname]
            #[doc = "` language parser."]
            pub(crate) type $parser = Parser<$code>;
        )*
    };
}

macro_rules! mk_langs {
    ( $( ($feature:literal, $camel:ident, $description: expr, $display: expr, $code:ident, $parser:ident, $name:ident, [ $( $ext:ident ),* ], [ $( $emacs_mode:expr ),* ], $version:literal) ),* ) => {
        mk_lang!($( ($feature, $camel, $name, $display, $description, $version) ),*);
        mk_action!($( ($feature, $camel, $parser) ),*);
        mk_extensions!($( ($camel, [ $( $ext ),* ]) ),*);
        mk_emacs_mode!($( ($camel, [ $( $emacs_mode ),* ]) ),*);
        mk_code!($( ($camel, $code, $parser, $name, stringify!($camel)) ),*);
    };
}

mod kind_sets;

pub(crate) use implement_metric_trait;
pub(crate) use kind_sets::{
    cpp_bool_terminal_kinds, csharp_bool_terminal_kinds, csharp_invocation_expr_kinds,
    csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds, csharp_var_decl_kinds,
    csharp_var_declarator_kinds, elixir_bool_terminal_kinds, go_bool_terminal_kinds,
    groovy_bool_terminal_kinds, irules_bool_terminal_kinds, java_bool_terminal_kinds,
    javascript_bool_terminal_kinds, kotlin_bool_terminal_kinds, lua_bool_terminal_kinds,
    mozjs_bool_terminal_kinds, perl_bool_terminal_kinds, php_bool_terminal_kinds,
    python_bool_terminal_kinds, ruby_bool_terminal_kinds, rust_bool_terminal_kinds,
    tcl_bool_terminal_kinds, tsx_bool_terminal_kinds, typescript_bool_terminal_kinds,
};
pub(crate) use {
    get_language, mk_action, mk_code, mk_emacs_mode, mk_extensions, mk_lang, mk_langs,
};
