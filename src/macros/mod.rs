//! The metric-side macros, plus re-exports of the kind-set macros the
//! metric modules share with the classifiers in `big-code-analysis-ast`.

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

pub(crate) use implement_metric_trait;
// Kind-set aliases and the parser dispatch macro are defined in
// `big-code-analysis-ast`; the metric modules keep reaching them as
// `crate::macros::<name>`.
pub(crate) use big_code_analysis_ast::{
    cpp_bool_terminal_kinds, csharp_bool_terminal_kinds, csharp_paren_expr_kinds,
    csharp_prefix_unary_expr_kinds, csharp_var_decl_kinds, csharp_var_declarator_kinds,
    elixir_bool_terminal_kinds, go_bool_terminal_kinds, groovy_bool_terminal_kinds,
    irules_bool_terminal_kinds, java_bool_terminal_kinds, javascript_bool_terminal_kinds,
    kotlin_bool_terminal_kinds, lua_bool_terminal_kinds, mozjs_bool_terminal_kinds,
    perl_bool_terminal_kinds, php_bool_terminal_kinds, python_bool_terminal_kinds,
    ruby_bool_terminal_kinds, rust_bool_terminal_kinds, tcl_bool_terminal_kinds,
    tsx_bool_terminal_kinds, typescript_bool_terminal_kinds, with_any_parser,
};
