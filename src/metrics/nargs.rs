// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::fmt;

use crate::c_declarator::innermost_declarator;
use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

/// The `NArgs` metric.
///
/// This metric counts the number of arguments
/// of functions/closures.
///
/// A callable is whatever opens a `Function` space and is
/// `is_func_with_code`: a Kotlin or C# primary constructor is one, its
/// space being the parameter list in the class header (#1828), so its
/// parameters are its own and never the class's.
///
/// Unlike the per-function fields of cyclomatic, cognitive and nexits
/// (#1284), the extrema and the averages here read different
/// populations (#1809):
///
/// - `*_min` / `*_max` fold every space's own count on the channel, so a
///   space declaring none folds a 0. The file unit and containers
///   declare none, so their [`function_args_min`](Self::function_args_min)
///   is always 0.
/// - `*_average` divides the channel's subtree total by the functions or
///   closures `nom` counts. A lambda that opens no space (Python, Java,
///   C++, …) adds its parameters to the enclosing function's own
///   `closure_args` yet is one closure to `nom`, so a function holding
///   four one-parameter lambdas reports `closure_args_min` 4 and
///   [`closure_args_average`](Self::closure_args_average) 1.0.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    fn_nargs: usize,
    closure_nargs: usize,
    fn_nargs_sum: usize,
    closure_nargs_sum: usize,
    fn_nargs_min: usize,
    closure_nargs_min: usize,
    fn_nargs_max: usize,
    closure_nargs_max: usize,
    total_functions: usize,
    total_closures: usize,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            fn_nargs: 0,
            closure_nargs: 0,
            fn_nargs_sum: 0,
            closure_nargs_sum: 0,
            fn_nargs_min: usize::MAX,
            closure_nargs_min: usize::MAX,
            fn_nargs_max: 0,
            closure_nargs_max: 0,
            total_functions: 0,
            total_closures: 0,
        }
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "function_args: {}, closure_args: {}, function_args_average: {}, closure_args_average: {}, total: {}, average: {}, function_args_min: {}, function_args_max: {}, closure_args_min: {}, closure_args_max: {}",
            self.function_args_sum(),
            self.closure_args_sum(),
            self.function_args_average(),
            self.closure_args_average(),
            self.total(),
            self.average(),
            self.function_args_min(),
            self.function_args_max(),
            self.closure_args_min(),
            self.closure_args_max()
        )
    }
}

impl Stats {
    /// Merges a second `NArgs` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.closure_nargs_min = self.closure_nargs_min.min(other.closure_nargs_min);
        self.closure_nargs_max = self.closure_nargs_max.max(other.closure_nargs_max);
        self.fn_nargs_min = self.fn_nargs_min.min(other.fn_nargs_min);
        self.fn_nargs_max = self.fn_nargs_max.max(other.fn_nargs_max);
        self.fn_nargs_sum += other.fn_nargs_sum;
        self.closure_nargs_sum += other.closure_nargs_sum;
    }

    /// Returns the number of function arguments in a space.
    #[inline]
    #[must_use]
    pub fn function_args(&self) -> u64 {
        self.fn_nargs as u64
    }

    /// Returns the number of closure arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args(&self) -> u64 {
        self.closure_nargs as u64
    }

    /// Returns the number of function arguments sum in a space.
    #[inline]
    #[must_use]
    pub fn function_args_sum(&self) -> u64 {
        self.fn_nargs_sum as u64
    }

    /// Returns the number of closure arguments sum in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_sum(&self) -> u64 {
        self.closure_nargs_sum as u64
    }

    /// Returns the average number of functions arguments in a space.
    #[inline]
    #[must_use]
    pub fn function_args_average(&self) -> f64 {
        crate::metrics::average(self.fn_nargs_sum as f64, self.total_functions)
    }

    /// Returns the average number of closures arguments in a space.
    ///
    /// Divides by the closures `nom` counts, so it can fall below
    /// [`closure_args_min`](Self::closure_args_min); see [`Stats`].
    #[inline]
    #[must_use]
    pub fn closure_args_average(&self) -> f64 {
        crate::metrics::average(self.closure_nargs_sum as f64, self.total_closures)
    }

    /// Returns the total number of arguments of each function and
    /// closure in a space.
    #[inline]
    #[must_use]
    pub fn total(&self) -> u64 {
        self.function_args_sum() + self.closure_args_sum()
    }

    /// Returns this space's **own** argument count: its function
    /// parameters plus the parameters of any lambda the language folds
    /// into it (a lambda that opens no space of its own).
    ///
    /// This is the quantity `bca check --threshold nargs=N` gates on
    /// since #1196, and the one the wire shape serializes as
    /// `nargs.value` — as opposed to [`total`](Self::total), which sums
    /// the whole subtree and so charges a function for every nested
    /// callable's parameters. Named here rather than spelled out at
    /// each call site so the CLI gate, the serialized field, and any
    /// future front-end read one definition instead of three copies of
    /// the same sum (#1236).
    #[inline]
    #[must_use]
    pub fn own_args(&self) -> u64 {
        self.function_args() + self.closure_args()
    }

    /// Returns the `NArgs` metric average value
    ///
    /// This value is computed dividing the `NArgs` value
    /// for the total number of functions/closures in a space.
    #[inline]
    #[must_use]
    pub fn average(&self) -> f64 {
        crate::metrics::average(
            self.total() as f64,
            self.total_functions + self.total_closures,
        )
    }
    /// Returns the minimum number of function arguments in a space.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `fn_nargs_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    ///
    /// Folds every space in the subtree, so it is 0 on the file unit and
    /// on every container; see [`Stats`].
    #[inline]
    #[must_use]
    pub fn function_args_min(&self) -> u64 {
        if self.fn_nargs_min == usize::MAX {
            0
        } else {
            self.fn_nargs_min as u64
        }
    }
    /// Returns the maximum number of function arguments in a space.
    #[inline]
    #[must_use]
    pub fn function_args_max(&self) -> u64 {
        self.fn_nargs_max as u64
    }
    /// Returns the minimum number of closure arguments in a space.
    ///
    /// Same `usize::MAX` sentinel collapse as `function_args_min`.
    #[inline]
    #[must_use]
    pub fn closure_args_min(&self) -> u64 {
        if self.closure_nargs_min == usize::MAX {
            0
        } else {
            self.closure_nargs_min as u64
        }
    }
    /// Returns the maximum number of closure arguments in a space.
    #[inline]
    #[must_use]
    pub fn closure_args_max(&self) -> u64 {
        self.closure_nargs_max as u64
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.closure_nargs_sum += self.closure_nargs;
        self.fn_nargs_sum += self.fn_nargs;
    }
    /// Folds this space's own counts into both channels' extrema,
    /// whatever its kind. Gating the function channel on
    /// `SpaceKind::Function`, as cyclomatic does, would change file-level
    /// `function_args_min`, now always 0: a definition change #1809 left
    /// open.
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.closure_nargs_min = self.closure_nargs_min.min(self.closure_nargs);
        self.closure_nargs_max = self.closure_nargs_max.max(self.closure_nargs);
        self.fn_nargs_min = self.fn_nargs_min.min(self.fn_nargs);
        self.fn_nargs_max = self.fn_nargs_max.max(self.fn_nargs);
        self.compute_sum();
    }
    pub(crate) fn finalize(&mut self, total_functions: usize, total_closures: usize) {
        self.total_functions = total_functions;
        self.total_closures = total_closures;
    }
}

/// How many of `params`' children are formal parameters.
///
/// The one place that says what a parameter is, and the only caller of
/// [`Checker::is_non_arg`]. No language's `is_non_arg` lists a comment —
/// they carry punctuation plus, in a few cases, a non-parameter the
/// grammar puts in the list anyway (Rust's `self` receivers, Python's
/// PEP 570 `/` marker, PHP's `...`) — so the purely negative filter
/// this replaces counted a comment sitting between two parameters —
/// `int h(int a /* one */, int b)` reported 3 — and counted the comment
/// that stands in for an unnamed parameter, so `void f(int /*unused*/)`
/// reported 2 (#1201). tree-sitter attaches a comment as a direct child
/// of the parameter list, not inside the parameter it documents, which
/// is why no `is_non_arg` list could have caught it.
///
/// Excluding comments here rather than in twenty `is_non_arg` impls is
/// what makes it one rule: the next language added inherits it. Perl,
/// Elixir and Kotlin lambdas reach their parameter list by three routes
/// `compute_args` cannot express, so they call this directly.
#[inline]
fn count_args<T: Checker>(params: &Node, code: &[u8]) -> usize {
    params
        .children()
        .filter(|child| {
            !T::is_non_arg(child) && !T::is_comment(child) && !T::is_empty_param_marker(child, code)
        })
        .count()
}

#[inline]
fn compute_args<T: Checker>(node: &Node, code: &[u8], nargs: &mut usize) {
    if let Some(params) = node.child_by_field_name("parameters") {
        // The field can hold a lone parameter rather than a list, in
        // which case there are no children to walk and `count_args`
        // yields zero — see `Checker::is_bare_param` (#1185).
        if T::is_bare_param(&params) {
            *nargs += 1;
            return;
        }
        *nargs += count_args::<T>(&params, code);
    } else if node.child_by_field_name("parameter").is_some() {
        // JS/TS/TSX/MozJS arrow functions with a bare identifier parameter
        // (`x => …`) use the singular `parameter` field instead of the plural
        // `parameters` field. The grammar guarantees this is exactly one
        // identifier, so count it as one argument.
        *nargs += 1;
    }
}

#[doc(hidden)]
/// Per-language counting of function arguments.
pub(crate) trait NArgs
where
    Self: Checker,
    Self: std::marker::Sized,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// Asks [`Checker::is_func_with_code`], the bytes and the chain in
    /// hand, exactly as [`crate::nom::Nom::compute`] does: a callable
    /// recognisable only from the source text — Elixir's `def` is an
    /// ordinary `Call` (#275) — or from what encloses it must not
    /// silently report 0 here (#1142). Every override below asks the
    /// same predicate; there is no byte-less one to reach for (#1863).
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_args::<Self>(
                &Self::params_owner(node, ancestors),
                code,
                &mut stats.fn_nargs,
            );
            return;
        }

        if Self::is_closure(node, ancestors) {
            compute_args::<Self>(
                &Self::params_owner(node, ancestors),
                code,
                &mut stats.closure_nargs,
            );
        }
    }

    /// The node whose `parameters` field holds this callable's formal
    /// arguments. Defaults to the callable's own node.
    ///
    /// Exists so a language that spells its parameters somewhere other
    /// than on the function node can say *that* and inherit everything
    /// else. Overriding [`Self::compute`] to change this one expression
    /// means re-stating the callable test and
    /// the closure fallback, and a language that copied them does not
    /// pick up a later correction — which is the drift #1142 and #1162
    /// were both filed about.
    ///
    /// It answers for both channels — a closure reaches its parameters
    /// through this too, which is what lets the C family express a C++
    /// lambda and a pointer-returning function as one rule (#1200).
    ///
    /// The two are mutually exclusive: only the default `compute` calls
    /// this, so a language that overrides `compute` (Objc, Go, Kotlin,
    /// Lua, Tcl, iRules, Perl, Elixir, Groovy, C#) would define a
    /// `params_owner` that is never consulted. Override one or the
    /// other, not both.
    fn params_owner<'tree>(node: &Node<'tree>, _ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        *node
    }
}

// The C family spells a function's parameters on the
// `function_declarator` buried under whatever the *return type*
// contributed, so all three point `params_owner` at the innermost
// declarator carrying a `parameters` field. Falling back to the node
// keeps the pre-#1200 answer for a shape with no declarator at all — a
// parameterless C++ lambda, `[]{ … }`, which has none.
//
// C++ and Mozcpp reach this for their lambdas as well as their
// functions; C has no closure form at all (`CCode::is_closure` is a
// constant `false`), so its closure channel is unreachable rather than
// merely unused.
impl NArgs for CppCode {
    fn params_owner<'tree>(node: &Node<'tree>, _ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        innermost_declarator::<Self>(node).unwrap_or(*node)
    }
}

impl NArgs for CCode {
    fn params_owner<'tree>(node: &Node<'tree>, _ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        innermost_declarator::<Self>(node).unwrap_or(*node)
    }
}

impl NArgs for MozcppCode {
    fn params_owner<'tree>(node: &Node<'tree>, _ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        innermost_declarator::<Self>(node).unwrap_or(*node)
    }
}

// Objective-C carries parameters in three different shapes, so it cannot
// share the single-`declarator`-field C/C++ impl:
//   * free `function_definition`s use the C declarator → `parameters`
//     field, counted exactly as in C;
//   * a `method_definition` lists one `method_parameter` per labelled
//     argument (`- (void)foo:(int)a bar:(int)b` → 2; the unary
//     `- (void)foo` → 0);
//   * a block `^(int x){ … }` holds its params in a `parameter_list`
//     child rather than under a `parameters` field.
impl NArgs for ObjcCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        match node.kind_id().into() {
            Objc::FunctionDefinition | Objc::FunctionDefinition2 => {
                // The same declarator walk C and C++ use: Objective-C
                // inherits C's declarator syntax, so `FILE *f(int a)`
                // buries the parameter list one level down (#1200).
                if let Some(owner) = innermost_declarator::<Self>(node) {
                    compute_args::<Self>(&owner, code, &mut stats.fn_nargs);
                }
            }
            Objc::MethodDefinition => {
                // Both `method_parameter` aliases are accepted per the
                // #285 / lesson-2 defensive convention: real parse trees
                // emit `MethodParameter` (475), but the grammar also
                // declares the `MethodParameter2` (476) alias, so list it
                // too rather than risk a future bump emitting it.
                node.act_on_child(&mut |n| {
                    if matches!(
                        n.kind_id().into(),
                        Objc::MethodParameter | Objc::MethodParameter2
                    ) {
                        stats.fn_nargs += 1;
                    }
                });
            }
            Objc::BlockLiteral => {
                // Through `count_args`, so the block channel gets the same
                // three exclusions `compute_args` gives the function
                // channel. Counting `ParameterDeclaration |
                // VariadicParameter` positively could not consult
                // `Checker::is_empty_param_marker`, so `^(void){ … }` —
                // whose `parameter_list` holds a real
                // `parameter_declaration` for the `void`, exactly as
                // `int f(void)` does — reported one parameter (#1218).
                //
                // It inherits the shared rule's *inclusions* too, which the
                // narrower positive match had excluded by construction: on
                // invalid source an `ERROR` child (`^(int a,,)`) or a
                // `compound_statement` one (`^({ int x; })`) now counts.
                // That is the point rather than a regression — those are
                // the numbers `int f(int a,,)` already reported through
                // `count_args`, so the block arm stopped being the one
                // caller that answered differently.
                //
                // `ParameterList2` is deliberately not matched: it is the
                // alias for the hidden `_old_style_parameter_list`, and
                // `block_literal` cannot produce it — even a K&R function
                // definition emits `ParameterList`. Marked rather than
                // silently omitted per `grammar-dispatch.md` §1/§2.
                if let Some(params) = node.first_child(|id| Objc::ParameterList == id) {
                    stats.closure_nargs += count_args::<Self>(&params, code);
                }
            }
            _ => {}
        }
    }
}

// Go's `parameter_declaration` allows multiple names to share one type
// (`func f(a, b int)` is one parameter_declaration with two `name` children
// but two formal parameters). Count names rather than declarations so the
// reported nargs matches Go's parameter count.
fn compute_go_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("parameters") else {
        return;
    };
    *nargs += params
        .children()
        .map(|child| match child.kind_id().into() {
            Go::ParameterDeclaration => child
                .children()
                .filter(|c| c.kind_id() == Go::Identifier)
                .count()
                .max(1),
            Go::VariadicParameterDeclaration => 1,
            _ => 0,
        })
        .sum::<usize>();
}

impl NArgs for GoCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_go_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node, ancestors) {
            compute_go_args(node, &mut stats.closure_nargs);
        }
    }
}

// A function or a secondary constructor lists `parameter`s in a
// `function_value_parameters`; a primary constructor (#1828) lists
// `class_parameter`s — each `[val|var] name: Type [= default]` — in a
// `class_parameters`. One wrapper per parameter in both, so a positive
// count needs no comment or punctuation exclusion.
fn compute_kotlin_func_args(node: &Node, nargs: &mut usize) {
    let (list, param) = if node.kind_id() == Kotlin::PrimaryConstructor {
        (Kotlin::ClassParameters, Kotlin::ClassParameter)
    } else {
        (Kotlin::FunctionValueParameters, Kotlin::Parameter)
    };
    if let Some(params) = node.children().find(|c| c.kind_id() == list) {
        *nargs += params.children().filter(|n| n.kind_id() == param).count();
    }
}

fn compute_kotlin_lambda_args(node: &Node, code: &[u8], nargs: &mut usize) {
    // Lambda parameters are plain identifiers or destructuring patterns separated
    // by commas; there is no typed `Parameter` wrapper node (unlike function
    // value parameters), so a negative filter is the correct predicate here — the
    // shared one, which also drops the comment `{ a, /* one */ b -> }` puts
    // between them (#1201). `KotlinCode::is_non_arg` adds the parens to the
    // comma, which costs nothing: a destructuring pattern nests its own parens
    // inside a `multi_variable_declaration`, so `lambda_parameters` never has a
    // paren as a direct child.
    if let Some(params) = node
        .children()
        .find(|c| c.kind_id() == Kotlin::LambdaParameters)
    {
        *nargs += count_args::<KotlinCode>(&params, code);
    }
}

impl NArgs for KotlinCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_kotlin_func_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node, ancestors) {
            if node.kind_id() == Kotlin::LambdaLiteral {
                compute_kotlin_lambda_args(node, code, &mut stats.closure_nargs);
            } else {
                compute_kotlin_func_args(node, &mut stats.closure_nargs);
            }
        }
    }
}

fn compute_lua_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("parameters") else {
        return;
    };
    *nargs += params
        .children()
        .filter(|c| matches!(c.kind_id().into(), Lua::Identifier | Lua::VarargExpression))
        .count();
}

impl NArgs for LuaCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_lua_args(node, &mut stats.fn_nargs);
        } else if Self::is_closure(node, ancestors) {
            compute_lua_args(node, &mut stats.closure_nargs);
        }
    }
}

fn compute_tcl_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("arguments") else {
        return;
    };
    *nargs += params
        .children()
        .filter(|c| c.kind_id() == Tcl::Argument)
        .count();
}

impl NArgs for TclCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_tcl_args(node, &mut stats.fn_nargs);
        }
    }
}

// iRules counterpart of `compute_tcl_args`. Only `procedure` carries an
// `arguments` *field*; `when_event` handlers have no formal parameters
// (the event context is implicit), so they correctly count zero. `{a 5}`
// default-valued parameters parse as a single `argument`, so each formal
// parameter contributes one regardless of its default.
fn compute_irules_args(node: &Node, nargs: &mut usize) {
    let Some(params) = node.child_by_field_name("arguments") else {
        return;
    };
    *nargs += params
        .children()
        .filter(|c| c.kind_id() == Irules::Argument)
        .count();
}

impl NArgs for IrulesCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_irules_args(node, &mut stats.fn_nargs);
        }
    }
}

// tree-sitter-perl emits a subroutine signature as an unnamed
// `function_signature` child rather than under a `parameters` field, so
// the shared `compute_args` helper never sees it. `FunctionSignature2`
// is the hidden `_function_signature` supertype, listed defensively per
// the lesson-2 convention.
//
// A bare attribute swallows the signature — `sub f :lvalue ($z)` parses
// as `function_attribute → function_signature` — while an attribute
// carrying its own parens (`sub f :prototype($$) ($a, $b)`) leaves the
// signature a direct child. Look one level into `function_attribute` so
// both spellings count; a `:prototype($$)` argument list is a
// `function_prototype`, a different kind, so it cannot be mistaken for a
// signature.
fn perl_signature<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    fn is_signature(id: u16) -> bool {
        matches!(
            id.into(),
            Perl::FunctionSignature | Perl::FunctionSignature2
        )
    }
    node.children().find_map(|child| {
        if is_signature(child.kind_id()) {
            Some(child)
        } else if child.kind_id() == Perl::FunctionAttribute {
            child.first_child(is_signature)
        } else {
            None
        }
    })
}

// Count every signature child `count_args` accepts: a defaulted parameter
// (`$y = 5`) is a `binary_expression`, not a bare `scalar_variable`, so a
// positive variant list would undercount it. The negative filter also
// survives signature forms the grammar may add — at the price of needing
// the comment exclusion, since a multi-line signature documents its
// parameters with `comments` children sitting directly under
// `function_signature`. Perl carried that exclusion inline for years
// before #1201 found the same hole in every other language and moved it
// into the shared predicate.
fn compute_perl_args(node: &Node, code: &[u8], nargs: &mut usize) {
    let Some(signature) = perl_signature(node) else {
        return;
    };
    *nargs += count_args::<PerlCode>(&signature, code);
}

