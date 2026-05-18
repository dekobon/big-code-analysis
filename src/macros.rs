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
    ( $( ($camel:ident, $name:ident, $display: expr, $description:expr) ),* ) => {
        /// The list of supported languages.
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

            // Returns a tree-sitter language.
            // This function is only used to construct a parser.
            pub(crate) fn get_ts_language(&self) -> Language {
                    match self {
                        $(
                            LANG::$camel => get_language!($name),
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
            /// # Examples
            ///
            /// ```
            /// use big_code_analysis::LANG;
            ///
            /// let _lang = LANG::Rust.get_tree_sitter_language();
            /// ```
            pub fn get_tree_sitter_language(&self) -> ::tree_sitter::Language {
                self.get_ts_language()
            }
        }
    };
}

macro_rules! mk_action {
    ( $( ($camel:ident, $parser:ident) ),* ) => {
        /// Runs a function, which implements the [`Callback`] trait,
        /// on a code written in one of the supported languages.
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
        /// action::<Metrics>(&language, source_as_vec, &cfg.path.clone(), None, cfg);
        /// ```
        ///
        /// [`Callback`]: trait.Callback.html
        #[inline]
        pub fn action<T: Callback>(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>, cfg: T::Cfg) -> T::Res {
            match lang {
                $(
                    LANG::$camel => {
                        let parser = $parser::new(source, path, pr);
                        T::call(cfg, &parser)
                    },
                )*
            }
        }

        /// Returns all function spaces data of a code.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// use big_code_analysis::{get_function_spaces, LANG};
        ///
        /// let source_code = "int a = 42;";
        /// let language = LANG::Cpp;
        ///
        /// // The path to a dummy file used to contain the source code
        /// let path = PathBuf::from("foo.c");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        ///
        /// get_function_spaces(&language, source_as_vec, &path, None).unwrap();
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::EmptyRoot`] when the AST walker
        /// cannot produce a top-level [`FuncSpace`] (typically empty
        /// input or input whose only content is comments).
        #[inline]
        pub fn get_function_spaces(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Result<FuncSpace, MetricsError> {
            match lang {
                $(
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        metrics(&parser, &path)
                    },
                )*
            }
        }

        /// Returns all function spaces data of a code, applying the
        /// per-traversal flags in `options` (e.g.
        /// `exclude_tests: true` to elide Rust `#[cfg(test)]` /
        /// `#[test]` subtrees from every metric). Equivalent to
        /// [`get_function_spaces`] when `options` is the default.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        ///
        /// use big_code_analysis::{get_function_spaces_with_options, LANG, MetricsOptions};
        ///
        /// let source_code = "fn main() {}\n#[test] fn t() {}";
        /// let language = LANG::Rust;
        ///
        /// let path = PathBuf::from("foo.rs");
        /// let source_as_vec = source_code.as_bytes().to_vec();
        /// let options = MetricsOptions::default().with_exclude_tests(true);
        ///
        /// get_function_spaces_with_options(&language, source_as_vec, &path, None, options).unwrap();
        /// ```
        ///
        /// # Errors
        ///
        /// Returns [`MetricsError::EmptyRoot`] when the AST walker
        /// cannot produce a top-level [`FuncSpace`].
        #[inline]
        pub fn get_function_spaces_with_options(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
            match lang {
                $(
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        metrics_with_options(&parser, &path, options)
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
        ///     .set_language(&LANG::Rust.get_tree_sitter_language())
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
        /// Returns [`MetricsError::EmptyRoot`] when the AST walker
        /// cannot produce a top-level [`FuncSpace`].
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
            match lang {
                $(
                    LANG::$camel => {
                        let parser = $parser::from_tree(tree, source);
                        metrics_with_options(&parser, &path, options)
                    },
                )*
            }
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
        /// Returns [`MetricsError::EmptyRoot`] when the AST walker
        /// cannot produce a top-level [`Ops`].
        #[inline]
        pub fn get_ops(lang: &LANG, source: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Result<Ops, MetricsError> {
            match lang {
                $(
                    LANG::$camel => {
                        let parser = $parser::new(source, &path, pr);
                        operands_and_operators(&parser, &path)
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
    ( $( ($camel:ident, $description: expr, $display: expr, $code:ident, $parser:ident, $name:ident, [ $( $ext:ident ),* ], [ $( $emacs_mode:expr ),* ]) ),* ) => {
        mk_lang!($( ($camel, $name, $display, $description) ),*);
        mk_action!($( ($camel, $parser) ),*);
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

pub(crate) use implement_metric_trait;
pub(crate) use {
    csharp_invocation_expr_kinds, csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds,
    csharp_var_decl_kinds, csharp_var_declarator_kinds, get_language, mk_action, mk_code,
    mk_emacs_mode, mk_extensions, mk_lang, mk_langs,
};
