//! `Npa` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// The attributes of one Ruby class or singleton-class body, counted off
// the statements the body walk `Npm` shares (`ruby_body.rs`) hands it,
// wherever that walk reads one — directly in the body, under a
// conditional, a loop, a `begin`, a `case` arm, a statement modifier or
// parentheses, on an assignment's right side, or as a call's argument:
// - a class-scope `@var = …` (`InstanceVariable`) or `@@var = …`
//   (`ClassVariable`) — the `Member` row's variables where they are a
//   plain assignment's sole target — is one attribute, whatever the right
//   side. The other ways Ruby sets one are not counted, as before the
//   shared walk: a multiple assignment (`@a, @b = 1, 2`), an operator
//   assignment (`@c ||= 1`), `for @d in …` and `rescue => @e`;
// - an `attr_accessor` / `attr_reader` / `attr_writer` macro is one
//   attribute per symbol it names.
//
// Visibility is the walk's: the body opens public, a bare `private` /
// `public` / `protected` sets the flag wherever Ruby evaluates it as the
// marker call — unless a local of that name shadows it — and the
// argument form (`private attr_accessor :b`) governs what its argument
// list declares and leaves the flag alone (#1255). An attribute is an
// instance-level declaration, so a class-method keyword
// (`private_class_method attr_accessor :b`) never governs one and the
// flag decides.
//
// The retroactive symbol form (`private :b` after `attr_accessor :b`)
// is deliberately not modelled. It renames the visibility of the
// generated *reader* only, leaving `b=` public, so there is no single
// answer for the attribute — unlike `Npm`, where the symbol names one
// method (see #1399). For the same reason a nested keyword chain is not
// re-filed: in `private public attr_accessor :x` (Ruby 3.1+, where
// `public` returns its argument; 3.0 raises) the attribute takes the
// innermost keyword, while `Npm` hands what `public` returns to the
// outer `private` and demotes a `def` spelled the same way.
//
// A keyword wrapping an ordinary call (`private memoize attr_reader :x`)
// is not resolved: the argument is not a declaration the walk recognises,
// so `x` takes the flag. Ruby makes it private when `memoize` returns its
// argument; `Npm` has the same gap for `private memoize def m`, so the
// two metrics still agree. So is a keyword whose argument is a statement
// wrapper (`private (attr_reader :x)` — the space makes the parentheses
// a statement list — or `private(begin; attr_reader :x; end)`): the walk
// reads the macro as one of the wrapper's statements, so it takes the
// flag, where Ruby hands the wrapper's last value to `private` and makes
// `x` private. `Npm` records the same approximation for a `def` there
// (`ruby_def_in_a_wrapper_argument_counts`).
//
// Attribute assignments to instance/class variables are visible only
// via the methods that wrap them, so the visibility flag at the point
// of declaration is what `npa` should reflect.
struct RubyAttributes<'s> {
    stats: &'s mut Stats,
}

impl RubyAttributes<'_> {
    // Counts what one statement or argument declares. `keyword` is the
    // visibility call `node` is an argument of, if it is one.
    fn read<'a>(
        &mut self,
        node: &Node<'a>,
        shape: RubyBodyShape,
        keyword: Option<RubyVisibilityCall>,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        use crate::roles::ruby::member;

        let code = body.code();
        let flag = body.visibility();
        match shape {
            RubyBodyShape::Call if ruby_is_attr_macro(node, code) => {
                let public = ruby_wrapped_is_public(keyword, false, flag);
                add_attributes(self.stats, ruby_attr_macro_symbol_count(node), public);
            }
            // An assignment is a wrapper to the walk, which reads its
            // right side for the statements it may hold; the target is
            // the `Member` row's question, asked under the assignment.
            // Of the wrappers only an assignment has a `left`, so no other
            // reaches the row. A keyword the assignment is an argument of
            // (`private(@x = 1)`) is handed the value, not a name, so it
            // governs nothing here and the flag decides.
            RubyBodyShape::Wrapper => {
                if let Some(target) = node.child_by_field_name("left")
                    && body.below(node, |under| member::owns(&target, code, under))
                {
                    add_attributes(self.stats, 1, ruby_declaration_is_public(false, flag));
                }
            }
            RubyBodyShape::Call | RubyBodyShape::Definition | RubyBodyShape::Leaf => {}
        }
    }
}

impl<'a> RubyBodyConsumer<'a> for RubyAttributes<'_> {
    type Gathered = ();

    fn statement(
        &mut self,
        statement: &Node<'a>,
        shape: RubyBodyShape,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        self.read(statement, shape, None, body);
    }

    // The argument carries the enclosing call's keyword, so `private
    // attr_accessor :b` is counted once, here, as the keyword says.
    fn argument(
        &mut self,
        argument: &Node<'a>,
        shape: RubyBodyShape,
        call: &mut RubyOpenCall<Self::Gathered>,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        self.read(argument, shape, call.keyword, body);
    }
}

impl Npa for RubyCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Ruby::*;

        if !matches!(node.kind_id().into(), BodyStatement | BodyStatement2) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };
        // role-bypass: allow(Container) — which container's body this is;
        // not whether it opens a space
        if !matches!(parent_kind, Class | SingletonClass) {
            return;
        }
        let mut attributes = RubyAttributes { stats };
        RubyBodyState::walk(code, ancestors.of_children(node), node, &mut attributes);
    }
}