impl NArgs for PerlCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            compute_perl_args(node, code, &mut stats.fn_nargs);
            return;
        }

        // Every anonymous sub reads 0 today: tree-sitter-perl 1.1.2 parses
        // its signature inside an `ERROR` node
        // (`anonymous_function → sub → ERROR → function_signature`), so
        // `perl_signature` finds nothing among the direct children.
        // Deliberately not recovered — descending through `ERROR` would
        // pin us to a parse shape upstream will change. The call stays
        // here so a grammar fix starts counting (and fails
        // `perl_anonymous_sub_signature_is_zero`) rather than passing
        // silently; revisit at the next `recreate-grammars.sh` bump.
        if Self::is_closure(node, ancestors) {
            compute_perl_args(node, code, &mut stats.closure_nargs);
        }
    }
}

// Elixir has no function-definition node. `def bar(a, b, c)` is a `Call`
// whose `arguments` holds a *second* `Call` carrying the real parameter
// list, which is why the `parameters`-field heuristic in `compute_args`
// finds nothing:
//
//   call (def)
//   ├─ identifier  def            <- the `target` field
//   ╰─ arguments                  <- NOT a field; tree-sitter-elixir
//      ╰─ call     bar(a, b, c)      gives `call` only a `target` field,
//         ├─ identifier bar          so both `arguments` levels have to
//         ╰─ arguments (a, b, c)     be found by kind.
//
// A guarded head interposes a `when` `binary_operator` whose `left` is
// that `Call` — without unwrapping it every guarded clause counts 0, and
// guards are a large fraction of real Elixir. A head that is a bare
// `identifier` (`def noargs, do: 1`) has no parameter list and counts 0.
//
// `def a + b` and `def -a` define the operator functions `+/2` and `-/1`.
// Their head is the operator node itself, with the parameters as its
// operands and no `arguments` container to walk, so the arity comes from
// the operator's shape.
fn elixir_declared_args(node: &Node, code: &[u8]) -> usize {
    let Some(head) = elixir_arguments(node).and_then(|a| a.children().find(Node::is_named)) else {
        return 0;
    };
    let head = elixir_unwrap_guard(&head, code);
    match head.kind_id().into() {
        Elixir::Call => elixir_arguments(&head).map_or(0, |p| count_args::<ElixirCode>(&p, code)),
        Elixir::BinaryOperator => 2,
        Elixir::UnaryOperator => 1,
        _ => 0,
    }
}

fn elixir_arguments<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.first_child(|id| id == Elixir::Arguments)
}

// Returns the guarded expression when `node` is a `when` guard, and
// `node` itself otherwise. Matching `BinaryOperator` alone would also
// unwrap an operator definition, whose operands are the parameters.
fn elixir_unwrap_guard<'a>(node: &Node<'a>, code: &[u8]) -> Node<'a> {
    let is_when = node.kind_id() == Elixir::BinaryOperator
        && node
            .child_by_field_name("operator")
            .and_then(|op| op.utf8_text(code))
            == Some("when");
    if is_when {
        node.child_by_field_name("left").unwrap_or(*node)
    } else {
        *node
    }
}

impl NArgs for ElixirCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        // A `def` is a `Call` told from any other by its target text
        // (#275), and one inside `quote do … end` declares nothing until
        // the macro expands (#310): the predicate reads both.
        if Self::is_func_with_code(node, code, ancestors) {
            stats.fn_nargs += elixir_declared_args(node, code);
            return;
        }

        // Every clause of one `fn` must have the same arity, so the first
        // `stab_clause` gives the closure's argument count. Summing the
        // clauses would report `2n` for an n-clause function — do not
        // "fix" this into a sum.
        //
        // A guarded clause (`fn x when is_integer(x) -> …`) aliases its
        // `left` to a `binary_operator`, exactly as a guarded `def` head
        // does, so it needs the same unwrap — otherwise every guarded
        // closure counts the fixed 3 children of the guard expression.
        if Self::is_closure(node, ancestors)
            && let Some(clause) = node.first_child(|id| id == Elixir::StabClause)
            && let Some(params) = clause.child_by_field_name("left")
        {
            stats.closure_nargs +=
                count_args::<ElixirCode>(&elixir_unwrap_guard(&params, code), code);
        }
    }
}

implement_metric_trait!(
    [NArgs],
    PythonCode,
    MozjsCode,
    JavascriptCode,
    TypescriptCode,
    TsxCode,
    RustCode,
    PreprocCode,
    CcommentCode,
    BashCode,
    PhpCode,
    RubyCode
);

// tree-sitter-c-sharp wraps an ordinary parameter in a `parameter`
// node but spells a `params` array through the hidden
// `_parameter_array` rule, whose pieces — the attribute lists, the
// `params` token, the type and the name — land directly in the list.
// The shared negative filter counted every one of them, so
// `void F(params int[] xs)` reported 3 and `void F([A] params int[] xs)`
// 4. Count positively instead: one per `parameter`, one per `params`
// token, which is the one child the hidden rule always emits. The same
// shape sits in an indexer's `bracketed_parameter_list`, which the
// `parameters` field also reaches.
//
// A comment in the list is neither, so the #1201 exclusion holds
// without naming it; a bare lambda parameter (`x => x`) is the
// childless `implicit_parameter` the field holds instead of a list
// (#1185), still answered by `is_bare_param`.
fn csharp_count_params(params: &Node) -> usize {
    params
        .children()
        .filter(|c| matches!(c.kind_id().into(), Csharp::Parameter | Csharp::Params))
        .count()
}

// A primary constructor *is* its `parameter_list` — the `FuncOpener`
// row of `roles/csharp.rs` opens the space on that node (#1828) — so
// the list is the node itself; every other callable holds its list in
// the `parameters` field.
fn csharp_compute_args(node: &Node, nargs: &mut usize) {
    if node.kind_id() == Csharp::ParameterList {
        *nargs += csharp_count_params(node);
        return;
    }
    if let Some(params) = node.child_by_field_name("parameters") {
        if CsharpCode::is_bare_param(&params) {
            *nargs += 1;
            return;
        }
        *nargs += csharp_count_params(&params);
    }
}

impl NArgs for CsharpCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func_with_code(node, code, ancestors) {
            csharp_compute_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node, ancestors) {
            csharp_compute_args(node, &mut stats.closure_nargs);
        }
    }
}

// A record's components are the `parameters` field of its
// `record_declaration`, and two of the record's callables own them from
// there rather than from a list of their own.
//
// A compact constructor (`record R(int a, int b) { R { … } }`, JLS
// 8.10.4) declares no formal parameter list: the grammar gives
// `compact_constructor_declaration` only `name` and `body` fields, so
// its owner is the record, which sits exactly two steps up — the
// constructor is a direct child of the record's `class_body`. Resolving
// it is what makes the two spellings of one constructor agree: the
// canonical `R(int a, int b) { … }` reports 2, and so does the compact
// form (#1160). The kind check states what that positional step is
// allowed to land on; no other kind carries a `parameters` field here,
// so it changes no count today, but it keeps the step from silently
// starting to mean something else if one ever does.
//
// A record that declares *no* canonical constructor reports the
// components on its header instead: the `FuncOpener` row of
// `roles/java.rs` opens a `Function` space on the `formal_parameters`
// itself where the body writes neither a compact nor a normal canonical
// constructor (#1842, revisiting #1160's "declared code rather than
// generated code": the component list *is* declared, as a C# positional
// record's is — #1828), so the list's owner is the record whose
// `record_header` it is (`roles/java.rs`). The row's gate keeps the
// count to one owner: with a compact constructor the header opens
// nothing and the redirect counts the components there; with neither
// the header is the callable. A non-canonical constructor is a callable
// of its own, owns its own list, and suppresses nothing.
impl NArgs for JavaCode {
    fn params_owner<'tree>(node: &Node<'tree>, ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        let record = match node.kind_id().into() {
            Java::FormalParameters => ancestors
                .parent(node)
                .filter(|record| crate::roles::java::is_record_header(node, record)),
            Java::CompactConstructorDeclaration => ancestors
                .iter(node)
                .nth(1)
                .map(|(grandparent, _)| grandparent)
                .filter(|grandparent| grandparent.kind_id() == Java::RecordDeclaration),
            _ => None,
        };
        record.unwrap_or(*node)
    }
}

