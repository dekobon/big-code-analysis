//! `Npa` implementation for Ruby.
//!
//! An attribute is public iff *any* of the accessors it generated is
//! public (#1399). `attr_accessor :b` synthesises two methods, the
//! reader `b` and the writer `b=`, and a later visibility call names
//! each on its own: `private :b` reaches the reader only, so `b` is
//! then readable privately and writable publicly, and the attribute
//! counts as public; `attr_reader :b` with the same `private :b` has no
//! accessor left public and counts as private, as does `attr_accessor
//! :b` under `private :b, :b=`. The rule errs towards public, the
//! direction the `nm` / `npm` split already takes for a method that is
//! reachable by any route.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use std::ops::Range;

// The accessor names a call's arguments spell or declare — `:b` is the
// reader of `b`, `:b=` its writer — for the call to re-file and, where
// Ruby returns them, to hand up (`leave_call`).
type RubyAccessorNames<'a> = Vec<(&'a str, RubyAccessor)>;

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
//   attribute per symbol or string it names, with the accessors the
//   macro generates each a slot of the shared ledger (`ruby_ledger.rs`).
//
// Visibility is the walk's: the body opens public, a bare `private` /
// `public` / `protected` sets the flag wherever Ruby evaluates it as the
// marker call — unless a local of that name shadows it — and the
// argument form (`private attr_accessor :b`) governs what its argument
// list declares and leaves the flag alone (#1255). An attribute is an
// instance-level declaration, so a class-method keyword
// (`private_class_method attr_accessor :b`) never governs one and the
// flag decides. The reader of an `attr_reader :initialize` is one of
// the names Ruby makes private as it is defined, as `npm`'s `def
// initialize` is (#1400); the writer is not.
//
// The retroactive symbol form (`private :b` after `attr_accessor :b`)
// re-files the accessor it names, exactly as `npm` re-files a method,
// and the attribute's bit is folded from its accessors once the body
// is read (the module doc's rule). A nested keyword chain composes the
// same way: in `private public attr_accessor :x` (Ruby 3.1+, where
// `public` returns its argument; 3.0 raises) the macro declares `x`
// public and hands both accessor names up, so the outer `private`
// re-files both.
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
struct RubyAttributes<'a> {
    code: &'a [u8],
    // `class << x` rather than `class X`, where the automatic-private
    // rule does not reach.
    in_singleton_class: bool,
    accessors: RubyLedger<(&'a str, RubyAccessor)>,
    // One attribute per entry: the ledger slots of its accessors,
    // declared together — up to a reader and a writer. An instance- or
    // class-variable assignment has no accessor and carries its own
    // bit in one unnamed slot instead.
    attributes: Vec<Range<usize>>,
}

impl<'a> RubyAttributes<'a> {
    fn new(code: &'a [u8], in_singleton_class: bool) -> Self {
        Self {
            code,
            in_singleton_class,
            accessors: RubyLedger::new(),
            attributes: Vec::new(),
        }
    }

