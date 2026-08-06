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
    /// Uses the source-aware [`Checker::is_func_with_code`] rather than the
    /// byte-less `is_func`, exactly as [`crate::nom::Nom::compute`] does.
    /// For every grammar with a syntactic function-definition node the two
    /// are the same predicate, so no count moves; the point is that a
    /// language whose declarations are only recognisable from the source
    /// text — Elixir's `def` is an ordinary `Call` (#275) — does not
    /// silently report 0 here (#1142).
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
    /// means re-stating the `is_func_with_code`-not-`is_func` rule and
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
    /// Lua, Tcl, iRules, Perl, Elixir, Groovy) would define a
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
    fn compute<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func(node, ancestors) {
            compute_go_args(node, &mut stats.fn_nargs);
            return;
        }

        if Self::is_closure(node, ancestors) {
            compute_go_args(node, &mut stats.closure_nargs);
        }
    }
}

fn compute_kotlin_func_args(node: &Node, nargs: &mut usize) {
    if let Some(params) = node
        .children()
        .find(|c| c.kind_id() == Kotlin::FunctionValueParameters)
    {
        params.act_on_child(&mut |n| {
            if n.kind_id() == Kotlin::Parameter {
                *nargs += 1;
            }
        });
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
        if Self::is_func(node, ancestors) {
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
    fn compute<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func(node, ancestors) {
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
    fn compute<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func(node, ancestors) {
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
    fn compute<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        if Self::is_func(node, ancestors) {
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
        if Self::is_func(node, ancestors) {
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
        // `is_func` is byte-less and constant `false` for Elixir, because a
        // `def` is textually indistinguishable from any other `Call`. The
        // code-aware predicate (#275) is the only one that identifies one,
        // and it also excludes a `def` inside `quote do … end`, which
        // declares nothing until the macro expands (#310).
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
    CsharpCode,
    RubyCode
);

// A record's compact constructor (`record R(int a, int b) { R { … } }`,
// JLS 8.10.4) declares no formal parameter list of its own: the grammar
// gives `compact_constructor_declaration` only `name` and `body` fields,
// and hangs the parameters — the record's components — off the enclosing
// `record_declaration`. Resolve that declaration so the two spellings of
// one constructor agree: the canonical `R(int a, int b) { … }` reports 2,
// and so should the compact form (#1160).
//
// The nesting is fixed by the grammar — the constructor is a direct child
// of the record's `class_body` — so the record sits exactly two steps up.
// The kind check states what that positional step is allowed to land on;
// no kind other than `record_declaration` carries a `parameters` field
// here, so it changes no count today, but it keeps the step from silently
// starting to mean something else if one ever does.
//
// A record that declares *no* constructor still reports 0: its canonical
// constructor is implicit, so there is no node to open a space for and
// nothing to attribute the components to. Adding an empty `R { }` to such
// a record therefore moves its `nargs` from 0 to the component count
// without changing the API — the same asymmetry an explicit
// `R(int a, int b) { }` already produces, and the price of measuring
// declared code rather than generated code.
fn java_compact_constructor_record<'tree>(
    node: &Node<'tree>,
    ancestors: Ancestors<'tree, '_>,
) -> Option<Node<'tree>> {
    if node.kind_id() != Java::CompactConstructorDeclaration {
        return None;
    }
    let (grandparent, _) = ancestors.iter(node).nth(1)?;
    (grandparent.kind_id() == Java::RecordDeclaration).then_some(grandparent)
}

impl NArgs for JavaCode {
    fn params_owner<'tree>(node: &Node<'tree>, ancestors: Ancestors<'tree, '_>) -> Node<'tree> {
        java_compact_constructor_record(node, ancestors).unwrap_or(*node)
    }
}

// Groovy closures use `closure_parameters` as an unnamed child rather
// than a `parameters` field, so the default NArgs walker (which looks
// for a `parameters` field) misses them. Match the closure_parameters
// child directly and count its `closure_parameter` grand-children.
impl NArgs for GroovyCode {
    fn compute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>, stats: &mut Stats) {
        use crate::languages::language_groovy::Groovy;

        if Self::is_func(node, ancestors) {
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
    use crate::test_support::check_metrics_only_shim;

    use super::*;

    // Nargs pulls Nom for its per-function average divisor, which is also
    // what this module's `metric.nom.functions_sum()` /
    // `closures_sum()` assertions read.
    check_metrics_only_shim!(check_metrics, Nargs);

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

    /// Perl 5.38's `method` is a second `is_func` kind
    /// (`function_definition_without_sub`) reaching the same helper, so it
    /// gets its own fixture rather than riding on the `sub` tests.
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

    #[test]
    fn groovy_no_functions_and_closures() {
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.nargs.total(), 0);
        });
    }

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

    #[test]
    fn javascript_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn javascript_async_bare_arrow_function() {
        check_metrics::<JavascriptParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn javascript_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn javascript_multi_parenthesized_arrow_function() {
        check_metrics::<JavascriptParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 2);
        });
    }

    #[test]
    fn typescript_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn typescript_async_bare_arrow_function() {
        check_metrics::<TypescriptParser>("const f = async x => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn typescript_parenthesized_arrow_function() {
        check_metrics::<TypescriptParser>("const f = (x: number) => x;", "foo.ts", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

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

    #[test]
    fn tsx_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn tsx_async_bare_arrow_function() {
        check_metrics::<TsxParser>("const f = async x => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn tsx_parenthesized_arrow_function() {
        check_metrics::<TsxParser>("const f = (x: number) => x;", "foo.tsx", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

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

    #[test]
    fn mozjs_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn mozjs_async_bare_arrow_function() {
        check_metrics::<MozjsParser>("const f = async x => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn mozjs_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x) => x;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 1);
        });
    }

    #[test]
    fn mozjs_multi_parenthesized_arrow_function() {
        check_metrics::<MozjsParser>("const f = (x, y) => x + y;", "foo.js", |metric| {
            assert_eq!(metric.nargs.total(), 2);
        });
    }

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

    #[test]
    fn lua_no_functions_and_closures() {
        check_metrics::<LuaParser>("local x = 1", "foo.lua", |metric| {
            // No functions or closures: both halves are zero.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn lua_single_function() {
        check_metrics::<LuaParser>("function f(a, b) return a + b end", "foo.lua", |metric| {
            // f(a, b) → fn_args_sum 2, no closures.
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

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

    #[test]
    fn tcl_no_functions() {
        check_metrics::<TclParser>("set x 1", "foo.tcl", |metric| {
            // Bare `set` command, no procs → both halves zero.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_single_function() {
        check_metrics::<TclParser>("proc f {a b} { puts $a }", "foo.tcl", |metric| {
            // proc f {a b} → fn_args_sum 2.
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

    #[test]
    fn tcl_single_function_no_args() {
        check_metrics::<TclParser>("proc f {} { puts hello }", "foo.tcl", |metric| {
            // proc f {} → empty arg list, fn_args_sum 0.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

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

    #[test]
    fn kotlin_zero_args() {
        check_metrics::<KotlinParser>("fun f(): Int { return 42 }", "foo.kt", |metric| {
            // fun f() → empty parameter list, fn_args_sum 0.
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
            insta::assert_json_snapshot!(metric.nargs);
        });
    }

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

    #[test]
    fn ruby_no_functions_and_closures() {
        check_metrics::<RubyParser>("a = 42\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    #[test]
    fn ruby_single_function() {
        // Single method with 3 parameters.
        check_metrics::<RubyParser>("def foo(a, b, c)\n  a + b + c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 3);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    #[test]
    fn ruby_single_closure() {
        // A bare block `[1,2,3].each { |x| ... }` is the only closure
        // here; `each` is a method call so the method-args count is 0.
        check_metrics::<RubyParser>("[1, 2, 3].each { |x| puts x }\n", "foo.rb", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
            assert_eq!(metric.nargs.closure_args_sum(), 1);
        });
    }

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
    #[test]
    fn irules_single_proc() {
        check_metrics::<IrulesParser>("proc f { a b } { return $a }\n", "foo.irule", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 2);
            assert_eq!(metric.nargs.closure_args_sum(), 0);
        });
    }

    /// A `proc` with an empty argument list contributes zero arguments.
    #[test]
    fn irules_proc_no_args() {
        check_metrics::<IrulesParser>("proc f { } { return 1 }\n", "foo.irule", |metric| {
            assert_eq!(metric.nargs.function_args_sum(), 0);
        });
    }

    /// A default-valued parameter (`{b 5}`) is a single `argument`, so each
    /// formal parameter counts once regardless of its default: `{a {b 5} c}`
    /// is three arguments.
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
#[cfg(test)]
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
    /// is classified by `check_if_arrow_func!` and lands in `fn_args`
    /// when bound to a variable, which is a separate question (#1188).
    /// Asserting the channel per language rather than globally keeps
    /// this test from encoding that as a bug.
    #[test]
    fn a_bare_lambda_stays_in_the_closure_channel() {
        for lang in [LANG::Java, LANG::Csharp] {
            if !lang.is_enabled() {
                continue;
            }
            let [bare, ..] = cases(lang).expect("both languages have cases");
            assert_eq!(
                args(lang, bare),
                (1, 0),
                "{lang:?}: the lambda's argument must be billed to closure_args"
            );
        }
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

    #[test]
    fn a_comment_in_a_parameter_list_is_not_a_parameter() {
        let (mut repaired, mut guards) = (0, 0);
        let mut failures = Vec::new();
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            for (table, counter) in [
                (repaired_cases(lang), &mut repaired),
                (already_correct_cases(lang), &mut guards),
            ] {
                for (source, expected) in table.unwrap_or_default() {
                    *counter += 1;
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
        }
        // Bound eagerly and interpolated by name: an `assert!` argument is
        // evaluated only when the assertion fires, so spelling these out
        // as arguments would leave two more never-executed lines behind.
        let (failed, total) = (failures.len(), repaired + guards);
        assert!(
            failures.is_empty(),
            "{failed}/{total} fixtures counted a comment as a parameter: {failures:#?}"
        );
        // Both tallies, so a table that stopped being reached — a renamed
        // `LANG` variant, a feature that stopped being enabled — fails
        // here rather than passing vacuously.
        assert!(
            repaired > 0 && guards > 0,
            "no fixture ran (repaired={repaired}, guards={guards}); this test asserted nothing"
        );
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
    /// `.claude/rules/grammar-dispatch.md`: dumping
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
    fn a_comment_on_a_bare_lambda_parameter_changes_nothing() {
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
    }
}

#[cfg(test)]
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
            // (`.claude/rules/testing.md`, "Seed the state you claim to
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