// Groovy closures use `closure_parameters` as an unnamed child rather
// than a `parameters` field, so the default NArgs walker (which looks
// for a `parameters` field) misses them. Match the closure_parameters
// child directly and count its `closure_parameter` grand-children.
//
// A record's header is its `record_components` — the `FuncOpener` row
// of `roles/groovy.rs` opens the space on that node where the body
// writes no canonical constructor (#1842) — so the list is the node
// itself and its `record_component` children are the arguments, one
// wrapper each (a variable-arity component's stray `...` lands in an
// `ERROR` beside its type, inside the one wrapper). They are counted by
// kind and not through `count_args`: the two agree on a clean parse,
// but `count_args` takes every child that is no parenthesis, comma or
// comment, so the `ERROR` recovery leaves beside a component
// (`record R(int a,)`) would read as a second argument (#1863). Every
// other callable holds its list in the `parameters` field.
impl NArgs for GroovyCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        use crate::languages::language_groovy::Groovy;

        if Self::is_func_with_code(node, code, ancestors) {
            if node.kind_id() == Groovy::RecordComponents {
                stats.fn_nargs += node
                    .children()
                    .filter(|n| n.kind_id() == Groovy::RecordComponent)
                    .count();
                return;
            }
            compute_args::<Self>(node, code, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node, ancestors)
            && let Some(params) = node.first_child(|id| id == Groovy::ClosureParameters)
        {
            params.act_on_child(&mut |n| {
                if n.kind_id() == Groovy::ClosureParameter {
                    stats.closure_nargs += 1;
                }
            });
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use crate::test_support::{
        check_func_space, check_func_space_only_shim, check_metrics_only_shim, child_space,
    };

    use super::*;

    // Nargs pulls Nom for its per-function average divisor, which is also
    // what this module's `metric.nom.functions_sum()` /
    // `closures_sum()` assertions read.
    check_metrics_only_shim!(check_metrics, Nargs);
    // The same selection, handed the whole space tree: the per-space own
    // count is only observable one space at a time.
    check_func_space_only_shim!(check_spaces, Nargs);

    /// Regression for #227: a `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `fn_args_min` or `closure_args_min`. Both getters collapse
    /// the sentinel to `0.0` so JSON never emits `1.8446744e19`.
    #[test]
    fn nargs_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.function_args_min(), 0);
        assert_eq!(stats.closure_args_min(), 0);
    }

    /// The per-space **own** argument count, at every space of a tree
    /// where it differs from the subtree `total` at every level.
    ///
    /// `bca check --threshold nargs=N` has gated on `own_args()` since
    /// #1196, and #1236 serialized it as `nargs.value` so the JSON-walking
    /// `to_sarif` binding can reproduce that gate. Before #1236 the
    /// binding compared `nargs.total` and reported `outer` at 7 — a
    /// finding `bca check` never emits, because `outer` declares two
    /// parameters and the other five belong to closures that are gated
    /// on rows of their own.
    ///
    /// The fixture is the #1236 reproducer, picked because no space in it
    /// has `own == total`: a fixture where the two agree passes whichever
    /// one the code reads.
    /// The `Stats` doc's and the book's `#nargs-aggregates` example, held
    /// here so the sentence "`closure_args_average` can fall below
    /// `closure_args_min`" stops being true the day #1809 changes the
    /// definition, and the three docs that state it are known to need
    /// an edit. Four one-parameter lambdas open no space, so `pick` owns
    /// all four parameters (min = max = 4) while `nom` counts four
    /// closures (average 1.0).
    #[cfg(feature = "python")]
    #[test]
    fn closure_args_average_falls_below_min_when_lambdas_open_no_space() {
        check_spaces::<PythonParser, _>(
            "def pick(rows):
    a = sorted(rows, key=lambda r: r[0])
    b = filter(lambda r: r[1], a)
    c = map(lambda r: r[2], b)
    return min(c, key=lambda r: r)
",
            "pick.py",
            |space| {
                let pick = space.spaces.first().expect("pick is the first space");
                assert_eq!(pick.name.as_deref(), Some("pick"));
                let nargs = &pick.metrics.nargs;
                assert_eq!(nargs.closure_args_min(), 4);
                assert_eq!(nargs.closure_args_max(), 4);
                assert_eq!(nargs.closure_args_sum(), 4);
                assert!((nargs.closure_args_average() - 1.0).abs() < 1e-9);
                assert!(nargs.closure_args_average() < nargs.closure_args_min() as f64);
                // The file unit declares no parameters, so its minimum is 0.
                assert_eq!(space.metrics.nargs.function_args_min(), 0);
            },
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn own_args_excludes_nested_closure_spaces() {
        fn walk(space: &FuncSpace, rows: &mut Vec<(String, usize, u64, u64)>) {
            rows.push((
                space.name.clone().unwrap_or_default(),
                space.start_line,
                space.metrics.nargs.own_args(),
                space.metrics.nargs.total(),
            ));
            for child in &space.spaces {
                walk(child, rows);
            }
        }

        check_spaces::<RustParser, _>(
            "fn outer(a: i32, b: i32) {
                 let f = |x: i32, y: i32, z: i32| x + y + z;
                 let g = |p: i32, q: i32| p + q;
                 f(a, b, 0);
                 g(a, b);
             }",
            "closures.rs",
            |space| {
                let mut rows = Vec::new();
                walk(&space, &mut rows);
                // Hand-pinned in walk order, `(name, start_line, own, total)`.
                // The file root owns no parameters; `outer` owns its two;
                // each closure owns its own list. Every row's `total` is
                // that space's own count plus its descendants'.
                assert_eq!(
                    rows,
                    vec![
                        ("closures.rs".to_owned(), 1, 0, 7),
                        ("outer".to_owned(), 1, 2, 7),
                        ("<anonymous>".to_owned(), 2, 3, 3),
                        ("<anonymous>".to_owned(), 3, 2, 2),
                    ],
                );

                // The serialized projection the Python `to_sarif` binding
                // walks: `value` is the gate's number, `total` is not.
                let outer = &space.spaces[0];
                let json = serde_json::to_value(&outer.metrics.nargs)
                    .expect("nargs serializes through its wire projection");
                assert_eq!(json["value"], 2, "nargs.value must be `outer`'s own args");
                assert_eq!(json["total"], 7, "nargs.total stays the subtree sum");
            },
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_no_functions_and_closures() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 0,
              "function_args_average": 0.0,
              "closure_args_average": 0.0,
              "total": 0,
              "value": 0,
              "average": 0.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 0
            }
            "#
            );
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_no_functions_and_closures() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 0,
              "function_args_average": 0.0,
              "closure_args_average": 0.0,
              "total": 0,
              "value": 0,
              "average": 0.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 0
            }
            "#
            );
        });
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_no_functions_and_closures() {
        check_metrics::<CppParser>("int a = 42;", "foo.cpp", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 0,
              "function_args_average": 0.0,
              "closure_args_average": 0.0,
              "total": 0,
              "value": 0,
              "average": 0.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 0
            }
            "#
            );
        });
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_no_functions_and_closures() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            // 0 functions + 0 closures
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 0,
              "function_args_average": 0.0,
              "closure_args_average": 0.0,
              "total": 0,
              "value": 0,
              "average": 0.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 0
            }
            "#
            );
        });
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_single_function() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a",
            "foo.py",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_single_function() {
        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "c")]
    #[test]
    fn c_single_function() {
        check_metrics::<CParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_single_function() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 1 function
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_single_lambda() {
        check_metrics::<PythonParser>("bar = lambda a: True", "foo.py", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 1,
              "function_args_average": 0.0,
              "closure_args_average": 1.0,
              "total": 1,
              "value": 1,
              "average": 1.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 1,
              "closure_args_max": 1
            }
            "#
            );
        });
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_single_closure() {
        check_metrics::<RustParser>("let bar = |i: i32| -> i32 { i + 1 };", "foo.rs", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 1,
              "function_args_average": 0.0,
              "closure_args_average": 1.0,
              "total": 1,
              "value": 0,
              "average": 1.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 1
            }
            "#
            );
        });
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_single_lambda() {
        check_metrics::<CppParser>(
            "auto bar = [](int x, int y) -> int { return x + y; };",
            "foo.cpp",
            |metric| {
                // 1 lambda
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 2,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 2,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_single_closure() {
        check_metrics::<JavascriptParser>("function (a, b) {return a + b};", "foo.js", |metric| {
            // 1 lambda
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 2,
              "function_args_average": 0.0,
              "closure_args_average": 2.0,
              "total": 2,
              "value": 0,
              "average": 2.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 2
            }
            "#
            );
        });
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_functions() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a
            def f(a, b):
                 if b:
                     return b",
            "foo.py",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );

        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a:
                     return a
            def f(a, b, c):
                 if b:
                     return b",
            "foo.py",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 5,
                  "closure_args": 0,
                  "function_args_average": 2.5,
                  "closure_args_average": 0.0,
                  "total": 5,
                  "value": 0,
                  "average": 2.5,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_functions() {
        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }
             fn f1(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f(a: bool, b: usize) {
                 if a {
                     return a;
                }
             }
             fn f1(a: bool, b: usize, c: usize) {
                 if a {
                     return a;
                }
             }",
            "foo.rs",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 5,
                  "closure_args": 0,
                  "function_args_average": 2.5,
                  "closure_args_average": 0.0,
                  "total": 5,
                  "value": 0,
                  "average": 2.5,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// The `self` receiver (`self`, `&self`, `&mut self`) parses as a
    /// `self_parameter` node and, like Go's `receiver` field and C++'s
    /// implicit `this`, must not be counted as a formal parameter (#457).
    #[cfg(feature = "rust")]
    #[test]
    fn rust_method_excludes_self_receiver() {
        check_metrics::<RustParser>(
            "struct S;
             impl S {
                 fn a(self) {}                  // self          -> 0 args
                 fn b(&self, x: i32) {}         // &self     + 1 -> 1 arg
                 fn c(&mut self, x: i32, y: i32) {} // &mut self + 2 -> 2 args
             }",
            "foo.rs",
            |metric| {
                // 3 methods: 0 + 1 + 2 explicit params. The three receiver
                // forms contribute nothing. sum = 3, max = 2.
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 2);
            },
        );

        // A *typed* receiver (`self: Box<Self>`, `self: Rc<Self>`,
        // `self: Pin<&mut Self>`) parses as an ordinary `parameter` node —
        // not `self_parameter` — but its binding is the `self` keyword, so
        // it is still a receiver and must be excluded too, matching the
        // bare-receiver case and Go/C++ receiver parity (#457). A normal
        // `parameter` like `x: i32` binds an `identifier`, never `self`.
        check_metrics::<RustParser>(
            "use std::rc::Rc;
             use std::pin::Pin;
             struct S;
             impl S {
                 fn a(self: Box<Self>, x: i32, y: i32) {} // receiver + 2 -> 2
                 fn b(self: Rc<Self>, x: i32) {}          // receiver + 1 -> 1
                 fn c(self: Pin<&mut Self>) {}            // receiver     -> 0
             }",
            "foo.rs",
            |metric| {
                // Each typed receiver contributes nothing. sum = 2+1+0 = 3,
                // max = 2 (from method `a`).
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 2);
            },
        );
    }

    #[cfg(all(feature = "c", feature = "cpp"))]
    #[test]
    fn c_functions() {
        check_metrics::<CParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }
             int f1(int a, int b) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );

        check_metrics::<CppParser>(
            "int f(int a, int b) {
                 if (a) {
                     return a;
                }
             }
             int f1(int a, int b, int c) {
                 if (a) {
                     return a;
                }
             }",
            "foo.c",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 5,
                  "closure_args": 0,
                  "function_args_average": 2.5,
                  "closure_args_average": 0.0,
                  "total": 5,
                  "value": 0,
                  "average": 2.5,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_functions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }
             function f1(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );

        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 return a * b;
             }
             function f1(a, b, c) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                // 2 functions
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 5,
                  "closure_args": 0,
                  "function_args_average": 2.5,
                  "closure_args_average": 0.0,
                  "total": 5,
                  "value": 0,
                  "average": 2.5,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_nested_functions() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 def foo(a):
                     if a:
                         return 1
                 bar = lambda a: lambda b: b or True or True
                 return bar(foo(a))(a)",
            "foo.py",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 2,
                  "function_args_average": 1.5,
                  "closure_args_average": 1.0,
                  "total": 5,
                  "value": 0,
                  "average": 1.25,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_nested_functions() {
        check_metrics::<RustParser>(
            "fn f(a: i32, b: i32) -> i32 {
                 fn foo(a: i32) -> i32 {
                     return a;
                 }
                 let bar = |a: i32, b: i32| -> i32 { a + 1 };
                 let bar1 = |b: i32| -> i32 { b + 1 };
                 return bar(foo(a), a);
             }",
            "foo.rs",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 3,
                  "function_args_average": 1.5,
                  "closure_args_average": 1.5,
                  "total": 6,
                  "value": 0,
                  "average": 1.5,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_nested_functions() {
        check_metrics::<CppParser>(
            "int f(int a, int b, int c) {
                 auto foo = [](int x) -> int { return x; };
                 auto bar = [](int x, int y) -> int { return x + y; };
                 return bar(foo(a), a);
             }",
            "foo.cpp",
            |metric| {
                // 1 functions + 2 lambdas = 3
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 3,
                  "function_args_average": 3.0,
                  "closure_args_average": 1.5,
                  "total": 6,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 3
                }
                "#
                );
            },
        );
    }

    /// Default arguments still surface as separate `parameter_declaration`
    /// nodes — defaults are not removed from the count.  A 3-param function
    /// whose third parameter has a default value reports `nargs = 3`.
    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_default_arguments() {
        check_metrics::<CppParser>(
            "int f(int a, int b, int c = 0) {
                 return a + b + c;
             }",
            "foo.cpp",
            |metric| {
                // 1 function, 3 parameters (defaults still count).
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 3);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// C-style variadic `...` parameter contributes +1 (one named declarator
    /// plus the `...` declarator).  The grammar emits the variadic ellipsis
    /// as a sibling parameter node that `count_args` counts, because it is
    /// neither a comment nor one of the `(`, `)`, `,` tokens `CCode::is_non_arg`
    /// rejects.
    #[cfg(feature = "c")]
    #[test]
    fn c_variadic_function() {
        check_metrics::<CParser>(
            "int printf(const char* fmt, ...) {
                 return 0;
             }",
            "foo.c",
            |metric| {
                // 1 function, 2 nargs: `fmt` and `...`
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 2);
                assert_eq!(s.function_args_max(), 2);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// C++ template parameter packs (`Args... args`) count as one runtime
    /// parameter (the parameter pack itself), not as N — the template
    /// arguments are compile-time and live on the template-parameter list,
    /// not on `parameters`.  The tree-sitter-cpp grammar represents
    /// `Args... args` as a single `variadic_parameter_declaration` under
    /// `parameters`.
    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_template_parameter_pack() {
        check_metrics::<CppParser>(
            "template<typename... Args>
             int sum(int seed, Args... args) {
                 return seed;
             }",
            "foo.cpp",
            |metric| {
                // 1 function, 2 nargs: `seed` and `Args... args`
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 2);
                assert_eq!(s.function_args_max(), 2);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// Lambda capture list (`[=, &x]`) is not part of the parameter list.
    /// `compute_args` reads the `declarator` field, which only contains the
    /// `( … )` parameter list.  Variables captured for the closure body do
    /// not inflate `nargs`.
    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_lambda_capture_not_counted() {
        check_metrics::<CppParser>(
            "int f() {
                 int x = 1;
                 int y = 2;
                 auto g = [=, &x](int a, int b) -> int { return a + b + x + y; };
                 return g(1, 2);
             }",
            "foo.cpp",
            |metric| {
                // 1 function (0 args), 1 lambda (2 args: a, b — captures `=, &x` excluded).
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 0);
                assert_eq!(s.closure_args_sum(), 2);
                assert_eq!(s.closure_args_max(), 2);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    /// Implicit `this` on a member function is not part of the AST
    /// parameter list — it is an implicit argument at the language level
    /// only.  A non-static member function `void M(int a)` reports
    /// `nargs = 1`, not 2.
    #[cfg(feature = "cpp")]
    #[test]
    fn cpp_member_function_this_not_counted() {
        check_metrics::<CppParser>(
            "struct S {
                 int x;
                 int set(int a) {     // implicit `this` is NOT counted
                     this->x = a;
                     return a;
                 }
             };",
            "foo.cpp",
            |metric| {
                // 1 member function with 1 explicit parameter `a`.
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 1);
                assert_eq!(s.function_args_max(), 1);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 1,
                  "closure_args": 0,
                  "function_args_average": 1.0,
                  "closure_args_average": 0.0,
                  "total": 1,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 1,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_zero_args() {
        check_metrics::<GoParser>(
            "package main
            func f() {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_multiple_args() {
        check_metrics::<GoParser>(
            "package main
            func f(a int, b string, c bool) {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_method_excludes_receiver() {
        check_metrics::<GoParser>(
            "package main
            type T struct{}
            func (t *T) Greet(name string) string {
                return name
            }",
            "foo.go",
            |metric| {
                // Receiver is in a separate `receiver` field and is not counted.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 1,
                  "closure_args": 0,
                  "function_args_average": 1.0,
                  "closure_args_average": 0.0,
                  "total": 1,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 1,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_variadic() {
        check_metrics::<GoParser>(
            "package main
            func f(args ...int) {}",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 1,
                  "closure_args": 0,
                  "function_args_average": 1.0,
                  "closure_args_average": 0.0,
                  "total": 1,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 1,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_grouped_params() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b int, c string) {}",
            "foo.go",
            |metric| {
                // `a, b int` is one parameter_declaration with two `name`
                // children — semantically two parameters.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "go")]
    #[test]
    fn go_func_literal_args() {
        check_metrics::<GoParser>(
            "package main
            var f = func(x, y int) int { return x + y }",
            "foo.go",
            |metric| {
                // Closure with grouped params: `x, y int` -> 2 closure args.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_nested_functions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {
                 function foo(a, c) {
                     return a;
                 }
                 var bar = function (a, b) {return a + b};
                 function (a) {return a};
                 return bar(foo(a), a);
             }",
            "foo.js",
            |metric| {
                // 3 functions + 1 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 6,
                  "closure_args": 1,
                  "function_args_average": 2.0,
                  "closure_args_average": 1.0,
                  "total": 7,
                  "value": 0,
                  "average": 1.75,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 1
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "perl")]
    #[test]
    fn perl_no_functions_and_closures() {
        check_metrics::<PerlParser>(
            "my $x = 1;
             print $x;",
            "foo.pl",
            |metric| {
                // Cross-check via nom that no spurious sub/closure was
                // recognised — symmetric with the other `perl_*` nargs
                // tests, and would catch a regression that miscounted
                // `print` (or similar) as a function.
                assert_eq!(metric.nom.functions_sum(), 0);
                assert_eq!(metric.nom.closures_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "perl")]
    #[test]
    fn perl_single_function() {
        // This sub declares no signature, so it has no formal parameters to
        // count and nargs is 0 — args arrive via `@_`. Signature-carrying
        // subs are counted; see `perl_signature_function`. To make sure the
        // test still discriminates "function parsed" from "function silently
        // dropped", also assert nom recognised exactly one function.
        check_metrics::<PerlParser>(
            "sub greet {
                my ($name) = @_;
                print \"hi $name\";
            }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                assert_eq!(metric.nom.closures_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "perl")]
    #[test]
    fn perl_single_closure() {
        // This closure declares no signature, so nargs stays 0; it takes its
        // arguments through `@_`. A signature-carrying closure also reads 0,
        // for an unrelated upstream-grammar reason — see
        // `perl_anonymous_sub_signature_is_zero`. Assert via nom that the
        // anonymous function was actually identified as a closure.
        check_metrics::<PerlParser>(
            "my $f = sub {
                my ($x) = @_;
                return $x + 1;
            };",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 0);
                assert_eq!(metric.nom.closures_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "perl")]
    #[test]
    fn perl_multiple_functions() {
        // Neither sub declares a signature, so both count 0. Assert nom
        // counted both top-level subs so the test fails if either sub is
        // dropped.
        check_metrics::<PerlParser>(
            "sub a { return 1; }
             sub b {
                 my ($x, $y) = @_;
                 return $x + $y;
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 2);
                assert_eq!(metric.nom.closures_sum(), 0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "perl")]
    #[test]
    fn perl_nested_closure() {
        // Neither the outer sub nor the nested closure declares a signature,
        // so both count 0. Assert nom recognised one outer sub plus one
        // nested closure.
        check_metrics::<PerlParser>(
            "sub outer {
                my $inner = sub { return 42; };
                return $inner->();
            }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                assert_eq!(metric.nom.closures_sum(), 1);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// Regression for #1147: a signature sub reported 0 because the
    /// signature is an unnamed `function_signature` child, not a
    /// `parameters` field.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_signature_function() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub add($x, $y) { return $x + $y; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 2);
                assert_eq!(s.function_args_max(), 2);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// A defaulted parameter is a `binary_expression`, not a bare
    /// `scalar_variable`, so counting only the variable kinds would report
    /// 2 here instead of 3. Pins the negative filter in
    /// `compute_perl_args` (#1147).
    #[cfg(feature = "perl")]
    #[test]
    fn perl_signature_defaults_and_slurpy() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub deflt($x, $y = 5, @rest) { return $x; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 3);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// A signature sub and an `@_` sub in one file: the min/max and the
    /// average have to keep the zero-argument sub in the divisor rather
    /// than folding it away.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_signature_and_at_underscore_mixed() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub sig($x, $y, $z) { return $x; }
             sub legacy { my ($a) = @_; return $a; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 2);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 3);
                // 3 args over 2 functions: the zero-argument sub stays in
                // the divisor, so a fold that dropped it would read 3.0.
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 1.5,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 1.5,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// Perl puts subroutine attributes before the signature
    /// (`sub NAME ATTRS SIG BLOCK`), and a bare attribute swallows the
    /// signature into its own `function_attribute` node. Pins the
    /// one-level descent in `perl_signature`.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_signature_behind_attribute() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub attrs :lvalue ($z) { return $z; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 1);
                assert_eq!(s.function_args_max(), 1);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 1,
                  "closure_args": 0,
                  "function_args_average": 1.0,
                  "closure_args_average": 0.0,
                  "total": 1,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 1,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// A multi-line signature documents its parameters with `comments`
    /// children sitting directly under `function_signature`, so the
    /// negative filter has to exclude them or a documented 3-parameter sub
    /// reads 6 and trips the default `nargs` limit of 5.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_signature_comments_are_not_parameters() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub documented(
                 $host,    # hostname to connect to
                 $port,    # TCP port
                 $timeout, # seconds
             ) { return $host; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 3);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// The two shapes that must stay at 0 for reasons the counting rule
    /// depends on: an empty signature has no children but the parens, and
    /// a prototype (`($$)`) is a `function_prototype`, a different kind
    /// that `perl_signature` deliberately does not match.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_empty_signature_and_prototype_are_zero() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             sub empty() { return 1; }
             sub proto($$) { return 1; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 2);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 0);
                assert_eq!(s.function_args_max(), 0);
            },
        );
    }

    /// Perl 5.38's `method` is a second callable kind
    /// (`function_definition_without_sub`) reaching the same helper, so it
    /// gets its own fixture rather than riding on the `sub` tests.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_method_signature_function() {
        check_metrics::<PerlParser>(
            "use v5.38;
             class Point {
                 method shift_by($dx, $dy) { return $dx; }
             }",
            "foo.pl",
            |metric| {
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 2);
                assert_eq!(s.function_args_max(), 2);
            },
        );
    }

    /// `FunctionSignature2` is the hidden `_function_signature` supertype;
    /// `perl_signature` lists it defensively. Pin that the grammar never
    /// emits it, so a bump that promotes the rule fails loudly instead of
    /// changing behaviour invisibly (lesson 34).
    #[cfg(feature = "perl")]
    #[test]
    fn perl_hidden_function_signature_is_unreachable() {
        let mut hidden = false;
        let mut emitted = false;
        crate::test_support::for_each_node_with_chain::<PerlCode>(
            b"use feature 'signatures';\nsub add($x, $y) { return $x + $y; }\n",
            |node, _| {
                hidden |= node.kind_id() == Perl::FunctionSignature2 as u16;
                emitted |= node.kind_id() == Perl::FunctionSignature as u16;
            },
        );
        assert!(
            emitted,
            "fixture must reach a real `function_signature`, else the \
             hidden-rule check below is vacuous"
        );
        assert!(
            !hidden,
            "grammar now emits the hidden `_function_signature`; re-check \
             the defensive arm in `perl_signature`"
        );
    }

    /// Upstream-grammar limitation, deliberately pinned: tree-sitter-perl
    /// 1.1.2 parses an anonymous sub's signature inside an `ERROR` node,
    /// so a signature-carrying closure counts 0. A grammar bump that fixes
    /// the parse should fail this test rather than shift metrics silently.
    #[cfg(feature = "perl")]
    #[test]
    fn perl_anonymous_sub_signature_is_zero() {
        check_metrics::<PerlParser>(
            "use feature 'signatures';
             my $mul = sub ($p, $q) { return $p * $q; };",
            "foo.pl",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 0);
                assert_eq!(metric.nom.closures_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.closure_args_sum(), 0);
                assert_eq!(s.closure_args_max(), 0);
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn java_no_functions() {
        check_metrics::<JavaParser>(
            "class Foo {
                 int x = 42;
                 String name = \"hello\";
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn java_single_method() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void greet(String name, int count) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn java_multiple_methods() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void a(int x) {
                     return;
                 }
                 void b(int x, int y, int z) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn java_constructor_args() {
        check_metrics::<JavaParser>(
            "class Foo {
                 Foo(String name, int age) {
                     return;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    /// A record's compact constructor (`R { … }`, JLS 8.10.4) writes no
    /// formal parameter list: its parameters are the record's components,
    /// which the grammar hangs off the enclosing `record_declaration`.
    /// `nargs` reports the component count so the two spellings of one
    /// constructor agree — the canonical `R(int a, int b) { … }` reports
    /// 2, and so does the compact form (#1160).
    ///
    /// The records are nested, and carry *different* component counts, so
    /// the fixture pins which record each constructor resolved to. The
    /// lookup steps two ancestors up from the constructor; a version that
    /// walked to the outermost record instead would give `Single` 2 and
    /// total 4, and one that read the constructor node itself — which has
    /// no `parameters` field — would give 0.
    #[cfg(feature = "java")]
    #[test]
    fn java_record_compact_constructor_counts_record_components() {
        check_metrics::<JavaParser>(
            "record Pair(int a, int b) {
                 Pair { }
                 record Single(int c) {
                     Single { }
                 }
             }",
            "foo.java",
            |metric| {
                let s = &metric.nargs;
                // Two constructors carrying 2 and 1 arguments. Pre-fix,
                // `compact_constructor_declaration` was not a function at
                // all, so both the count and the sum were 0.
                assert_eq!(metric.nom.functions_sum(), 2);
                assert_eq!(s.function_args_sum(), 3);
                // A sum of 3 across two functions whose largest is 2 can
                // only be 2 + 1.
                assert_eq!(s.function_args_max(), 2);
                assert_eq!(s.closure_args_sum(), 0);
            },
        );
    }

    /// Java's explicit receiver parameter (`void m(S this, int a)`, JLS
    /// 8.4.1) parses as a `receiver_parameter` node — distinct from a real
    /// `formal_parameter` — and binds `this`, not a value. Like Rust's
    /// `self_parameter` (#457), Go's `receiver` field, and C++'s implicit
    /// `this`, it must not be counted as a formal parameter (#470).
    #[cfg(feature = "java")]
    #[test]
    fn java_method_excludes_explicit_receiver() {
        check_metrics::<JavaParser>(
            "class S {
                 void m(S this, int a) {}   // receiver + 1 -> 1 arg
                 void n(int a, int b) {}     // control: 2 real params
                 void r(S this) {}           // receiver only  -> 0 args
             }",
            "foo.java",
            |metric| {
                // m:1 + n:2 + r:0. The two receiver parameters contribute
                // nothing. Pre-fix, the receivers inflated this to sum = 5
                // (m:2 + n:2 + r:1), max = 2. After #470: sum = 3, max = 2.
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 2);
            },
        );
    }

    #[cfg(feature = "java")]
    #[test]
    fn java_lambda_args() {
        check_metrics::<JavaParser>(
            "class Foo {
                 void run() {
                     Runnable r = (int a, int b) -> a + b;
                 }
             }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_no_functions_and_closures() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.nargs.total(), 0);
        });
    }

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_single_method() {
        check_metrics::<GroovyParser>(
            "class A {
                void greet(String name, int times) {
                    println(name)
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 2);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
            },
        );
    }

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_multiple_methods() {
        check_metrics::<GroovyParser>(
            "class A {
                int add(int x, int y) { x + y }
                int sub(int x, int y, int z) { x - y - z }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 5);
            },
        );
    }

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_lambda_args() {
        // Two-parameter Groovy closure inside a method body. Groovy's
        // primary lambda-shaped construct is the closure
        // (`{ params -> body }`); the dekobon grammar does not model
        // Java's `(params) -> body` arrow form because real-world
        // Groovy code rarely uses it.
        check_metrics::<GroovyParser>(
            "class Foo {
                void run() {
                    def f = { int a, int b -> a + b }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 2);
            },
        );
    }

    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_implicit_it_not_counted() {
        // The `it` implicit closure parameter is just an identifier in
        // the grammar — no `formal_parameters` node. `nargs` counts
        // declared parameters only, so this closure has 0.
        check_metrics::<GroovyParser>(
            "class A {
                void apply() {
                    [1, 2, 3].each { println(it) }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 0);
            },
        );
    }

    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_no_functions() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 int x = 42;
                 string Name = \"hello\";
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_single_method() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void Greet(string name, int count) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_multiple_methods() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void A(int x) {
                     return;
                 }
                 void B(int x, int y, int z) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_constructor_args() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 public Foo(string name, int age) {
                     return;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_lambda_args() {
        check_metrics::<CsharpParser>(
            "class Foo {
                 void Run() {
                     System.Func<int, int, int> f = (int a, int b) => a + b;
                 }
             }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    /// A `params` array is one parameter. The grammar's hidden
    /// `_parameter_array` rule flattens its pieces into the list, so the
    /// shared negative filter read `F(params int[] xs)` as 3 and the
    /// attributed form as 4; the positive count reads one each, and the
    /// ordinary parameters beside it — attributed, `ref`/`out`/`in`,
    /// defaulted — still one each. Every C# callable shape that can
    /// carry one is a row, each pinned by its own space, so the file
    /// total cannot hide one shape compensating for another.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_params_array_is_one_parameter() {
        check_spaces::<CsharpParser, _>(
            "class C {
                 public C(params int[] xs) { }
                 void M(params int[] ys) { }
                 void A([Attr] params int[] zs) { }
                 void B([Attr] int a, ref int b, out int c, in int d, int e = 1, params int[] f) { }
                 int this[params int[] i] => 0;
                 void L() {
                     var f = (params int[] a) => 0;
                     var g = x => x;
                     var h = delegate(int q, params int[] r) { };
                 }
             }",
            "params.cs",
            |space| {
                let class = &space.spaces[0];
                let rows: Vec<(Option<&str>, u64, u64)> = class
                    .spaces
                    .iter()
                    .map(|s| {
                        (
                            s.name.as_deref(),
                            s.metrics.nargs.function_args(),
                            s.metrics.nargs.closure_args(),
                        )
                    })
                    .collect();
                assert_eq!(
                    rows,
                    vec![
                        (Some("C"), 1, 0),
                        (Some("M"), 1, 0),
                        (Some("A"), 1, 0),
                        (Some("B"), 6, 0),
                        (Some("<anonymous>"), 1, 0),
                        (Some("L"), 0, 0),
                    ]
                );
                let l = &class.spaces[5];
                let lambdas: Vec<u64> = l
                    .spaces
                    .iter()
                    .map(|s| s.metrics.nargs.closure_args())
                    .collect();
                assert_eq!(lambdas, vec![1, 1, 2]);
                assert_eq!(space.metrics.nargs.function_args_sum(), 10);
                assert_eq!(space.metrics.nargs.closure_args_sum(), 4);
            },
        );
        // The count rests on `_parameter_array` staying hidden: a grammar
        // that promoted it to a named wrapper around its `params` token
        // would read 2 per array through `Parameter | Params`. Pinned
        // per lesson 34 so the promotion fails here, not in a corpus.
        let parser = CsharpParser::new(
            b"class C { void M(params int[] ys) { } }".to_vec(),
            std::path::Path::new("hidden.cs"),
            None,
        );
        assert!(
            !crate::test_support::ast_has_kind_id(&parser, Csharp::ParameterArray as u16),
            "`_parameter_array` is a hidden rule and never emitted"
        );
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_function_and_arrow() {
        check_metrics::<TsxParser>(
            "function add(a: number, b: number): number {
                 return a + b;
             }
             const multiply = (x: number, y: number) => x * y;",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_typed_and_optional_params() {
        check_metrics::<TypescriptParser>(
            "function format(value: number, prefix?: string, suffix?: string): string {
                 return (prefix ?? '') + value.toString() + (suffix ?? '');
             }
             const identity = (x: number): number => x;",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 4,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_single_function() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {
                 return a * b;
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_closure_args() {
        check_metrics::<MozjsParser>("function (a, b) {return a + b};", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 2,
              "function_args_average": 0.0,
              "closure_args_average": 2.0,
              "total": 2,
              "value": 0,
              "average": 2.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 2
            }
            "#
            );
        });
    }

    // Regression tests for issue #77: bare-identifier arrow functions
    // (`x => x`) use the singular `parameter` field instead of the plural
    // `parameters` field, and were previously counted as nargs=0.
    //
    // `total` is used so the assertion is independent of whether the
    // arrow function is classified as a function or a closure (this depends
    // on its enclosing context — e.g. a `VariableDeclarator` ancestor makes
    // it a function).

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_async_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn javascript_multi_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 2);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_async_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = async x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_parenthesized_arrow_function() {
        check_metrics::<TypescriptParser>("const f = (x: number) => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn typescript_multi_parenthesized_arrow_function() {
        check_metrics::<TypescriptParser>(
            "const f = (x: number, y: number) => x + y;",
            "foo.ts",
            |metric| {
                assert_eq!(metric.nargs.total(), 2);
            },
        );
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_async_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = async x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_parenthesized_arrow_function() {
        check_metrics::<TsxParser>("const f = (x: number) => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "typescript")]
    #[test]
    fn tsx_multi_parenthesized_arrow_function() {
        check_metrics::<TsxParser>(
            "const f = (x: number, y: number) => x + y;",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.nargs.total(), 2);
            },
        );
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_async_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_multi_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 2);
        });
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_nargs_functions_and_closures() {
        check_metrics::<KotlinParser>(
            "fun add(a: Int, b: Int): Int {
                val transform = { x: Int, y: Int -> x + y }
                return transform(a, b)
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 2,
                  "function_args_average": 2.0,
                  "closure_args_average": 2.0,
                  "total": 4,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_no_functions_and_closures() {
        check_metrics::<LuaParser>("local x = 1", "foo.lua", |metric| {
            // No functions or closures: both halves are zero.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_single_function() {
        check_metrics::<LuaParser>("function f(a, b) return a + b end", "foo.lua", |metric| {
            // f(a, b) → fn_args_sum 2, no closures.
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_single_closure() {
        check_metrics::<LuaParser>(
            "local f = function(a, b) return a + b end",
            "foo.lua",
            |metric| {
                // Anonymous `function(a, b)` bound via `local` → closure_args_sum 2.
                assert_eq!(metric.nargs.function_args_sum(), 0);
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_functions() {
        check_metrics::<LuaParser>(
            "function f(a) return a end
function g(x, y, z) return x + y + z end",
            "foo.lua",
            |metric| {
                // f(a)=1 + g(x,y,z)=3 → fn_args_sum 4.
                assert_eq!(metric.nargs.function_args_sum(), 4);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_vararg_function() {
        // `...` is a vararg_expression node and counts as one argument.
        check_metrics::<LuaParser>("function f(a, ...) return a end", "foo.lua", |metric| {
            // a + ... → fn_args_sum 2.
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "lua")]
    #[test]
    fn lua_colon_method_nargs() {
        // Colon syntax: `self` is implicit and NOT in the `parameters` node.
        // Only explicit params (a, b) are counted.
        check_metrics::<LuaParser>(
            "function obj:method(a, b) return a + b end",
            "foo.lua",
            |metric| {
                // Only explicit a, b → fn_args_sum 2 (implicit self excluded).
                assert_eq!(metric.nargs.function_args_sum(), 2);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_no_functions() {
        check_metrics::<TclParser>("set x 1", "foo.tcl", |metric| {
            // Bare `set` command, no procs → both halves zero.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_single_function() {
        check_metrics::<TclParser>("proc f {a b} { puts $a }", "foo.tcl", |metric| {
            // proc f {a b} → fn_args_sum 2.
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_single_function_no_args() {
        check_metrics::<TclParser>("proc f {} { puts hello }", "foo.tcl", |metric| {
            // proc f {} → empty arg list, fn_args_sum 0.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_functions() {
        check_metrics::<TclParser>(
            "proc f {a b} { puts $a }
proc g {x y z} { puts $x }",
            "foo.tcl",
            |metric| {
                // f(a,b)=2 + g(x,y,z)=3 → fn_args_sum 5.
                assert_eq!(metric.nargs.function_args_sum(), 5);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_nested_functions() {
        check_metrics::<TclParser>(
            "proc outer {a} {
    proc inner {x y} { puts $x }
    inner $a $a
}",
            "foo.tcl",
            |metric| {
                // outer(a)=1 + inner(x,y)=2 → fn_args_sum 3.
                assert_eq!(metric.nargs.function_args_sum(), 3);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_args_vararg() {
        // `args` is the Tcl variadic catch-all; it counts as one argument.
        check_metrics::<TclParser>("proc f {a b args} { puts $a }", "foo.tcl", |metric| {
            // a + b + args → fn_args_sum 3 (variadic is one slot).
            assert_eq!(metric.nargs.function_args_sum(), 3);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "tcl")]
    #[test]
    fn tcl_default_arg() {
        // `{name default}` is a single argument with a default value.
        check_metrics::<TclParser>(
            "proc greet {{name World} greeting} {
    puts \"$greeting, $name!\"
}",
            "foo.tcl",
            |metric| {
                // {name World} counts as one slot + greeting → fn_args_sum 2.
                assert_eq!(metric.nargs.function_args_sum(), 2);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_zero_args() {
        check_metrics::<KotlinParser>("fun f(): Int { return 42 }", "foo.kt", |metric| {
            // fun f() → empty parameter list, fn_args_sum 0.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_single_arg() {
        check_metrics::<KotlinParser>(
            "fun double(x: Int): Int { return x * 2 }",
            "foo.kt",
            |metric| {
                // double(x) → fn_args_sum 1.
                assert_eq!(metric.nargs.function_args_sum(), 1);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_multiple_args() {
        check_metrics::<KotlinParser>(
            "fun add(a: Int, b: Int, c: Int): Int { return a + b + c }",
            "foo.kt",
            |metric| {
                // add(a, b, c) → fn_args_sum 3.
                assert_eq!(metric.nargs.function_args_sum(), 3);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_default_args() {
        check_metrics::<KotlinParser>(
            "fun greet(name: String = \"World\", greeting: String = \"Hello\"): String {
                 return \"$greeting, $name!\"
             }",
            "foo.kt",
            |metric| {
                // Defaults still count as parameter slots → fn_args_sum 2.
                assert_eq!(metric.nargs.function_args_sum(), 2);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_empty_lambda() {
        // Two lambdas in the same function body: one with two explicit parameters
        // (proving the lambda path is taken and args are counted), and one with an
        // explicit empty parameter list `{ -> expr }` (proving
        // `compute_kotlin_lambda_args` returns 0 for it without crashing or
        // accidentally counting tokens inside the arrow expression).
        // If the grammar fails to parse either lambda, `total_closures` would be
        // lower than 2, making the snapshot unambiguous.
        check_metrics::<KotlinParser>(
            "fun f() {
                 val two = { x: Int, y: Int -> x + y }
                 val zero = { -> 42 }
             }",
            "foo.kt",
            |metric| {
                // Outer fun f() has 0 params; two lambdas counted as closures:
                // {x, y -> ...} contributes 2, {-> 42} contributes 0 →
                // closure_args_sum 2 across two closure entries.
                assert_eq!(metric.nargs.function_args_sum(), 0);
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_anonymous_function() {
        // `fun(x: Int, y: Int) = x + y` — anonymous function expression.
        // The grammar surfaces it as an `AnonymousFunction` node, which routes
        // through `compute_kotlin_func_args` (not the lambda path).
        check_metrics::<KotlinParser>(
            "val add = fun(x: Int, y: Int): Int = x + y",
            "foo.kt",
            |metric| {
                // Anonymous fun(x, y) is classified as a closure → closure_args_sum 2.
                assert_eq!(metric.nargs.function_args_sum(), 0);
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                insta::assert_json_snapshot!(metric.nargs);
            },
        );
    }

    /// The `(name, kind, own fn args, own closure args)` rows of a space
    /// tree in walk order, for the primary-constructor fixtures below:
    /// two constructors share their class's name, so the rows are read
    /// positionally rather than by name.
    #[cfg(any(
        feature = "kotlin",
        feature = "csharp",
        feature = "java",
        feature = "groovy"
    ))]
    fn space_rows(space: &FuncSpace) -> Vec<(String, SpaceKind, u64, u64)> {
        fn walk(space: &FuncSpace, rows: &mut Vec<(String, SpaceKind, u64, u64)>) {
            rows.push((
                space.name.clone().unwrap_or_default(),
                space.kind,
                space.metrics.nargs.function_args(),
                space.metrics.nargs.closure_args(),
            ));
            for child in &space.spaces {
                walk(child, rows);
            }
        }
        let mut rows = Vec::new();
        for child in &space.spaces {
            walk(child, &mut rows);
        }
        rows
    }

    /// One `space_rows` row with no closure arguments, for the
    /// function-space fixtures below.
    #[cfg(any(
        feature = "kotlin",
        feature = "csharp",
        feature = "java",
        feature = "groovy"
    ))]
    fn row(name: &str, kind: SpaceKind, args: u64) -> (String, SpaceKind, u64, u64) {
        (name.to_owned(), kind, args, 0)
    }

    /// #1828's reproducer: a six-parameter primary constructor counted
    /// nowhere, so `bca check --threshold nargs=5` passed it. It opens a
    /// `Function` space named after its class (the `FuncOpener` row of
    /// `roles/kotlin.rs`), which owns the six; `nom` counts it as the
    /// callable the average divides by. Every row read 0 on `main`.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_primary_constructor_owns_its_parameters() {
        check_spaces::<KotlinParser, _>(
            "data class D(val a: Int, val b: Int, val c: Int, val d: Int, val e: Int, val f: Int)",
            "d.kt",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        ("D".to_owned(), SpaceKind::Class, 0, 0),
                        ("D".to_owned(), SpaceKind::Function, 6, 0),
                    ]
                );
                let ctor = &space.spaces[0].spaces[0];
                assert_eq!(ctor.metrics.nargs.own_args(), 6);
                assert_eq!((ctor.start_line, ctor.end_line), (1, 1));
                assert_eq!(space.metrics.nom.functions_sum(), 1);
                assert_eq!(space.metrics.nargs.function_args_sum(), 6);
                assert!((space.metrics.nargs.function_args_average() - 6.0).abs() < 1e-9);
            },
        );
    }

    /// The two constructor spellings beside each other, with the field
    /// lambda that made the container route wrong: the class keeps the
    /// lambda's two closure parameters (a Kotlin lambda opens no
    /// space), the primary constructor owns exactly its two, the
    /// secondary its one. The superclass call `Base(a)` is a sibling of
    /// the parameter list, so the class still owns that ABC branch
    /// (#1456), and the `val a` property is still the class's own
    /// attribute, not the constructor's.
    /// The secondary constructor reads `<anonymous>` as it did before
    /// #1828 — `secondary_constructor` has no `name` field and no
    /// override names it — a gap of its own, not this test's.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_primary_and_secondary_constructors_each_own_their_parameters() {
        check_func_space::<KotlinParser, _>(
            "class P(val a: Int, b: Int) : Base(a) {
                 constructor(x: Int) : this(x, x)
                 val cmp = { p: Int, q: Int -> p }
             }",
            "p.kt",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        ("P".to_owned(), SpaceKind::Class, 0, 2),
                        ("P".to_owned(), SpaceKind::Function, 2, 0),
                        ("<anonymous>".to_owned(), SpaceKind::Function, 1, 0),
                    ]
                );
                let class = &space.spaces[0];
                assert_eq!(class.metrics.abc.branches(), 1, "`Base(a)` is the class's");
                assert_eq!(class.spaces[0].metrics.abc.branches(), 0);
                assert_eq!(class.spaces[1].metrics.abc.branches(), 1, "`this(x, x)`");
                // `val a` (a parameter property) and `val cmp` are both
                // the class's own attributes; the constructor declares
                // none — `kotlin_parameter_properties_stay_the_classs`
                // pins the split.
                assert_eq!(class.metrics.npa.class_na(), 2);
                assert_eq!(class.spaces[0].metrics.npa.class_na(), 0);
                assert_eq!(class.metrics.nom.functions_sum(), 2);
                assert_eq!(class.metrics.nom.closures_sum(), 1);
            },
        );
    }

    /// Every preamble the grammar permits before the list, and the
    /// shapes that must not open one: the `constructor` keyword, an
    /// annotation and a visibility modifier on it, an `enum class` and
    /// an `annotation class`, empty parens (a declared zero-parameter
    /// constructor), a comment and a trailing comma in the list, a
    /// default, a local class inside a function (billed to the local
    /// class's constructor, not to `outer`), and a class with no list
    /// at all, which opens nothing.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_primary_constructor_preambles() {
        check_spaces::<KotlinParser, _>(
            "class E constructor(x: Int)
             class F @Inject private constructor(x: Int, y: Int)
             enum class G(val v: Int) { A(1) }
             annotation class H(val x: Int)
             class N
             class Z()
             class C(val a: Int, /* c */ val b: Int = 2,)
             fun outer(p: Int) { class Loc(val q: Int, val r: Int) }",
            "preambles.kt",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("E", SpaceKind::Class, 0),
                        row("E", SpaceKind::Function, 1),
                        row("F", SpaceKind::Class, 0),
                        row("F", SpaceKind::Function, 2),
                        row("G", SpaceKind::Class, 0),
                        row("G", SpaceKind::Function, 1),
                        row("H", SpaceKind::Class, 0),
                        row("H", SpaceKind::Function, 1),
                        row("N", SpaceKind::Class, 0),
                        row("Z", SpaceKind::Class, 0),
                        row("Z", SpaceKind::Function, 0),
                        row("C", SpaceKind::Class, 0),
                        row("C", SpaceKind::Function, 2),
                        row("outer", SpaceKind::Function, 1),
                        row("Loc", SpaceKind::Class, 0),
                        row("Loc", SpaceKind::Function, 2),
                    ]
                );
            },
        );
    }

    /// A primary constructor's space holds its parameter list and no
    /// code, so a parameterless one — `class Z()` — has no Halstead
    /// volume and reads the `mi.*` `0.0` sentinel, which `bca check`
    /// and `bca report` skip (#1621); a parameterised one's list is its
    /// volume. Pinned so the exemption is known to cover the new rows,
    /// and so a serialiser reading `0.0` literally knows why one is
    /// there. `check_func_space` because `mi` pulls its own inputs.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_parameterless_primary_constructor_is_mi_empty() {
        check_func_space::<KotlinParser, _>(
            "class Z()
class A(val x: Int)",
            "z.kt",
            |space| {
                let z = &child_space(&space, "Z").spaces[0];
                let a = &child_space(&space, "A").spaces[0];
                assert_eq!((z.kind, a.kind), (SpaceKind::Function, SpaceKind::Function));
                assert!(z.metrics.mi.is_empty(), "no list, no volume");
                assert_eq!(z.metrics.mi.original(), 0.0);
                assert!(!a.metrics.mi.is_empty(), "the list is the space's code");
            },
        );
    }

    /// A default spelled as a lambda is part of the signature, so its
    /// parameters are the constructor's closure parameters, exactly as
    /// a function's spaceless lambdas are its own (#1196): `own_args`
    /// reads 1 + 2. Pinned so the row's number is known to carry the
    /// default's lambda rather than the written arity alone.
    #[cfg(feature = "kotlin")]
    #[test]
    fn kotlin_primary_constructor_default_lambda_is_its_closure() {
        check_spaces::<KotlinParser, _>(
            "class L(val f: (Int, Int) -> Int = { a, b -> a + b })",
            "l.kt",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        ("L".to_owned(), SpaceKind::Class, 0, 0),
                        ("L".to_owned(), SpaceKind::Function, 1, 2),
                    ]
                );
                assert_eq!(space.spaces[0].spaces[0].metrics.nargs.own_args(), 3);
            },
        );
    }

    /// #1828's C# reproducer: a C# 12 primary constructor is the
    /// `parameter_list` hanging directly off the type declaration — the
    /// `FuncOpener` row of `roles/csharp.rs` gates it on that parent —
    /// and it opens a `Function` space named after the type, owning the
    /// six. Every row read 0 on `main`.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_primary_constructor_owns_its_parameters() {
        check_spaces::<CsharpParser, _>(
            "public class C(int a, int b, int c, int d, int e, int f)
             {
                 public int Sum() => a + b + c + d + e + f;
             }",
            "c.cs",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        ("C".to_owned(), SpaceKind::Class, 0, 0),
                        ("C".to_owned(), SpaceKind::Function, 6, 0),
                        ("Sum".to_owned(), SpaceKind::Function, 0, 0),
                    ]
                );
                let ctor = &space.spaces[0].spaces[0];
                assert_eq!(ctor.metrics.nargs.own_args(), 6);
                assert_eq!((ctor.start_line, ctor.end_line), (1, 1));
                assert_eq!(space.metrics.nom.functions_sum(), 2);
                assert!((space.metrics.nargs.function_args_average() - 3.0).abs() < 1e-9);
            },
        );
    }

    /// Every type that can carry a primary constructor, and the shapes
    /// around it: a positional `record`, `record struct` and `record
    /// class`, a `struct`, a class delegating to its base (the
    /// `base_list` is a sibling of the list, so the class keeps that
    /// ABC branch — #1456), a class with no list and one with empty
    /// parens, a generic class whose `type_parameter_list` is a
    /// different kind, a comment in the list beside an explicit
    /// `this(…)` constructor, nesting in a namespace and in a class,
    /// and a method's own list, which opens nothing of its own.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_primary_constructor_shapes() {
        check_func_space::<CsharpParser, _>(
            "public record R(int A, int B);
             public record struct RS(int A);
             public record class RC(int A, int B, int C);
             public struct S(int x) { }
             public class Sub(int x) : Base(x) { }
             public class N { }
             public class Z() { }
             public class G<T>(T x, [Attr] int y = 1, params int[] zs) { }
             public class H(int a /* c */, int b) { public H() : this(1, 2) { } }
             namespace Ns { public class Nested(int q) { public class Inner(int r, int s) { void M(int t) { } } } }",
            "shapes.cs",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("R", SpaceKind::Class, 0),
                        row("R", SpaceKind::Function, 2),
                        row("RS", SpaceKind::Class, 0),
                        row("RS", SpaceKind::Function, 1),
                        row("RC", SpaceKind::Class, 0),
                        row("RC", SpaceKind::Function, 3),
                        row("S", SpaceKind::Class, 0),
                        row("S", SpaceKind::Function, 1),
                        row("Sub", SpaceKind::Class, 0),
                        row("Sub", SpaceKind::Function, 1),
                        row("N", SpaceKind::Class, 0),
                        row("Z", SpaceKind::Class, 0),
                        row("Z", SpaceKind::Function, 0),
                        row("G", SpaceKind::Class, 0),
                        row("G", SpaceKind::Function, 3),
                        row("H", SpaceKind::Class, 0),
                        row("H", SpaceKind::Function, 2),
                        row("H", SpaceKind::Function, 0),
                        row("Nested", SpaceKind::Class, 0),
                        row("Nested", SpaceKind::Function, 1),
                        row("Inner", SpaceKind::Class, 0),
                        row("Inner", SpaceKind::Function, 2),
                        row("M", SpaceKind::Function, 1),
                    ]
                );
                let sub = child_space(&space, "Sub");
                assert_eq!(sub.metrics.abc.branches(), 1, "`: Base(x)` stays the class's");
                assert_eq!(sub.spaces[0].metrics.abc.branches(), 0);
            },
        );
    }

    /// The classic and primary spellings of one constructor agree, so a
    /// migration to primary constructors moves no `nargs` number — the
    /// `params` row included, which is why its miscount was fixed
    /// alongside (`csharp_params_array_is_one_parameter`).
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_primary_and_classic_constructors_agree() {
        check_spaces::<CsharpParser, _>(
            "class Primary(int a, string b, params int[] rest) { }
             class Classic { public Classic(int a, string b, params int[] rest) { } }",
            "twins.cs",
            |space| {
                let primary = child_space(&space, "Primary").spaces[0]
                    .metrics
                    .nargs
                    .own_args();
                let classic = child_space(&space, "Classic").spaces[0]
                    .metrics
                    .nargs
                    .own_args();
                assert_eq!((primary, classic), (3, 3));
            },
        );
    }

    /// Error recovery: a list with no closing paren parses to an `ERROR`
    /// root holding the `parameter` nodes bare, with no `parameter_list`
    /// at all, so no constructor space opens. This cannot see the row's
    /// parent gate — with no list node there is nothing for the gate to
    /// reject, and an ungated row answers the same here — it records
    /// the recovery shape, so a grammar bump that starts recovering the
    /// list under `ERROR` changes this row rather than a corpus
    /// snapshot.
    #[cfg(feature = "csharp")]
    #[test]
    fn csharp_unclosed_primary_constructor_opens_nothing() {
        check_spaces::<CsharpParser, _>("class C(int a,", "broken.cs", |space| {
            assert_eq!(space_rows(&space), []);
            assert_eq!(space.metrics.nargs.total(), 0);
        });
    }

    /// #1842's reproducer: a six-component Java record with no
    /// constructor counted its components nowhere, so
    /// `bca check --threshold nargs=5` passed it. The header's
    /// `formal_parameters` opens a `Function` space named after the
    /// record (the gated `FuncOpener` owner of `roles/java.rs`), which
    /// owns the six; `nom` counts it as the callable the average divides
    /// by. Every row read 0 on `main`.
    #[cfg(feature = "java")]
    #[test]
    fn java_record_header_owns_its_components() {
        check_spaces::<JavaParser, _>(
            "record R(int a, int b, int c, int d, int e, int f) {}",
            "r.java",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("R", SpaceKind::Class, 0),
                        row("R", SpaceKind::Function, 6),
                    ]
                );
                let ctor = &space.spaces[0].spaces[0];
                assert_eq!(ctor.metrics.nargs.own_args(), 6);
                assert_eq!((ctor.start_line, ctor.end_line), (1, 1));
                assert_eq!(space.metrics.nom.functions_sum(), 1);
                assert_eq!(space.metrics.nargs.function_args_sum(), 6);
                assert!((space.metrics.nargs.function_args_average() - 6.0).abs() < 1e-9);
            },
        );
    }

    /// Exactly one owner of the components in every constructor
    /// combination JLS 8.10.4 allows, the header's space present only
    /// where the body writes no canonical constructor: implicit (`I`),
    /// compact (`K`, #1160's redirect counts the components there),
    /// explicit canonical (`N`, told by its types — spelled with a
    /// qualified generic to pin that the comparison reads the whole
    /// type), alternative only (`A`, a callable of its own beside the
    /// header, which it does not suppress), alternative beside a
    /// compact (`B`, the compact owns the two, the alternative its one).
    /// A compact plus a normal canonical constructor is invalid Java
    /// and is not a fixture. The file sum is each record's component
    /// count once plus the alternatives' own: 2+2+2+(2+1)+(2+1) = 12.
    #[cfg(feature = "java")]
    #[test]
    fn java_record_components_have_exactly_one_owner() {
        // `check_func_space`, not the `nargs`-only shim: the compact
        // constructor is pinned by its cyclomatic and nexits below.
        check_func_space::<JavaParser, _>(
            "record I(int a, int b) {}
             record K(int a, int b) { K { if (a < 0) throw new IllegalArgumentException(); } }
             record N(int a, java.util.List<String> b) { N(int a, java.util.List<String> b) { this.a = a; this.b = b; } }
             record A(int a, int b) { A(int a) { this(a, 0); } }
             record B(int a, int b) { B { } B(int a) { this(a, 0); } }",
            "owners.java",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("I", SpaceKind::Class, 0),
                        row("I", SpaceKind::Function, 2),
                        row("K", SpaceKind::Class, 0),
                        row("K", SpaceKind::Function, 2),
                        row("N", SpaceKind::Class, 0),
                        row("N", SpaceKind::Function, 2),
                        row("A", SpaceKind::Class, 0),
                        row("A", SpaceKind::Function, 2),
                        row("A", SpaceKind::Function, 1),
                        row("B", SpaceKind::Class, 0),
                        row("B", SpaceKind::Function, 2),
                        row("B", SpaceKind::Function, 1),
                    ]
                );
                assert_eq!(space.metrics.nargs.function_args_sum(), 12);
                assert_eq!(space.metrics.nom.functions_sum(), 7);
                // `K`'s one row is the compact constructor, which the
                // metric names from its own `name` field; the header
                // row would be named from the record and read the same,
                // so pin it by what only the compact body holds: its
                // `if` and `throw`.
                let k = &child_space(&space, "K").spaces[0];
                assert_eq!(k.metrics.cyclomatic.cyclomatic(), 2);
                assert_eq!(k.metrics.nexits.nexits(), 1);
            },
        );
    }

    /// The shapes a header can take and the places a record can sit: no
    /// components (`E`, a declared zero-parameter constructor, like C#'s
    /// `class G()`), a variable-arity component (`V`, one parameter), a
    /// generic record with an annotated component (`G`, the
    /// `type_parameters` is a different kind and an annotation is not
    /// a parameter), a comment in the header beside a method whose own
    /// list opens nothing, a member record and a local record inside a
    /// method (each billed to its own header, not to `m`), and a record
    /// implementing an interface.
    #[cfg(feature = "java")]
    #[test]
    fn java_record_header_shapes() {
        check_spaces::<JavaParser, _>(
            "record E() {}
             record V(int... xs) {}
             record G<T>(T a, @Deprecated int b) {}
             record C(int a /* c */, int b) { int m(int t) { return t; } }
             class O { record N(int a) {} void m() { record L(int x, int y) {} } }
             record S(int a) implements java.io.Serializable {}",
            "shapes.java",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("E", SpaceKind::Class, 0),
                        row("E", SpaceKind::Function, 0),
                        row("V", SpaceKind::Class, 0),
                        row("V", SpaceKind::Function, 1),
                        row("G", SpaceKind::Class, 0),
                        row("G", SpaceKind::Function, 2),
                        row("C", SpaceKind::Class, 0),
                        row("C", SpaceKind::Function, 2),
                        row("m", SpaceKind::Function, 1),
                        row("O", SpaceKind::Class, 0),
                        row("N", SpaceKind::Class, 0),
                        row("N", SpaceKind::Function, 1),
                        row("m", SpaceKind::Function, 0),
                        row("L", SpaceKind::Class, 0),
                        row("L", SpaceKind::Function, 2),
                        row("S", SpaceKind::Class, 0),
                        row("S", SpaceKind::Function, 1),
                    ]
                );
            },
        );
    }

    /// The explicit canonical constructor is recognised by its
    /// parameter *types*, not its arity or its names (JLS 8.10.4.1): a
    /// same-arity constructor with another type (`A`), with the
    /// components' order swapped (`O`), with an array where the
    /// component is variable-arity (`W`), or with the element type
    /// spelled bare where the component is variable-arity (`U` — the
    /// one pair the arity marker alone tells apart, `int` reading as
    /// `int` on both sides) is an alternative, so the header opens
    /// beside it; the same types under other names (`M`), with other
    /// annotations (`T`), with the spaces inside a generic laid out
    /// differently (`Y`) or with a variable-arity parameter matching a
    /// variable-arity component (`X`) is canonical, and the header
    /// closes. The type is compared as spelled: `List<String>` beside
    /// a `java.util.List<String>` component (`Q`) is read as an
    /// alternative, the limit `roles/java.rs` documents, and so is a
    /// type-use annotation on the component (`N`, `List<@Nullable
    /// String>` beside `List<String>`): a token of the type, not
    /// resolved. A comment is no token: inside the generic arguments
    /// (`G`), inside the C-style `dimensions` (`H`) or between a
    /// variable-arity parameter's modifiers and its type (`V`, where
    /// the type is sought as the first named child that is no modifier
    /// or annotation, and a `block_comment` is named) the constructor
    /// still reads as canonical. C-style trailing dimensions are part
    /// of the spelling: `int a[]` is `int` plus `[]`, the same type
    /// `int[] a` spells (`D`, canonical, JLS 8.4.1), and `int a[][]`
    /// one dimension more (`Z`, an alternative). Arity is part of the
    /// rule wherever the lists part ways: a constructor whose parameters
    /// run past the components (`P`) or stop short of them (`S`) is an
    /// alternative however far the types agree. `Nullable` is declared
    /// at the end so `N` is the valid Java javac accepts (`@Deprecated`
    /// is not a type-use annotation); its row is the annotation type's.
    #[cfg(feature = "java")]
    #[test]
    fn java_canonical_constructor_is_told_by_its_types() {
        check_spaces::<JavaParser, _>(
            "record A(int a) { A(long a) { this((int) a); } }
             record O(int a, String b) { O(String b, int a) { this(a, b); } }
             record W(int... xs) { W(int[] xs) { this.xs = xs; } }
             record U(int... xs) { U(int x) { this(new int[] { x }); } }
             record M(int a) { M(int other) { this.a = other; } }
             record T(@Deprecated int a) { T(@SuppressWarnings(\"x\") int a) { this.a = a; } }
             record Y(java.util.Map<String, Integer> m) { Y(java.util.Map<String,Integer> m) { this.m = m; } }
             record X(int... xs) { X(int... xs) { this.xs = xs; } }
             record Q(java.util.List<String> l) { Q(List<String> l) { this.l = l; } }
             record D(int[] a) { D(int a[]) { this.a = a; } }
             record Z(int[] a) { Z(int a[][]) { this.a = a[0]; } }
             record N(java.util.List<@Nullable String> l) { N(java.util.List<String> l) { this.l = l; } }
             record G(java.util.List</* c */ String> l) { G(java.util.List<String> l) { this.l = l; } }
             record H(int[] a) { H(int a[/* c */]) { this.a = a; } }
             record V(int... xs) { V(final /* c */ int... xs) { this.xs = xs; } }
             record P(int a) { P(int a, int b) { this(a); } }
             record S(int a, int b) { S(int a) { this(a, 0); } }
             @java.lang.annotation.Target(java.lang.annotation.ElementType.TYPE_USE) @interface Nullable { }",
            "types.java",
            |space| {
                let functions_per_record: Vec<(&str, usize)> = space
                    .spaces
                    .iter()
                    .map(|r| (r.name.as_deref().unwrap_or("?"), r.spaces.len()))
                    .collect();
                assert_eq!(
                    functions_per_record,
                    [
                        ("A", 2),
                        ("O", 2),
                        ("W", 2),
                        ("U", 2),
                        ("M", 1),
                        ("T", 1),
                        ("Y", 1),
                        ("X", 1),
                        ("Q", 2),
                        ("D", 1),
                        ("Z", 2),
                        ("N", 2),
                        ("G", 1),
                        ("H", 1),
                        ("V", 1),
                        ("P", 2),
                        ("S", 2),
                        ("Nullable", 0),
                    ]
                );
            },
        );
    }

    /// Groovy's twin of `java_canonical_constructor_is_told_by_its_types`,
    /// without the shapes the pinned grammar cannot parse (an annotated
    /// component, a variable-arity component, C-style dimensions): a
    /// same-arity constructor with another type (`A`) or the components'
    /// order swapped (`O`) is an alternative and the header opens beside
    /// it; the same types under other names (`M`) or with the spaces
    /// inside a generic laid out differently (`Y`) is canonical and the
    /// header closes. `List<String>` beside `java.util.List<String>`
    /// (`Q`) is an alternative, the spelling limit `roles/groovy.rs`
    /// documents. A comment is no token: inside the generic arguments
    /// (`G`), inside a qualified name (`K`) or inside an array type's
    /// brackets (`H`) the constructor still reads as canonical. A
    /// constructor whose parameters run past the components (`P`) or
    /// stop short of them (`S`) is an alternative.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_canonical_constructor_is_told_by_its_types() {
        check_spaces::<GroovyParser, _>(
            "record A(int a) { A(long a) { this((int) a) } }
             record O(int a, String b) { O(String b, int a) { this(a, b) } }
             record M(int a) { M(int other) { this.a = other } }
             record Y(Map<String, Integer> m) { Y(Map<String,Integer> m) { this.m = m } }
             record Q(java.util.List<String> l) { Q(List<String> l) { this.l = l } }
             record G(List</* c */ String> l) { G(List<String> l) { this.l = l } }
             record K(java.util.List<String> l) { K(java.util./* c */List<String> l) { this.l = l } }
             record H(int[] a) { H(int[/* c */] a) { this.a = a } }
             record P(int a) { P(int a, int b) { this(a) } }
             record S(int a, int b) { S(int a) { this(a, 0) } }",
            "types.groovy",
            |space| {
                let functions_per_record: Vec<(&str, usize)> = space
                    .spaces
                    .iter()
                    .map(|r| (r.name.as_deref().unwrap_or("?"), r.spaces.len()))
                    .collect();
                assert_eq!(
                    functions_per_record,
                    [
                        ("A", 2),
                        ("O", 2),
                        ("M", 1),
                        ("Y", 1),
                        ("Q", 2),
                        ("G", 1),
                        ("K", 1),
                        ("H", 1),
                        ("P", 2),
                        ("S", 2),
                    ]
                );
            },
        );
    }

    /// The canonical constructor is named after its record (JLS 8.8),
    /// and the gate requires it: `Q(int a, int b) { }` in `R`'s body is
    /// a `constructor_declaration` named `Q` — the grammar does not check
    /// a constructor's name — whose arity and types match `R`'s
    /// components exactly. Read by types alone it closed `R`'s header:
    /// `R::R` was missing and `Q` counted as `R`'s only callable. The
    /// header opens beside it: `R`'s two components and `Q`'s two
    /// parameters.
    ///
    /// Until dekobon-tree-sitter-groovy 0.5.0 the same shape was reached
    /// through a nested class, which the grammar recovered into `R`'s
    /// body as a stray constructor. A nested class is now a space of its
    /// own holding its constructor, and `R`'s header stays open: the
    /// second fixture.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_constructor_named_otherwise_does_not_close_the_record_header() {
        const NAMED: &str = "record R(int a, int b) { Q(int a, int b) { } }";
        const NESTED: &str = "record R(int a, int b) { class Inner { Inner(int a, int b) { } } }";
        for source in [NAMED, NESTED] {
            let parser = GroovyParser::new(
                source.as_bytes().to_vec(),
                std::path::Path::new("named.groovy"),
                None,
            );
            assert!(!parser.root().has_error(), "{source:?} parses clean");
        }
        check_spaces::<GroovyParser, _>(NAMED, "named.groovy", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 2),
                    row("Q", SpaceKind::Function, 2),
                ]
            );
            assert_eq!(space.metrics.nargs.function_args_sum(), 4);
        });
        check_spaces::<GroovyParser, _>(NESTED, "nested.groovy", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 2),
                    row("Inner", SpaceKind::Class, 0),
                    row("Inner", SpaceKind::Function, 2),
                ]
            );
            assert_eq!(space.metrics.nargs.function_args_sum(), 4);
        });
    }

    /// Java's twin of
    /// `groovy_constructor_named_otherwise_does_not_close_the_record_header`.
    /// Valid Java cannot reach the name test — javac names a nested
    /// class's constructor under that class, and rejects a constructor
    /// named otherwise ("invalid method declaration; return type
    /// required") — but tree-sitter-java parses `Q(int a, int b) { }`
    /// in `R`'s body as a `constructor_declaration` named `Q`, with no
    /// error node, so the rule is exercised on the same shape: `Q` is
    /// not `R`'s canonical constructor and the header opens beside it.
    #[cfg(feature = "java")]
    #[test]
    fn java_constructor_named_otherwise_does_not_close_the_record_header() {
        let source = "record R(int a, int b) { Q(int a, int b) { } }";
        let parser = JavaParser::new(
            source.as_bytes().to_vec(),
            std::path::Path::new("named.java"),
            None,
        );
        assert!(
            !parser.root().has_error(),
            "the grammar now rejects a constructor named otherwise; the pin is moot"
        );
        check_spaces::<JavaParser, _>(source, "named.java", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 2),
                    row("Q", SpaceKind::Function, 2),
                ]
            );
        });
    }

    /// The header's row is gated on the *bytes* of the body's
    /// constructor types, so a callable test handed none reads
    /// every same-arity constructor as canonical — `A(long a)` beside
    /// component `int a` closes the header under no bytes and leaves it
    /// open under the real ones. The space walk opens the header with
    /// the bytes; a `compute` whose callable test dropped them (the
    /// byte-less `is_func` #1863 removed) counted nothing on that space,
    /// and the header row read `nargs` 0 beside the alternative's 1.
    /// Both rows are 1:
    /// the header's one component and the alternative's one parameter.
    #[cfg(feature = "java")]
    #[test]
    fn java_header_beside_a_same_arity_alternative_counts_its_components() {
        check_spaces::<JavaParser, _>(
            "record A(int a) { A(long a) { this((int) a); } }",
            "alt.java",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("A", SpaceKind::Class, 0),
                        row("A", SpaceKind::Function, 1),
                        row("A", SpaceKind::Function, 1),
                    ]
                );
                assert_eq!(space.metrics.nargs.function_args_sum(), 2);
            },
        );
    }

    /// Groovy's twin of
    /// `java_header_beside_a_same_arity_alternative_counts_its_components`:
    /// the `record_components` row's gate compares type bytes, so a
    /// callable test handed none closes the header beside any
    /// same-arity constructor and the walk's space read 0.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_header_beside_a_same_arity_alternative_counts_its_components() {
        check_spaces::<GroovyParser, _>(
            "record A(int a) { A(long a) { this((int) a) } }",
            "alt.groovy",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("A", SpaceKind::Class, 0),
                        row("A", SpaceKind::Function, 1),
                        row("A", SpaceKind::Function, 1),
                    ]
                );
                assert_eq!(space.metrics.nargs.function_args_sum(), 2);
            },
        );
    }

    /// A header's space holds the component list and no code, so a
    /// component-less `record E() {}` has no Halstead volume and reads
    /// the `mi.*` `0.0` sentinel, which `bca check` and `bca report` skip
    /// (#1621), as Kotlin's `class Z()` does; a record with components
    /// has its list as its volume. `check_func_space` because `mi` pulls
    /// its own inputs.
    #[cfg(feature = "java")]
    #[test]
    fn java_componentless_record_header_is_mi_empty() {
        check_func_space::<JavaParser, _>(
            "record E() {}
record A(int x) {}",
            "e.java",
            |space| {
                let e = &child_space(&space, "E").spaces[0];
                let a = &child_space(&space, "A").spaces[0];
                assert_eq!((e.kind, a.kind), (SpaceKind::Function, SpaceKind::Function));
                assert!(e.metrics.mi.is_empty(), "no list, no volume");
                assert_eq!(e.metrics.mi.original(), 0.0);
                assert!(!a.metrics.mi.is_empty(), "the list is the space's code");
            },
        );
    }

    /// The header's space is one callable of weight 1 and no decisions:
    /// cyclomatic 1, cognitive 0, one `nom` function, one `wmc` method
    /// — and `npa` reads nothing from it, a component being no field
    /// declaration. The record's own `cyclomatic` stays its space's 1,
    /// so the constructor's entry is billed to the constructor and not
    /// to the class.
    #[cfg(feature = "java")]
    #[test]
    fn java_record_header_is_one_callable_with_no_decisions() {
        check_func_space::<JavaParser, _>("record R(int a, int b) { int f; }", "r.java", |space| {
            let record = child_space(&space, "R");
            let ctor = &record.spaces[0];
            assert_eq!(ctor.kind, SpaceKind::Function);
            assert_eq!(ctor.metrics.cyclomatic.cyclomatic(), 1);
            assert_eq!(ctor.metrics.cognitive.cognitive(), 0);
            assert_eq!(record.metrics.cyclomatic.cyclomatic(), 1);
            assert_eq!(record.metrics.nom.functions_sum(), 1);
            assert_eq!(record.metrics.wmc.class_wmc(), 1);
            assert_eq!(record.metrics.npa.class_na_sum(), 1, "`f` alone");
            assert_eq!(ctor.metrics.npa.class_na(), 0);
        });
    }

    /// Error recovery: `record R(int a, int b);` — a C#-style bodiless
    /// record — is not Java, and the grammar recovers it as an
    /// expression statement holding an `ERROR` with the list under it,
    /// not a `record_declaration`, so no space opens and the header
    /// gate, seeing no record parent, declines the list. Pinned so a
    /// grammar bump that starts recovering a `record_declaration` here
    /// changes this row rather than a corpus snapshot.
    #[cfg(feature = "java")]
    #[test]
    fn java_bodiless_record_opens_nothing() {
        check_spaces::<JavaParser, _>("record R(int a, int b);", "broken.java", |space| {
            assert_eq!(space_rows(&space), []);
            assert_eq!(space.metrics.nargs.total(), 0);
        });
    }

    /// #1842's Groovy reproducer: a record's `record_components` opens
    /// the implicit canonical constructor's space, named after the
    /// record, owning the components. Both rows read 0 on `main`.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_record_header_owns_its_components() {
        check_spaces::<GroovyParser, _>(
            "record R(int a, int b, int c, int d, int e, int f) {}",
            "r.groovy",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("R", SpaceKind::Class, 0),
                        row("R", SpaceKind::Function, 6),
                    ]
                );
                let ctor = &space.spaces[0].spaces[0];
                assert_eq!(ctor.metrics.nargs.own_args(), 6);
                assert_eq!((ctor.start_line, ctor.end_line), (1, 1));
                assert_eq!(space.metrics.nom.functions_sum(), 1);
                assert!((space.metrics.nargs.function_args_average() - 6.0).abs() < 1e-9);
            },
        );
    }

    /// Groovy's owner rule, Java's without the compact form (the pinned
    /// grammar cannot parse one — `groovy_compact_constructor_does_not_parse`):
    /// implicit (`I`), explicit canonical (`N`, closing the header),
    /// alternative only (`A`, a callable of its own beside the header),
    /// no body at all (`B`, Groovy allows it), no components (`E`), a
    /// generic and a qualified component type (`G`), a comment in the
    /// header, and a variable-arity component (`V`): one parameter,
    /// though the grammar strands its `...` in an `ERROR` inside the
    /// component. Sum: 2+2+(2+1)+1+0+2+2+1 = 13.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_record_components_have_exactly_one_owner() {
        check_spaces::<GroovyParser, _>(
            "record I(int a, int b) {}
             record N(int a, String b) {
                 N(int a, String b) { this.a = a; this.b = b }
             }
             record A(int a, int b) { A(int a) { this(a, 0) } }
             record B(int a)
             record E() {}
             record G(List<String> a, java.util.Map<String, Integer> b) {}
             record C(int a /* c */, int b) {}
             record V(String... xs) {}",
            "owners.groovy",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("I", SpaceKind::Class, 0),
                        row("I", SpaceKind::Function, 2),
                        row("N", SpaceKind::Class, 0),
                        row("N", SpaceKind::Function, 2),
                        row("A", SpaceKind::Class, 0),
                        row("A", SpaceKind::Function, 2),
                        row("A", SpaceKind::Function, 1),
                        row("B", SpaceKind::Class, 0),
                        row("B", SpaceKind::Function, 1),
                        row("E", SpaceKind::Class, 0),
                        row("E", SpaceKind::Function, 0),
                        row("G", SpaceKind::Class, 0),
                        row("G", SpaceKind::Function, 2),
                        row("C", SpaceKind::Class, 0),
                        row("C", SpaceKind::Function, 2),
                        row("V", SpaceKind::Class, 0),
                        row("V", SpaceKind::Function, 1),
                    ]
                );
                assert_eq!(space.metrics.nargs.function_args_sum(), 13);
                assert_eq!(space.metrics.nom.functions_sum(), 9);
                // `N`'s one row is the body's constructor, not the
                // header: the constructor sits on the line after the
                // header's, which a header row would start on.
                assert_eq!(child_space(&space, "N").spaces[0].start_line, 3);
            },
        );
    }

    /// The pinned dekobon grammar does not parse a compact constructor.
    /// In `record R(int a) { R { assert a > 0 } }` the bare `R` is an
    /// `ERROR` and the `{ … }` after it an instance initializer `block`
    /// of the record's body, so no constructor is written and the header
    /// opens: one owner, the right count — but a limitation, not
    /// support, since the compact body is no function of its own.
    /// Before 0.5.0 the `record_declaration` ended at the header's `)`
    /// and every member after the compact body was read at file level;
    /// the body is intact now and `m` stays in the record. Spelled
    /// `public R { … }` the name is a typeless field and nothing errors,
    /// which `groovy_compact_constructor_with_a_modifier_reads_as_a_public_field`
    /// (`npa.rs`) pins; the header opens the same way. Recorded so a
    /// grammar bump that starts parsing the form
    /// (tree-sitter-groovy#72) moves this row and the book's limitation
    /// note together, and so no one reads the header-only pass above as
    /// compact support.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_compact_constructor_does_not_parse() {
        let source = "record R(int a, int b) { R { assert a > 0 } }";
        let parser = GroovyParser::new(
            source.as_bytes().to_vec(),
            std::path::Path::new("compact.groovy"),
            None,
        );
        assert!(
            parser.root().has_error(),
            "the grammar learned the compact form: revisit the owner rule and the book"
        );
        check_spaces::<GroovyParser, _>(source, "compact.groovy", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 2),
                ]
            );
        });
        // A member after the compact body is the record's: `m` is `R`'s
        // child, and `R` spans both lines.
        check_spaces::<GroovyParser, _>(
            "record R(int a, int b) { R { assert a > 0 }\n  int m(int t) { t } }",
            "compact.groovy",
            |space| {
                assert_eq!(
                    space_rows(&space),
                    [
                        row("R", SpaceKind::Class, 0),
                        row("R", SpaceKind::Function, 2),
                        row("m", SpaceKind::Function, 1),
                    ]
                );
                let names: Vec<_> = space.spaces.iter().map(|s| s.name.as_deref()).collect();
                assert_eq!(names, [Some("R")]);
                assert_eq!(space.spaces[0].end_line, 2);
            },
        );
    }

    /// The pinned grammar requires a type on a `record_component`, so
    /// Groovy's untyped `record R(a, b)` recovers as *one* component —
    /// `type_identifier: a`, an `ERROR` holding the comma, `identifier:
    /// b` — and the header counts 1 where the language declares 2. A
    /// written canonical constructor `R(a, b)` has two untyped
    /// `formal_parameter`s, which the one-component header never
    /// matches, so the header opens beside it: a limitation, recorded
    /// rather than inferred over the `ERROR`, so a grammar bump that
    /// parses the form moves this row and the book's note together.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_untyped_record_components_recover_as_one() {
        let source = "record R(a, b) { R(a, b) { this.a = a; this.b = b } }";
        let parser = GroovyParser::new(
            source.as_bytes().to_vec(),
            std::path::Path::new("untyped.groovy"),
            None,
        );
        assert!(
            parser.root().has_error(),
            "the grammar learned untyped components: revisit the owner rule and the book"
        );
        check_spaces::<GroovyParser, _>(source, "untyped.groovy", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 1),
                    row("R", SpaceKind::Function, 2),
                ]
            );
            assert_eq!(space.metrics.nom.functions_sum(), 2);
        });
    }

    /// A header's arguments are its `record_component` children, counted
    /// by kind: the `ERROR` recovery leaves in the list beside a
    /// component — here for a trailing comma — is none, so the header
    /// reads 1. `count_args`, which counts every other callable's list,
    /// takes each child that is no parenthesis, comma or comment and
    /// reads 2 on this list. The two agree on every clean parse, which
    /// is why #1863 proposed swapping one for the other; the swap would
    /// move this row. The second assertion keeps that claim honest: a
    /// `count_args` that learns to skip an `ERROR` fails it, and the
    /// header arm can then share the one definition.
    #[cfg(feature = "groovy")]
    #[test]
    fn groovy_header_counts_components_and_no_recovery_error() {
        use crate::languages::language_groovy::Groovy;
        let source = "record R(int a,) { }";
        let parser = GroovyParser::new(
            source.as_bytes().to_vec(),
            std::path::Path::new("trailing.groovy"),
            None,
        );
        let headers: Vec<_> = parser
            .root()
            .preorder()
            .filter(|node| node.kind_id() == Groovy::RecordComponents)
            .map(|header| {
                (
                    header
                        .children()
                        .map(|child| child.kind().to_owned())
                        .collect::<Vec<_>>(),
                    count_args::<GroovyCode>(&header, source.as_bytes()),
                )
            })
            .collect();
        assert_eq!(
            headers,
            [(
                ["(", "record_component", "ERROR", ")"]
                    .map(str::to_owned)
                    .to_vec(),
                2
            )]
        );
        check_spaces::<GroovyParser, _>(source, "trailing.groovy", |space| {
            assert_eq!(
                space_rows(&space),
                [
                    row("R", SpaceKind::Class, 0),
                    row("R", SpaceKind::Function, 1),
                ]
            );
        });
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_no_functions_and_closures() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            insta::assert_json_snapshot!(
                metric.nargs,
                @r#"
            {
              "function_args": 0,
              "closure_args": 0,
              "function_args_average": 0.0,
              "closure_args_average": 0.0,
              "total": 0,
              "value": 0,
              "average": 0.0,
              "function_args_min": 0,
              "function_args_max": 0,
              "closure_args_min": 0,
              "closure_args_max": 0
            }
            "#
            );
        });
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_single_function() {
        // Two parameters in a regular function.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, int $b): bool {
                if ($a) { return $a; }
                return false;
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_single_closure() {
        // Anonymous function with 2 params + arrow function with 1 param.
        // Each is a separate closure space.
        check_metrics::<PhpParser>(
            "<?php
            $f = function (int $a, int $b) { return $a + $b; };
            $g = fn (int $x) => $x * 2;",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 0,
                  "closure_args": 3,
                  "function_args_average": 0.0,
                  "closure_args_average": 1.5,
                  "total": 3,
                  "value": 0,
                  "average": 1.5,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_functions() {
        // Two top-level functions, 1 + 2 args.
        check_metrics::<PhpParser>(
            "<?php
            function a(int $x): int { return $x; }
            function b(int $x, int $y): int { return $x + $y; }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 1.5,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 1.5,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#
                );
            },
        );
    }

    #[cfg(feature = "php")]
    #[test]
    fn php_nested_functions() {
        // PHP cannot define nested named functions inside a function body
        // syntactically, but a class with methods exhibits the same shape:
        // a top-level scope plus inner function-spaces.
        check_metrics::<PhpParser>(
            "<?php
            class A {
                public function outer(int $a): int {
                    $f = function (int $b) use ($a) { return $a + $b; };
                    return $f($a);
                }
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.nargs,
                    @r#"
                {
                  "function_args": 1,
                  "closure_args": 1,
                  "function_args_average": 1.0,
                  "closure_args_average": 1.0,
                  "total": 2,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 1,
                  "closure_args_min": 0,
                  "closure_args_max": 1
                }
                "#
                );
            },
        );
    }

    /// Regression for #1142: the parameter list sits two `arguments`
    /// levels down, so the `parameters`-field heuristic found nothing and
    /// every Elixir function reported 0.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_named_function_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def bar(a, b, c) do\n    a + b + c\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 3);
            },
        );
    }

    /// A guard interposes a `when` `binary_operator` between the macro's
    /// `arguments` and the head `Call`. Without unwrapping it every
    /// guarded clause — a large fraction of real Elixir — counts 0.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_guarded_clause_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defp baz(x) when is_integer(x), do: x\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 1);
                assert_eq!(s.function_args_max(), 1);
            },
        );
    }

    /// `def noargs, do: 1` puts a bare `identifier` where the head `Call`
    /// would be. It has no parameter list, and the walk must stop there
    /// rather than fall through to the enclosing `arguments` — which
    /// holds the `do:` keyword pair and would count 1.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_zero_arg_function_has_no_parameter_list() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def noargs, do: 1\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                assert_eq!(metric.nargs.function_args_sum(), 0);
            },
        );
    }

    /// Pattern and defaulted parameters are `map` and `binary_operator`
    /// nodes rather than plain identifiers, so the punctuation-negative
    /// filter is what keeps them counted.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_pattern_and_default_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(%{a: x}, b \\\\ 1), do: {x, b}\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 2);
                assert_eq!(s.function_args_max(), 2);
            },
        );
    }

    /// Every clause of one `fn` has the same arity, so a two-clause
    /// two-argument closure is 2 — summing the clauses would report 4.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_multi_clause_closure_counts_one_clause() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def run do\n    fn\n      a, b -> a + b\n      a, _ -> a\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.closures_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.closure_args_sum(), 2);
                assert_eq!(s.closure_args_max(), 2);
            },
        );
    }

    /// A guarded `fn` clause aliases its `left` to the same `when`
    /// `binary_operator` a guarded `def` head uses, so it needs the same
    /// unwrap. Without it the count is the guard expression's fixed three
    /// children — 3 for any arity, which is why the four-parameter form is
    /// the fixture here.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_guarded_closure_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def run do\n    fn a, b, c, d when is_integer(a) -> a + b + c + d end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.closures_sum(), 1);
                let s = &metric.nargs;
                assert_eq!(s.closure_args_sum(), 4);
                assert_eq!(s.closure_args_max(), 4);
            },
        );
    }

    /// `def a + b` and `def -a` define the operator functions `+/2` and
    /// `-/1`. Their head is the operator node itself, with no `arguments`
    /// container to walk, so the arity comes from the operator's shape.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_operator_definition_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def a + b, do: {a, b}\n  def -a, do: a\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nom.functions_sum(), 2);
                let s = &metric.nargs;
                assert_eq!(s.function_args_sum(), 3);
                assert_eq!(s.function_args_max(), 2);
            },
        );
    }

    /// A `def` inside `quote do … end` is a code template, not a
    /// declaration, and must not contribute arguments (#310). The quoted
    /// head carries three parameters, so dropping the rule reads 3.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_quoted_def_contributes_no_args() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defmacro mac do\n    quote do\n      def generated(p, q, r), do: p + q + r\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 0);
                // Anchor the zero: if Elixir def-recognition broke
                // entirely, `function_args_sum` would also read 0. The
                // enclosing `defmacro mac` still counting proves the
                // recognizer ran and the quote-block gate did the
                // excluding.
                assert_eq!(metric.nom.functions_sum(), 1);
            },
        );
    }

    /// Only `def` / `defp` / `defmacro` / `defmacrop` declare a function.
    /// `defmodule` and `defdelegate` are ordinary `Call`s of the same
    /// shape — `defdelegate log(msg), to: Logger` has a head `Call` with
    /// one parameter, so a gate that matched any macro would read 1.
    #[cfg(feature = "elixir")]
    #[test]
    fn elixir_non_method_macros_count_zero() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  defdelegate log(msg), to: Logger\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 0);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_no_functions_and_closures() {
        check_metrics::<RubyParser>("a = 42\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_single_function() {
        // Single method with 3 parameters.
        check_metrics::<RubyParser>("def foo(a, b, c)\n  a + b + c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 3);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_single_closure() {
        // A bare block `[1,2,3].each { |x| ... }` is the only closure
        // here; `each` is a method call so the method-args count is 0.
        check_metrics::<RubyParser>("[1, 2, 3].each { |x| puts x }\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 1);
        });
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_functions() {
        // Two methods, args=2 and args=1; one lambda with args=2.
        check_metrics::<RubyParser>(
            "def add(a, b)\n  a + b\nend\n\ndef neg(x)\n  -x\nend\n\nf = ->(a, b) { a * b }\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 3);
                assert_eq!(metric.nargs.closure_args_sum(), 2);
            },
        );
    }

    #[cfg(feature = "ruby")]
    #[test]
    fn ruby_nested_functions() {
        // An outer method with 1 arg containing an inner method with 2.
        check_metrics::<RubyParser>(
            "def outer(a)\n  def inner(b, c)\n    b + c\n  end\n  inner(a, a)\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 3);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
            },
        );
    }

    /// PEP 570 positional-only `/` and PEP 3102 keyword-only `*` markers are
    /// punctuation, not parameters. The grammar emits them as
    /// `positional_separator` / `keyword_separator` siblings of the real
    /// parameter nodes; both must be excluded from nargs (issue #414).
    #[cfg(feature = "python")]
    #[test]
    fn python_both_parameter_separators() {
        // 1 function, 3 real parameters: pos_only, normal, kw_only.
        check_metrics::<PythonParser>(
            "def f(pos_only, /, normal, *, kw_only): pass",
            "foo.py",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 3);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
            },
        );
    }

    /// Trailing positional-only `/` (no following parameter) is still excluded.
    #[cfg(feature = "python")]
    #[test]
    fn python_positional_separator_only() {
        // 1 function, 2 real parameters: a, b (`/` excluded).
        check_metrics::<PythonParser>("def f(a, b, /): pass", "foo.py", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// Leading keyword-only `*` (forcing all following parameters to be
    /// keyword-only) is excluded.
    #[cfg(feature = "python")]
    #[test]
    fn python_keyword_separator_only() {
        // 1 function, 2 real parameters: a, b (`*` excluded).
        check_metrics::<PythonParser>("def f(*, a, b): pass", "foo.py", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// Lambdas accept the same keyword-only `*` separator; it is excluded
    /// from the closure arg count.
    #[cfg(feature = "python")]
    #[test]
    fn python_lambda_keyword_separator() {
        // 1 lambda, 2 real parameters: a, b (`*` excluded).
        check_metrics::<PythonParser>("g = lambda a, *, b: a", "foo.py", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 2);
        });
    }

    /// Regression guard: `*args` / `**kwargs` are real parameter nodes
    /// (`list_splat_pattern` / `dictionary_splat_pattern`), not separators,
    /// and must keep contributing to the count after the #414 fix.
    #[cfg(feature = "python")]
    #[test]
    fn python_args_kwargs_still_counted() {
        // 1 function, 3 parameters: a, *args, **kwargs.
        check_metrics::<PythonParser>("def f(a, *args, **kwargs): pass", "foo.py", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 3);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// A file of bare top-level commands has no function spaces, so the
    /// argument count is zero.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_no_functions_and_closures() {
        check_metrics::<IrulesParser>("set x 1\nlog local0. $x\n", "foo.irule", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// A `when` handler is a function space but has no formal parameters
    /// (the event context is implicit), so its argument count is zero —
    /// `when_event` carries no `arguments` field. Guards edge case #10.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_handler_zero_args() {
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST { log local0. \"hit\" }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 0);
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                // The handler is still counted as a function space.
                assert_eq!(metric.nom.functions_sum(), 1);
            },
        );
    }

    /// A `proc` with two formal parameters contributes two arguments.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_single_proc() {
        check_metrics::<IrulesParser>("proc f { a b } { return $a }\n", "foo.irule", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// A `proc` with an empty argument list contributes zero arguments.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_proc_no_args() {
        check_metrics::<IrulesParser>("proc f { } { return 1 }\n", "foo.irule", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
        });
    }

    /// A default-valued parameter (`{b 5}`) is a single `argument`, so each
    /// formal parameter counts once regardless of its default: `{a {b 5} c}`
    /// is three arguments.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_proc_arg_defaults() {
        check_metrics::<IrulesParser>(
            "proc f { a {b 5} c } { return $a }\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 3);
            },
        );
    }

    /// A `proc` and a `when` handler in one file: only the proc's two
    /// parameters count; the handler contributes zero.
    #[cfg(feature = "irules")]
    #[test]
    fn irules_multiple_functions() {
        check_metrics::<IrulesParser>(
            "proc add { a b } { return [expr { $a + $b }] }
when HTTP_REQUEST { log local0. \"hit\" }
",
            "foo.irule",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 2);
                assert_eq!(metric.nom.functions_sum(), 2);
            },
        );
    }

    /// Objective-C unary method `- (void)foo` declares zero arguments.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_no_args() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)foo {
    [self doWork];
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.total(), 0);
                insta::assert_json_snapshot!(metric.nargs, @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C keyword method `- (void)foo:(int)a bar:(int)b` has two
    /// `method_parameter` children, so `function_args` is 2.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_method_two_args() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)foo:(int)a bar:(int)b {
    [self use:a];
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 2);
                insta::assert_json_snapshot!(metric.nargs, @r#"
                {
                  "function_args": 2,
                  "closure_args": 0,
                  "function_args_average": 2.0,
                  "closure_args_average": 0.0,
                  "total": 2,
                  "value": 0,
                  "average": 2.0,
                  "function_args_min": 0,
                  "function_args_max": 2,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#);
            },
        );
    }

    /// Free C `function_definition` inside an ObjC translation unit counts
    /// its declarator parameters: `void f(int a, int b, int c)` has 3.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_function_args() {
        check_metrics::<ObjcParser>(
            "void f(int a, int b, int c) {
    return;
}
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.function_args_sum(), 3);
                insta::assert_json_snapshot!(metric.nargs, @r#"
                {
                  "function_args": 3,
                  "closure_args": 0,
                  "function_args_average": 3.0,
                  "closure_args_average": 0.0,
                  "total": 3,
                  "value": 0,
                  "average": 3.0,
                  "function_args_min": 0,
                  "function_args_max": 3,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#);
            },
        );
    }

    /// Objective-C block literal `^(int x, int y){ … }` is a closure
    /// whose `parameter_list` holds two `parameter_declaration`s, so
    /// `closure_args` is 2.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_block_args() {
        check_metrics::<ObjcParser>(
            "@implementation Foo
- (void)bar {
    void (^blk)(int, int) = ^(int x, int y){
        [self use:x];
    };
    blk(1, 2);
}
@end
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                insta::assert_json_snapshot!(metric.nargs, @r#"
                {
                  "function_args": 0,
                  "closure_args": 2,
                  "function_args_average": 0.0,
                  "closure_args_average": 2.0,
                  "total": 2,
                  "value": 0,
                  "average": 1.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 2
                }
                "#);
            },
        );
    }

    /// A block's `(void)` marker declares nothing, so `^(void){ … }` is a
    /// closure of zero parameters (#1218).
    ///
    /// The objc grammar reuses C's `parameter_list` rule, so `^(void)`
    /// emits a real `parameter_declaration` for the `void` — the same
    /// shape `int f(void)` produces, and the reason
    /// `Checker::is_empty_param_marker` reads the source bytes rather
    /// than the tree. The block arm counted it until it began routing
    /// through `count_args`, while the function channel beside it was
    /// already correct: `host` below reports 0 either way, which is what
    /// makes this a test of the block channel specifically.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_block_void_marker_is_not_a_parameter() {
        check_metrics::<ObjcParser>(
            "void host(void) {
    void (^empty)(void) = ^(void){ };
    empty();
}
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                assert_eq!(metric.nargs.function_args_sum(), 0);
                insta::assert_json_snapshot!(metric.nargs, @r#"
                {
                  "function_args": 0,
                  "closure_args": 0,
                  "function_args_average": 0.0,
                  "closure_args_average": 0.0,
                  "total": 0,
                  "value": 0,
                  "average": 0.0,
                  "function_args_min": 0,
                  "function_args_max": 0,
                  "closure_args_min": 0,
                  "closure_args_max": 0
                }
                "#);
            },
        );
    }

    /// A comment inside a block's parameter list is not a parameter, so
    /// `^(int a /* c */, int b){ … }` is 2 (#1201, #1218).
    ///
    /// **This fixture cannot fail by reverting the block arm.** The
    /// positive `matches!(ParameterDeclaration | VariadicParameter)` the
    /// arm used before #1218 already ignored a `comment` child, so the
    /// count was correct for the wrong reason — nothing asserted it, and
    /// the #1201 changelog claimed Objective-C blocks were swept when
    /// only the method fixture existed. It became load-bearing when the
    /// arm switched to `count_args`, whose *negative* filtering is what
    /// now makes `Checker::is_comment` live on this path. Perturb it by
    /// dropping `is_comment` from `count_args`, not by reverting the arm.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_block_comment_is_not_a_parameter() {
        check_metrics::<ObjcParser>(
            "void host(void) {
    void (^two)(int, int) = ^(int a /* c */, int b){ };
    two(1, 2);
}
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                assert_eq!(metric.nargs.function_args_sum(), 0);
            },
        );
    }

    /// A variadic block keeps its `...` counted: `^(int a, ...){ … }` is 2.
    ///
    /// The guard against #1218's fix, not against #1218. Swapping the
    /// arm's positive `matches!` for `count_args`' negative filters is
    /// what could silently drop `variadic_parameter` — it is named in the
    /// old match and in none of the new filters, so only a fixture says
    /// whether it survived. `ObjcCode::is_non_arg` covers the list's
    /// punctuation (`(`, `,`, `)`) and nothing else, so it does.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_block_variadic_parameter_still_counts() {
        check_metrics::<ObjcParser>(
            "void host(void) {
    void (^var)(int, ...) = ^(int a, ...){ };
    var(1);
}
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 2);
                assert_eq!(metric.nargs.function_args_sum(), 0);
            },
        );
    }

    /// A block written without a parameter list at all is 0.
    ///
    /// `^{ }` has no `parameter_list` child, so the arm's
    /// `first_child(ParameterList)` guard short-circuits before
    /// `count_args` is reached. Pinned beside the `^(void)` case because
    /// the two spellings mean the same thing and only one of them ever
    /// went through the counting path.
    #[cfg(feature = "objc")]
    #[test]
    fn objc_block_without_a_parameter_list_is_zero() {
        check_metrics::<ObjcParser>(
            "void host(void) {
    void (^none)(void) = ^{ };
    none();
}
",
            "foo.m",
            |metric| {
                assert_eq!(metric.nargs.closure_args_sum(), 0);
                assert_eq!(metric.nargs.function_args_sum(), 0);
            },
        );
    }

    /// Regression for #782: the textual `Display` headline must report
    /// the cross-space *sum* (`function_args_sum`/`closure_args_sum`),
    /// matching the JSON/YAML/TOML/CBOR serializers, not the per-space
    /// direct accumulator. At a parent space that rolls up child
    /// function-spaces (the file/unit space of `python_nested_functions`)
    /// the accumulator under-counts: it reflects only the direct
    /// function `f` (2 args) and no merged closures (0), while the sum
    /// is 3 function args (f=2, foo=1) and 2 closure args. Before the
    /// fix Display printed `function_args: 2, closure_args: 0`.
    #[cfg(feature = "python")]
    #[test]
    fn display_headline_matches_sum_for_nested_functions() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 def foo(a):
                     if a:
                         return 1
                 bar = lambda a: lambda b: b or True or True
                 return bar(foo(a))(a)",
            "foo.py",
            |metric| {
                let stats = &metric.nargs;
                // The summed accessors are the cross-format source of truth.
                assert_eq!(stats.function_args_sum(), 3);
                assert_eq!(stats.closure_args_sum(), 2);

                // The Display headline must echo those sums verbatim.
                let rendered = stats.to_string();
                assert!(
                    rendered.starts_with(&format!(
                        "function_args: {}, closure_args: {},",
                        stats.function_args_sum(),
                        stats.closure_args_sum()
                    )),
                    "Display headline diverged from the summed accessors: {rendered}"
                );
            },
        );
    }
}