    // Records the attributes an `attr_*` macro declares, one per name
    // it spells, each accessor the macro generates a slot under the
    // accessor's name. `keyword` is the visibility call the macro is an
    // argument of, if it is one. Answers the accessor names declared,
    // for the call the macro sits in to pass on.
    fn declare_attributes(
        &mut self,
        generated: &[RubyAccessor],
        names: impl Iterator<Item = Option<&'a str>>,
        keyword: Option<RubyVisibilityCall>,
        flag: RubyVisibility,
    ) -> RubyAccessorNames<'a> {
        let public = ruby_wrapped_is_public(keyword, false, flag);
        let mut declared = Vec::new();
        for name in names {
            let first = self.accessors.len();
            for &accessor in generated {
                // The automatic-private rule loses to a keyword naming
                // the macro (`public attr_reader :initialize`), as it
                // does for `npm`'s `public def initialize`: the reader
                // is declared private here and the keyword re-files it
                // on `leave_call`, handed the name below.
                let auto_private = accessor == RubyAccessor::Reader
                    && name.is_some_and(|name| ruby_is_auto_private(name, self.in_singleton_class));
                let key = name.map(|name| (name, accessor));
                self.accessors.declare(key, public && !auto_private);
                declared.extend(key);
            }
            self.attributes.push(first..self.accessors.len());
        }
        declared
    }

    // Counts what one statement or argument declares. `keyword` is the
    // visibility call `node` is an argument of, if it is one. Answers
    // the accessor names declared, for `argument` to hand the call.
    fn read(
        &mut self,
        node: &Node<'a>,
        shape: RubyBodyShape,
        keyword: Option<RubyVisibilityCall>,
        body: &mut RubyBodyState<'a, '_>,
    ) -> RubyAccessorNames<'a> {
        use crate::roles::ruby::member;

        let code = self.code;
        let flag = body.visibility();
        match shape {
            RubyBodyShape::Call => {
                let Some(generated) = ruby_attr_macro_accessors(node, code) else {
                    return Vec::new();
                };
                let names = ruby_call_named_arguments(node)
                    .flat_map(|argument| ruby_argument_names(&argument, code));
                self.declare_attributes(generated, names, keyword, flag)
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
                    let slot = self
                        .accessors
                        .declare(None, ruby_declaration_is_public(false, flag));
                    self.attributes.push(slot..slot + 1);
                }
                Vec::new()
            }
            RubyBodyShape::Definition | RubyBodyShape::Leaf => Vec::new(),
        }
    }

    // The attributes so far: how many, and how many public once each
    // accessor's last re-file is applied — an attribute is public iff
    // any of its accessors is.
    fn tally(&self) -> (usize, usize) {
        let accessors = self.accessors.resolved();
        let public = self
            .attributes
            .iter()
            .filter(|slots| accessors[(*slots).clone()].contains(&true))
            .count();
        (self.attributes.len(), public)
    }
}

impl<'a> RubyBodyConsumer<'a> for RubyAttributes<'a> {
    type Gathered = RubyAccessorNames<'a>;

    fn statement(
        &mut self,
        statement: &Node<'a>,
        shape: RubyBodyShape,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        self.read(statement, shape, None, body);
    }

    // The argument carries the enclosing call's keyword, so `private
    // attr_accessor :b` is counted once, here, as the keyword says. An
    // `attr_*` macro names the accessors it declared to the call — Ruby
    // 3.0+ returns them — and a symbol or string names one.
    fn argument(
        &mut self,
        argument: &Node<'a>,
        shape: RubyBodyShape,
        call: &mut RubyOpenCall<Self::Gathered>,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        let declared = self.read(argument, shape, call.keyword, body);
        let names = &mut call.gathered;
        names.extend(declared);
        if shape == RubyBodyShape::Leaf {
            names.extend(
                ruby_argument_names(argument, self.code)
                    .into_iter()
                    .flatten()
                    .map(RubyAccessor::split),
            );
        }
    }

    // An attribute's accessors are instance methods, so only the bare
    // keywords re-file one; the class-method pair names the singleton
    // family and reaches nothing here.
    fn leave_call(
        &mut self,
        call: RubyOpenCall<Self::Gathered>,
        outer: Option<&mut RubyOpenCall<Self::Gathered>>,
    ) {
        ruby_leave_visibility_call(call, outer, |name, keyword| {
            if !keyword.targets_singleton {
                self.accessors
                    .refile(&name, keyword.visibility == RubyVisibility::Public);
            }
        });
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
        // role-bypass: allow(Container) — the singleton-class bucket
        let mut attributes = RubyAttributes::new(code, matches!(parent_kind, SingletonClass));
        RubyBodyState::walk(code, ancestors.of_children(node), node, &mut attributes);

        let (count, public) = attributes.tally();
        stats.class_na += count;
        stats.class_npa += public;
    }
}