/// A lambda's parameter count must not depend on optional parentheses
/// (#1185).
///
/// `x -> x + 1` and `(x) -> x + 1` are the same lambda — the parens are
/// optional in the grammar and carry no meaning — so they must score
/// alike, the same "byte-equivalent constructs score identically"
/// contract the book states for cognitive.
///
/// The cause is shared: the `parameters` field holds a lone, childless
/// parameter node rather than a list, and `compute_args` walks the
/// field's children. The issue names Java; the sweep found **C#** has
/// the identical defect via `implicit_parameter`. Kotlin and Groovy
/// were checked and are correct — each overrides `compute` with its own
/// closure-parameter shape — and the JS family reaches the right answer
/// through the singular `parameter` field.
// Gated on the union of the languages `cases()` has a row for, so a
// build enabling none of them drops the module rather than tripping its
// `checked > 0` guard (`docs/development/testing.md`, #1286). The
// closure-channel test below reads a strict subset of that table and so
// carries its own narrower gate.
#[cfg(test)]
#[cfg(any(
    feature = "csharp",
    feature = "java",
    feature = "javascript",
    feature = "mozjs",
    feature = "typescript"
))]
mod lambda_parenthesisation_parity {
    use crate::test_support::metrics_verbatim;
    use crate::{LANG, MetricsOptions};

    /// `(closure_args, function_args)` — the split matters as much as
    /// the count: a lambda must stay in the closure channel.
    fn args(lang: LANG, source: &str) -> (u64, u64) {
        let m = metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default());
        (m.nargs.closure_args_sum(), m.nargs.function_args_sum())
    }

    /// `(bare, parenthesised, two_params, zero_params)`.
    fn cases(lang: LANG) -> Option<[&'static str; 4]> {
        Some(match lang {
            LANG::Java => [
                "class K{ void f(){ Function<Integer,Integer> a = x -> x + 1; } }",
                "class K{ void f(){ Function<Integer,Integer> a = (x) -> x + 1; } }",
                "class K{ void f(){ BiFunction<Integer,Integer,Integer> c = (x, y) -> x + y; } }",
                "class K{ void f(){ Supplier<Integer> d = () -> 1; } }",
            ],
            LANG::Csharp => [
                "class K{ void f(){ Func<int,int> a = x => x + 1; } }",
                "class K{ void f(){ Func<int,int> a = (x) => x + 1; } }",
                "class K{ void f(){ Func<int,int,int> c = (x, y) => x + y; } }",
                "class K{ void f(){ Func<int> d = () => 1; } }",
            ],
            LANG::Javascript | LANG::Typescript | LANG::Tsx | LANG::Mozjs => [
                "function f(){ var a = x => x + 1; }",
                "function f(){ var a = (x) => x + 1; }",
                "function f(){ var c = (x, y) => x + y; }",
                "function f(){ var d = () => 1; }",
            ],
            _ => return None,
        })
    }

    #[test]
    fn optional_parentheses_do_not_change_the_count() {
        let mut checked = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            let Some([bare, paren, two, zero]) = cases(lang) else {
                continue;
            };
            checked += 1;

            let (bare_args, paren_args) = (args(lang, bare), args(lang, paren));
            assert_eq!(
                bare_args, paren_args,
                "{lang:?}: the parentheses changed the argument count\n  bare:  {bare}\n  paren: {paren}"
            );
            // The absolute value, so a regression that zeroed *both*
            // spellings would still fail.
            assert_eq!(
                bare_args.0 + bare_args.1,
                1,
                "{lang:?}: a one-parameter lambda must report one argument"
            );
            // A zero-parameter lambda must stay 0: the bare-parameter
            // branch must not mistake an empty list for a parameter.
            assert_eq!(
                args(lang, zero),
                (0, 0),
                "{lang:?}: `() -> …` has no arguments"
            );
            // And the plural path must be undisturbed.
            let two_args = args(lang, two);
            assert_eq!(
                two_args.0 + two_args.1,
                2,
                "{lang:?}: a two-parameter lambda must report two arguments"
            );
        }
        assert!(
            checked > 0,
            "no lambda language enabled; this test asserted nothing"
        );
    }

    /// The lambda stays in the *closure* channel, not the function one.
    ///
    /// Java and C# route it through `is_closure`; the JS family's arrow
    /// is classified by the `FuncOpener` row's `bound_arrow` gate and
    /// lands in `fn_args` when bound to a variable, which is a separate
    /// question (#1188).
    /// Asserting the channel per language rather than globally keeps
    /// this test from encoding that as a bug.
    #[test]
    #[cfg(any(feature = "csharp", feature = "java"))]
    fn a_bare_lambda_stays_in_the_closure_channel() {
        let mut checked = 0;
        for lang in [LANG::Java, LANG::Csharp]
            .into_iter()
            .filter(LANG::is_enabled)
        {
            checked += 1;
            let [bare, ..] = cases(lang).expect("both languages have cases");
            assert_eq!(
                args(lang, bare),
                (1, 0),
                "{lang:?}: the lambda's argument must be billed to closure_args"
            );
        }
        // Two rows against the module's five-feature gate, so this test
        // needs the narrower `cfg` above; the guard then covers the
        // residual `is_enabled` disagreement (#1286).
        assert!(
            checked > 0,
            "neither java nor csharp is enabled; this test asserted nothing"
        );
    }
}

/// #1201 — a comment inside a parameter list counted as a parameter.
///
/// tree-sitter attaches a comment written between two parameters as a
/// direct child of the parameter-*list* node, not inside the parameter
/// it documents. Every negative filter in this module lists punctuation
/// only, so each comment scored one: `int h(int a /* one */, int b)`
/// reported 3, and the C++ idiom for a deliberately unused parameter,
/// `void f(int /*unused*/)`, reported 2.
///
/// Four independent loops could carry the defect and each has its own
/// row below, so reverting any single one of them fails on its own:
/// `compute_args` (the C family through Groovy), `elixir_declared_args`,
/// `compute_kotlin_lambda_args`, and `compute_perl_args` — the last of
/// which already excluded comments and is here as a no-change guard on
/// its collapse onto the shared `count_args`.
///
/// Go, Lua, Objective-C *methods*, Kotlin *functions* and Groovy
/// *closures* were already correct — each filters positively for its
/// parameter kind — and are swept anyway, because "this one is a
/// positive filter" is the reasoning that has to hold for a grammar
/// bump, not just for today.
///
/// Objective-C *blocks* were on that list until #1218 and are not any
/// more: their arm now routes through the shared `count_args`, so what
/// keeps a comment out of a block's count is the same negative filter
/// the repaired languages rely on, not a positive parameter-kind match.
/// Their fixture is `objc_block_comment_is_not_a_parameter`, which sits
/// in the module above beside the `^(void)` case that motivated the
/// move rather than in the table below.
#[cfg(test)]
mod comments_in_parameter_lists {
    use crate::test_support::metrics_verbatim;
    use crate::{LANG, MetricsOptions};

    /// `(closure_args, function_args)`. Asserting the pair rather than
    /// the sum keeps a fix that merely moved a count between channels
    /// from reading as a pass.
    fn args(lang: LANG, source: &str) -> (u64, u64) {
        let m = metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default());
        (m.nargs.closure_args_sum(), m.nargs.function_args_sum())
    }

    /// Fixtures for the languages #1201 repaired. Every one of these
    /// reported an inflated count before the fix.
    ///
    /// Each fixture declares exactly two parameters — bar the C-family
    /// unnamed-parameter shape, which declares one — so the expected
    /// value is a 2 in one channel or the other. Where a language spells
    /// both a block and a line comment, both appear: they are separate
    /// `is_comment` arms in Rust, Java, C#, Kotlin, Groovy, PHP and the
    /// JS family, and a block-only sweep leaves the other arm untested.
    fn repaired_cases(lang: LANG) -> Option<&'static [(&'static str, (u64, u64))]> {
        Some(match lang {
            LANG::C => &[
                (
                    "int h(int a /* one */, int b /* two */) { return a; }",
                    (0, 2),
                ),
                ("int h(int a, // one\n      int b) { return a; }", (0, 2)),
                // The unnamed-parameter idiom: the comment is the only
                // thing standing where the name would be, so counting it
                // doubled the arity rather than adding to it.
                ("void f(int /*unused*/) { }", (0, 1)),
            ],
            // Split from `C` only for the lambda, which C has no form of.
            LANG::Cpp | LANG::Mozcpp => &[
                (
                    "int h(int a /* one */, int b /* two */) { return a; }",
                    (0, 2),
                ),
                ("int h(int a, // one\n      int b) { return a; }", (0, 2)),
                ("void f(int /*unused*/) { }", (0, 1)),
                // The closure channel, which reaches `compute_args`
                // through the same `declarator` field as the function
                // one but bills a different counter.
                (
                    "int g() { auto f = [](int a, /* one */ int b){ return a + b; }; return f(1,2); }",
                    (2, 0),
                ),
            ],
            LANG::Objc => &[("int h(int a /* one */, int b) { return a; }", (0, 2))],
            LANG::Javascript | LANG::Mozjs => &[
                ("function h(a, /* one */ b) { return a; }", (0, 2)),
                ("function h(a, // one\n           b) { return a; }", (0, 2)),
            ],
            LANG::Typescript | LANG::Tsx => &[
                (
                    "function h(a: number, /* one */ b: number) { return a; }",
                    (0, 2),
                ),
                (
                    "function h(a: number, // one\n           b: number) { return a; }",
                    (0, 2),
                ),
            ],
            // Python has no block comment, so the `#` form is the whole
            // of its exposure.
            LANG::Python => &[("def h(a,  # one\n      b):\n    return a\n", (0, 2))],
            LANG::Rust => &[
                ("fn h(a: i32, /* one */ b: i32) -> i32 { a }", (0, 2)),
                ("fn h(a: i32, // one\n     b: i32) -> i32 { a }", (0, 2)),
                // `compute_args` is reached a second time for closures,
                // with `closure_nargs` as the target. Same walk, but a
                // fixture that only ever asserts the function channel
                // cannot tell a regression at that call site apart from
                // a pass.
                (
                    "fn g() { let f = |a: i32, /* one */ b: i32| a + b; }",
                    (2, 0),
                ),
            ],
            // One source parses as both, so they share an arm rather
            // than tripping `clippy::match_same_arms` on two copies.
            LANG::Java | LANG::Csharp => &[
                (
                    "class K { int h(int a, /* one */ int b) { return a; } }",
                    (0, 2),
                ),
                (
                    "class K { int h(int a, // one\n                int b) { return a; } }",
                    (0, 2),
                ),
            ],
            LANG::Php => &[
                ("<?php function h($a, /* one */ $b) { return $a; }", (0, 2)),
                (
                    "<?php function h($a, // one\n                 $b) { return $a; }",
                    (0, 2),
                ),
            ],
            LANG::Ruby => &[("def h(a, # one\n      b)\n  a\nend", (0, 2))],
            LANG::Groovy => &[
                ("def h(a, /* one */ b) { return a }", (0, 2)),
                ("def h(a, // one\n      b) { return a }", (0, 2)),
            ],
            // Elixir's parameter list is a second `Call`'s `arguments`,
            // reached without ever entering `compute_args`, so nothing
            // above proves anything about it.
            LANG::Elixir => &[(
                "defmodule M do\n  def h(a, # one\n        b) do\n    a\n  end\nend",
                (0, 2),
            )],
            // `compute_kotlin_lambda_args` is a separate loop with its own
            // negative filter, which the Kotlin *function* guard below
            // never reaches.
            LANG::Kotlin => &[
                (
                    "fun g() {\n  val f = { a: Int, /* one */ b: Int -> a }\n  println(f)\n}",
                    (2, 0),
                ),
                (
                    "fun g() {\n  val f = { a: Int, // one\n            b: Int -> a }\n  println(f)\n}",
                    (2, 0),
                ),
            ],
            // Bash functions take no formal parameters, and Tcl/iRules
            // have no comment in this position at all — see
            // `tcl_has_no_comment_inside_a_parameter_list`.
            _ => return None,
        })
    }

    /// Fixtures for the counting loops that were already correct before
    /// #1201, each because it filters *positively* for its parameter
    /// kind and so never saw a comment to miscount.
    ///
    /// They are swept anyway: "this one is a positive filter" is a claim
    /// that has to keep holding across a grammar bump, not just today,
    /// and Perl's is the guard on collapsing its private comment
    /// exclusion onto the shared `count_args`.
    fn already_correct_cases(lang: LANG) -> Option<&'static [(&'static str, (u64, u64))]> {
        Some(match lang {
            // One `method_parameter` per labelled argument.
            LANG::Objc => &[(
                "@implementation K\n- (void)foo:(int)a /* one */ bar:(int)b { }\n@end",
                (0, 2),
            )],
            // A positive `ClosureParameter` filter.
            LANG::Groovy => &[("def c = { x, /* one */ y -> x }", (2, 0))],
            // A positive `Parameter` filter.
            LANG::Kotlin => &[("fun h(a: Int, /* one */ b: Int): Int { return a }", (0, 2))],
            // Names are counted inside each `parameter_declaration`, so a
            // sibling comment is invisible.
            LANG::Go => &[
                (
                    "package m\nfunc h(a int, /* one */ b int) int { return a }",
                    (0, 2),
                ),
                (
                    "package m\nvar f = func(x int, /* one */ y int) int { return x }",
                    (2, 0),
                ),
            ],
            // A positive `Identifier | VarargExpression` filter.
            LANG::Lua => &[("local f = function(a, --[[one]] b) return a end", (2, 0))],
            // Perl carried the comment exclusion inline before #1201
            // moved it into `count_args`; this pins that the move changed
            // nothing.
            LANG::Perl => &[(
                "use feature 'signatures';\nsub h(\n  $a, # one\n  $b\n) { return $a; }",
                (0, 2),
            )],
            _ => return None,
        })
    }

    /// Sweeps one fixture table, reporting every row that miscounted and
    /// failing differently when the table ran empty.
    ///
    /// The two populations are swept by two tests rather than one
    /// because they are **disjoint**, and a single test could not be
    /// gated (#1286): its guard was the conjunction `repaired > 0 &&
    /// guards > 0`, which no union of the two tables satisfies —
    /// `--features rust` supplies a repaired row and no guard row,
    /// `--features go` the reverse. Split, each test's `cfg` is exactly
    /// its own table's languages and each guard stays loud. The shared
    /// collect-then-report shape lives here so the split costs no
    /// duplication.
    fn sweep(table: impl Fn(LANG) -> Option<&'static [(&'static str, (u64, u64))]>, what: &str) {
        let mut checked = 0;
        let mut failures = Vec::new();
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            for (source, expected) in table(lang).unwrap_or_default() {
                checked += 1;
                let got = args(lang, source);
                // Collected rather than asserted inline so a revert of
                // any one of the four loops shows every language it
                // broke, not just the alphabetically first. The branch
                // carries no formatting — a line that runs only on
                // failure can never be covered, so the report is built
                // once, below, from the raw tuples.
                if got != *expected {
                    failures.push((lang, source, *expected, got));
                }
            }
        }
        // Bound eagerly and interpolated by name: an `assert!` argument is
        // evaluated only when the assertion fires, so spelling these out
        // as arguments would leave two more never-executed lines behind.
        let failed = failures.len();
        assert!(
            failures.is_empty(),
            "{failed}/{checked} {what} fixtures counted a comment as a parameter: {failures:#?}"
        );
        // The tally, so a table that stopped being reached — a renamed
        // `LANG` variant, a feature that stopped being enabled — fails
        // here rather than passing vacuously.
        assert!(
            checked > 0,
            "no {what} fixture ran; this test asserted nothing"
        );
    }

    #[test]
    #[cfg(any(
        feature = "c",
        feature = "cpp",
        feature = "csharp",
        feature = "elixir",
        feature = "groovy",
        feature = "java",
        feature = "javascript",
        feature = "kotlin",
        feature = "mozcpp",
        feature = "mozjs",
        feature = "objc",
        feature = "php",
        feature = "python",
        feature = "ruby",
        feature = "rust",
        feature = "typescript"
    ))]
    fn a_comment_in_a_repaired_parameter_list_is_not_a_parameter() {
        sweep(repaired_cases, "repaired");
    }

    #[test]
    #[cfg(any(
        feature = "go",
        feature = "groovy",
        feature = "kotlin",
        feature = "lua",
        feature = "objc",
        feature = "perl"
    ))]
    fn a_positive_parameter_filter_still_ignores_a_comment() {
        sweep(already_correct_cases, "already-correct");
    }

    /// Tcl — and iRules, which shares the shape — reports **4** for a
    /// `#` line inside a `proc` argument list, and that is correct.
    ///
    /// Tcl recognises a comment only where a command is expected, so
    /// `proc h {a\n# c\n b}` really does declare four arguments named
    /// `a`, `#`, `c` and `b`. The grammar agrees: it emits four
    /// `argument` nodes and no comment node, which is why
    /// `compute_tcl_args` never needed the exclusion the other
    /// languages did. Issue #1201 cited Tcl as the language that had
    /// already solved this; it had not — it has no problem to solve.
    ///
    /// **Both rows assert parity over a non-problem, not a defence.**
    /// Neither language has an exclusion here that a regression could
    /// remove; what these pin is that the *shape* stays the one described
    /// above, so a grammar bump that started emitting a comment node
    /// would surface as a count change rather than silently. The iRules
    /// row is the second half of a claim this doc and the #1201 changelog
    /// entry both made while only Tcl was exercised (#1218). It is a
    /// separate dialect grammar, and dialect grammars do diverge on leaf
    /// naming — its `argument` is kind 137 against Tcl's 93 — so it was
    /// dumped rather than assumed: `proc h {a\n# c\n b}` yields four
    /// `argument` nodes and no comment node under both.
    // Gated on the fixtures' own features for the reason #1220 names: the
    // case list is two languages wide and the loop below asserts it ran, so
    // a feature set enabling neither — `--no-default-features --features
    // rust` — would fail here and read as a defect in whatever was being
    // changed. The gate makes the test absent rather than vacuous; the
    // `ran > 0` assertion then covers the narrower case where `is_enabled`
    // stops agreeing with the feature it is compiled under.
    #[test]
    #[cfg(any(feature = "tcl", feature = "irules"))]
    fn tcl_has_no_comment_inside_a_parameter_list() {
        // Guarded per language rather than once: the two features are
        // independent, so a build with only one enabled must still run
        // that one's row.
        let mut ran = 0;
        for lang in [LANG::Tcl, LANG::Irules]
            .into_iter()
            .filter(LANG::is_enabled)
        {
            ran += 1;
            assert_eq!(
                args(lang, "proc h {a\n  # c\n  b} { return $a }"),
                (0, 4),
                "{lang:?}: a `#` in an argument list is an argument named `#`, not a comment"
            );
            // The uncommented control, so a regression that zeroed the
            // whole count would not read as this rule holding.
            assert_eq!(args(lang, "proc h {a b} { return $a }"), (0, 2), "{lang:?}");
        }
        // A feature set enabling neither leaves a loop of zero iterations
        // and a test that passes having asserted nothing — the shape
        // `assert_fixtures_present` exists to make loud (#1220).
        assert!(
            ran > 0,
            "neither tcl nor irules is enabled; this test asserted nothing"
        );
    }

    /// The one path `count_args` never runs on: `Checker::is_bare_param`
    /// short-circuits before the child walk, so a comment on an
    /// un-parenthesised lambda parameter is safe only because of where
    /// the *grammar* puts it. Confirmed rather than reasoned, per
    /// `docs/development/grammar_dispatch.md`: dumping
    /// `x /* c */ -> x` shows `block_comment` as a **sibling** of the
    /// bare `identifier`, and the `parameters` field points at that
    /// childless identifier.
    ///
    /// So the comment is not a discriminating input here, and this test
    /// is deliberately written as a *parity* assertion rather than a
    /// count one. No perturbation distinguishes the commented spelling
    /// from the bare one — both fail together under every perturbation of
    /// the bare-parameter branch, which
    /// `lambda_parenthesisation_parity` already covers. What this adds
    /// is the guarantee that the two spellings cannot diverge, which is
    /// what would break if a grammar bump moved the comment inside the
    /// field.
    #[test]
    #[cfg(any(feature = "csharp", feature = "java"))]
    fn a_comment_on_a_bare_lambda_parameter_changes_nothing() {
        let mut checked = 0;
        for (lang, commented, bare) in [
            (
                LANG::Java,
                "class K { java.util.function.Function<Integer,Integer> f = x /* c */ -> x; }",
                "class K { java.util.function.Function<Integer,Integer> f = x -> x; }",
            ),
            (
                LANG::Csharp,
                "class K { System.Func<int,int> f = x /* c */ => x + 1; }",
                "class K { System.Func<int,int> f = x => x + 1; }",
            ),
        ]
        .into_iter()
        .filter(|(lang, ..)| lang.is_enabled())
        {
            checked += 1;
            let got = args(lang, commented);
            assert_eq!(
                got,
                args(lang, bare),
                "{lang:?}: the comment moved the count off the bare spelling's answer"
            );
            // The absolute value too, so a regression that zeroed both
            // spellings would not read as parity holding.
            assert_eq!(
                got,
                (1, 0),
                "{lang:?}: a bare lambda parameter is one closure argument"
            );
        }
        // The non-vacuity half the `cfg` above does not cover: a
        // zero-iteration loop here means `is_enabled` stopped agreeing
        // with the feature this compiled under (#1286).
        assert!(
            checked > 0,
            "neither java nor csharp is enabled; this test asserted nothing"
        );
    }
}

// The guard below is a *conjunction* over three tables, so the gate is
// their intersection rather than their union: `cpp_only_shapes` is the
// narrowest at `Cpp | Mozcpp`, and both other tables include those two,
// so `any(cpp, mozcpp)` is exactly the set of feature configurations in
// which all three tallies can be non-zero (#1286). A plain union would
// re-admit the spurious failure — `--features c` supplies shared and
// attributed rows but no `cpp_only` one.
#[cfg(test)]
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
mod c_family_return_type_declarators {
    use crate::test_support::space_verbatim;
    use crate::{LANG, MetricsOptions};

    /// `(closure_args, function_args)` read from the fixture's sole
    /// nested space.
    ///
    /// The space rather than the file roll-up, because since #1196 the
    /// `nargs` gate reads a callable's *own* parameter count: a fix that
    /// repaired only the roll-up would leave the gate exactly as blind
    /// as #1200 found it. The pair rather than the total, so a
    /// regression that merely moved a count between the function and
    /// closure channels cannot read as a pass.
    #[track_caller]
    fn sole_space_args(lang: LANG, source: &str) -> (u64, u64) {
        let root = space_verbatim(lang, source.as_bytes(), MetricsOptions::default());
        // Descend to the *innermost* sole space, not the first one.
        // A free function is one level down, but a conversion operator
        // is two — its `struct` opens a container space in between,
        // whose own counters stay at zero however badly the operator is
        // counted. Stopping at the first level made both `operator_cast`
        // fixtures assert about the struct and pass with the defect
        // reinstated.
        let mut space = &root;
        let mut depth = 0;
        while let [only] = space.spaces.as_slice() {
            space = only;
            depth += 1;
        }
        assert!(depth > 0, "{lang:?}: fixture opened no space at all");
        (
            space.metrics.nargs.closure_args(),
            space.metrics.nargs.function_args(),
        )
    }

    /// The return shapes every C-derived grammar here shares, plus the
    /// unwrapped control that keeps the fix honest.
    ///
    /// Every pointer row reported **0** before #1200 and the nested rows
    /// reported the *return type's* arity; the plain row already passed
    /// and is here so a helper that returned nothing at all could not
    /// look like a fix.
    ///
    /// Each nested row deliberately gives the inner and outer parameter
    /// lists **different** lengths. An earlier draft of the `__cdecl`
    /// row spelled both as one argument, which made it agree with the
    /// answer it was written to reject.
    fn c_declarator_shapes(lang: LANG) -> Option<&'static [(&'static str, (u64, u64))]> {
        if !matches!(lang, LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc) {
            return None;
        }
        // C declarator syntax, shared by all four grammars. `Foo` is
        // deliberately an undeclared type: tree-sitter resolves the
        // shape syntactically, and a fixture that leaned on a typedef
        // would be testing the fixture.
        Some(&[
            // A pointer return: the reported symptom in #1200.
            ("FILE *f(int a, int b, int c) { return 0; }", (0, 3)),
            // Two levels of `pointer_declarator`, so a walk that steps
            // exactly once still fails.
            ("int **g(int a, int b) { return 0; }", (0, 2)),
            // A storage-class specifier ahead of the pointer, which
            // sits outside the declarator entirely.
            ("static int *h(int a) { return 0; }", (0, 1)),
            // A function returning a pointer to a one-argument
            // function. The *outer* `function_declarator` owns
            // `(int c)` — the return type's list — so taking the first
            // `parameters` found reports 1. `fp` takes two.
            ("int (*fp(int a, int b))(int c) { return 0; }", (0, 2)),
            // The same shape with an MSVC calling convention, which
            // parses as a real `ms_call_modifier` node *preceding* the
            // declarator inside the `parenthesized_declarator`. This is
            // what makes the fallback take the last named child rather
            // than the first.
            (
                "int (__cdecl *w(int a, int b))(int c) { return 0; }",
                (0, 2),
            ),
            // The GNU attribute spelling, which all four grammars
            // absorb *into* the `function_declarator` rather than
            // wrapping it — so it never builds an
            // `attributed_declarator` and was never miscounted. It is
            // the control for the C++11 spelling below, a different
            // tree for the same source-level idea.
            (
                "int gdef(int a, int b) __attribute__((deprecated)) { return a; }",
                (0, 2),
            ),
            // C's `(void)` marker declares *no* parameters, but the
            // grammar emits a real `parameter_declaration` for it, so
            // every negative filter counted it as one.
            ("int none(void) { return 0; }", (0, 0)),
            // The two shapes `(void)` must not be confused with. An
            // unnamed parameter is structurally identical — a bare type
            // with no declarator — and really is one argument, so only
            // the bytes separate them. `void *` carries a declarator
            // and is likewise a real parameter.
            ("int unnamed(int) { return 0; }", (0, 1)),
            ("int ptr(void *p, int a) { return 0; }", (0, 2)),
            // The unwrapped control: its `declarator` field already is
            // the `function_declarator`, so it passed before the fix
            // and must keep passing after it.
            ("int plain(int a, int b) { return a; }", (0, 2)),
            // An unexpanded function-like macro in declarator position,
            // the shape every JNI shim takes (#1213). The macro's
            // `(name)` is the innermost list, so #1200's walk read 1
            // where the function declares 2.
            ("void MACRO(name)(int a, int b) { }", (0, 2)),
            // The multi-argument spelling. A gate keyed on the inner
            // list holding a single bare `type_identifier` — the
            // heuristic form the report proposed — passes the row above
            // and misses this one, which read the macro's 2 rather than
            // the function's 1. The two lists are deliberately
            // different lengths and in the opposite direction to the
            // row above, so neither row can agree with the other's
            // wrong answer.
            ("void MACRO(a, b)(int x) { }", (0, 1)),
            // The two mechanisms composed: a return type to step
            // through *and* a macro to stop at, so this is the only row
            // where the gate fires at a link the chain reached rather
            // than at the one it started from. It is also why the gate
            // tests `current`'s kind and not just the inner link's — a
            // `pointer_declarator`'s `declarator` field is a
            // `function_declarator` too, and stopping there lands on a
            // node with no `parameters`. That half is not exclusively
            // this row's to guard, though: dropping it also regresses
            // #1200's own `FILE *f(…)` and `int **g(…)` rows to 0.
            ("char *MACRO(n)(int a, int b) { return 0; }", (0, 2)),
            // Two nested invocations, so a gate that stopped one link
            // in still reads a macro's list. Three distinct lengths
            // because the single-nesting spelling `A(b)(c)(int x)`
            // reads 1 both before the fix and after it, and would
            // prove nothing.
            ("void A(b, c)(d)(int x, int y, int z) { }", (0, 3)),
            // A return type that nests without being a
            // `function_declarator` at all: the outer link is an
            // `array_declarator`, so the macro gate has nothing to fire
            // on and the chain must still reach `arr`'s own list. Two
            // arguments rather than the `(void)` this row first
            // carried — `(0, 0)` is what a walk returning `None` for
            // *everything* reports, so the row passed with the whole
            // chain dead while 28 others failed
            // (`docs/development/testing.md`, "Seed the state you claim to
            // assert on").
            ("int (*arr(int a, int b))[4] { return 0; }", (0, 2)),
        ])
    }

    /// The C++11 `[[…]]` attribute, which is the only spelling that
    /// builds an `attributed_declarator` — the one fieldless rule
    /// putting its declarator *first*, so the last-named-child fallback
    /// lands on the attribute unless `attribute_declaration` is
    /// excluded. Reported 0 both before #1200 and after its first cut.
    ///
    /// Objective-C is absent because its grammar parses `[[…]]` on a
    /// *definition* as a `declaration` — no `function_definition`, so no
    /// space and nothing to count. That is upstream, not a miscount of
    /// ours; the GNU spelling above covers Objective-C's attribute path.
    fn cpp11_attribute_shapes(lang: LANG) -> Option<&'static [(&'static str, (u64, u64))]> {
        matches!(lang, LANG::C | LANG::Cpp | LANG::Mozcpp).then_some(&[(
            "int attr(int a, int b) [[deprecated]] { return a; }",
            (0, 2),
        )])
    }

    /// The shapes only C++ has: `reference_declarator`, which — unlike
    /// `pointer_declarator` — exposes no `declarator` field at all, and
    /// the lambda, which reaches `params_owner` through the closure
    /// channel rather than the function one.
    fn cpp_only_shapes(lang: LANG) -> Option<&'static [(&'static str, (u64, u64))]> {
        Some(match lang {
            LANG::Cpp | LANG::Mozcpp => &[
                ("int &r(int a, int b) { static int x; return x; }", (0, 2)),
                // The rule's other spelling: `&&` is a distinct token
                // in the same `reference_declarator`, so a fix keyed on
                // the `&` token alone would pass the row above.
                (
                    "Foo &&m(int a) { static Foo f; return static_cast<Foo &&>(f); }",
                    (0, 1),
                ),
                // A member function returning a reference: the chain
                // ends at a `qualified_identifier` rather than a bare
                // one, which has named children of its own.
                ("Foo &Bar::get(int a) { static Foo f; return f; }", (0, 1)),
                // `operator()` is the one construct whose *source text*
                // looks like the macro nesting #1213 gates on, and it is
                // not rare: 1,546 function spaces across `DeepSpeech`,
                // against 46 direct nestings not one of which is an
                // operator. The grammar emits a single `operator_name`
                // with the parameter list as its sibling, so the gate is
                // unreachable from here rather than merely inactive.
                //
                // Which is what this row guards, and it is worth being
                // precise: no widening of the gate can fail it, because
                // the chain already stops at this declarator. It fails
                // if a future `tree-sitter-cpp` starts spelling
                // `operator()` as a nested `function_declarator` — at
                // which point the gate would silently halve the reported
                // arity of that whole population.
                (
                    "struct S { int operator()(int a, int b) const { return a; } };",
                    (0, 2),
                ),
                // An explicit template argument spelling a function
                // type. `template_function` is another name form with
                // named children, and its last one is the argument
                // list — so the last-named-child fallback walks off the
                // name side into `int (*)(int x, int y)` and bills that
                // type's two parameters to a one-argument function
                // unless `template_argument_list` is excluded. The two
                // lists are deliberately different lengths, so the row
                // cannot agree with the answer it rejects.
                (
                    "template <> void tspec<int (*)(int x, int y)>(int a) { }",
                    (0, 1),
                ),
                // A conversion operator takes no arguments, however
                // many its target *type* has. `operator_cast` is the
                // one link whose `declarator` field leaves the name
                // side, and following it billed the converted-to
                // function-pointer type's `(int x)` to the operator —
                // a regression the first cut of #1200 introduced and
                // no fixture then covered.
                (
                    "struct S { operator int (*)(int x) { return nullptr; } };",
                    (0, 0),
                ),
                // The same shape through a reference, so the fix cannot
                // be keyed on the pointer spelling alone.
                (
                    "struct S { operator int (&)(int x, int y) { static int *p; return *reinterpret_cast<int (*)(int, int)>(p); } };",
                    (0, 0),
                ),
                (
                    "int g() { auto f = [](int a, int b){ return a + b; }; return f(1, 2); }",
                    (2, 0),
                ),
                // The one shape with no declarator to walk: a
                // parameterless lambda has no
                // `abstract_function_declarator` at all, so the walk
                // returns `None` on its first step and `params_owner`
                // falls back to the node. `g` carries parameters of its
                // own so the expected pair is not all-zero — an
                // all-default expectation would hold however badly the
                // fallback behaved.
                (
                    "int g(int a, int b) { auto f = []{ return 1; }; return f(); }",
                    (0, 2),
                ),
                // …and the shape that makes that first step *matter*.
                // The row above executes the early return but cannot
                // discriminate it — perturbing the walk to begin at the
                // last named child instead of the `declarator` field
                // fails none of the suite, because a parameterless
                // lambda's body has nothing carrying `parameters` down
                // its last-child spine.
                //
                // A local *function declaration* does. `declaration`
                // has a `declarator` field of its own, so a walk that
                // starts outside the declarator chain lands on `q`'s
                // `function_declarator` and bills its two parameters to
                // the enclosing lambda, which declares none.
                (
                    "int g(int a, int b) { auto f = []{ int q(int x, int y); }; f(); return a + b; }",
                    (0, 2),
                ),
                // The guard for the walk's stop condition. A lambda's
                // `abstract_function_declarator` carries `parameters`
                // but its `declarator` field is *optional* and absent
                // here, so a walk that falls through to the last named
                // child descends into the parameter list and reports
                // `cb`'s own `(int x)` — one argument instead of two.
                (
                    "int g() { auto f = [](int a, int (*cb)(int x)){ return cb(a); }; return 0; }",
                    (2, 0),
                ),
            ],
            _ => return None,
        })
    }

    #[test]
    fn a_wrapped_return_type_does_not_hide_the_parameter_list() {
        let (mut shared, mut cpp_only, mut attributed) = (0, 0, 0);
        let mut failures = Vec::new();
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            for (table, counter) in [
                (c_declarator_shapes(lang), &mut shared),
                (cpp_only_shapes(lang), &mut cpp_only),
                (cpp11_attribute_shapes(lang), &mut attributed),
            ] {
                for (source, expected) in table.unwrap_or_default() {
                    *counter += 1;
                    let got = sole_space_args(lang, source);
                    // Collected rather than asserted inline so a
                    // regression shows every language and shape it
                    // broke, not just the first. The branch carries no
                    // formatting — a line that runs only on failure can
                    // never be covered.
                    if got != *expected {
                        failures.push((lang, source, *expected, got));
                    }
                }
            }
        }
        let (failed, checked) = (failures.len(), shared + cpp_only + attributed);
        assert!(
            failures.is_empty(),
            "{failed}/{checked} return-type shapes lost their parameter list: {failures:#?}"
        );
        // Every tally, so a renamed `LANG` variant or a feature that
        // stopped being enabled fails here rather than passing
        // vacuously.
        assert!(
            shared > 0 && cpp_only > 0 && attributed > 0,
            "no fixture ran (shared={shared}, cpp_only={cpp_only}, \
             attributed={attributed}); this test asserted nothing"
        );
    }

    // The #1208 bug-lock that stood here — asserting these shapes lost
    // their space *name* while keeping their arity — was retired when
    // #1208 landed. The name half now lives beside the walk it shares
    // with this module, in `crate::c_declarator`.
}
