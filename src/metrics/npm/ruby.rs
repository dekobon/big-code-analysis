//! `Npm` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::npa::{
    RubyBodyConsumer, RubyBodyShape, RubyBodyState, RubyLedger, RubyOpenCall, RubyVisibility,
    RubyVisibilityCall, ruby_argument_names, ruby_is_auto_private, ruby_leave_visibility_call,
    ruby_method_name, ruby_wrapped_is_public,
};

use super::*;

// The method names a call's arguments spell or declare, for the call
// to re-file and, where Ruby returns them, to hand up (`leave_call`).
type RubyArgumentNames<'a> = Vec<&'a str>;

// The methods a Ruby class body has declared so far and what each is
// worth. The statements, the wrappers they sit in and the visibility
// flag are the shared body walk's (`npa/ruby_body.rs`), and the
// declarations with their public bit and the re-filing the visibility
// keywords applied to them are the shared ledger's
// (`npa/ruby_ledger.rs`); this is what `npm` reads off them. Visibility
// cannot be settled arm-by-arm — `private :foo` demotes a method
// declared *earlier* in the same body — so the tally is taken once the
// whole body has been read (#1255).
struct RubyMethodLedger<'a> {
    code: &'a [u8],
    // `class << x` rather than `class X`. Ruby's automatic-private rule
    // does not reach a singleton class, so the ledger needs to know
    // which of the two it is keeping.
    in_singleton_class: bool,
    // Every declaration by name and family (instance or singleton):
    // `private :foo` reaches every instance `foo` declared so far —
    // Ruby's method table holds one and the call re-files it — and
    // never the singleton `foo`.
    methods: RubyLedger<(&'a str, bool)>,
}

impl<'a> RubyMethodLedger<'a> {
    fn new(code: &'a [u8], in_singleton_class: bool) -> Self {
        Self {
            code,
            in_singleton_class,
            methods: RubyLedger::new(),
        }
    }

    // Whether the automatic-private rule covers a declaration of this
    // name. Two independent ways to be exempt, and the walk sees them at
    // different levels: `def self.x` carries its own singleton flag,
    // while a `def x` inside `class << x` is an ordinary `Method` node
    // that only the enclosing body knows about.
    fn is_auto_private(&self, name: Option<&str>, singleton: bool) -> bool {
        !singleton && name.is_some_and(|n| ruby_is_auto_private(n, self.in_singleton_class))
    }

    // Records a `method` / `singleton_method` node, taking its
    // visibility from `keyword` when a visibility call wraps it and from
    // the body-wide flag otherwise. Answers the declared name, for the
    // call it sits in to pass on.
    fn declare(
        &mut self,
        method: &Node<'a>,
        keyword: Option<RubyVisibilityCall>,
        body_flag: RubyVisibility,
    ) -> Option<&'a str> {
        use Ruby::*;

        // A `def` is the `FuncOpener` row's callable, which the walk
        // has already asked; the row's two callables differ in the
        // bucket (`def self.` is the singleton), which is this
        // consumer's reading.
        // role-bypass: allow(FuncOpener) — the singleton bucket
        let singleton = method.kind_id() == SingletonMethod as u16;
        let name = ruby_method_name(method, self.code);
        let named_by_keyword = keyword.is_some_and(|kw| kw.governs(singleton));
        let public = ruby_wrapped_is_public(keyword, singleton, body_flag)
            && (named_by_keyword || !self.is_auto_private(name, singleton));
        self.methods
            .declare(name.map(|name| (name, singleton)), public);
        name
    }

    // The declarations so far: how many, and how many public once each
    // name's last re-file is applied over the declarations it covered.
    fn tally(&self) -> (usize, usize) {
        let public = self.methods.resolved();
        (public.len(), public.iter().filter(|&&p| p).count())
    }
}

impl<'a> RubyBodyConsumer<'a> for RubyMethodLedger<'a> {
    type Gathered = RubyArgumentNames<'a>;

    // A `def` the body or a wrapper runs declares a method under the
    // body-wide flag.
    fn statement(
        &mut self,
        statement: &Node<'a>,
        shape: RubyBodyShape,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        if shape == RubyBodyShape::Definition {
            self.declare(statement, None, body.visibility());
        }
    }

    // One argument of a call: a `def` (`private def x; end`, `memoize
    // def x; end`) declares under the call's keyword and names itself
    // to the call, and a symbol or string names a method. A nested call
    // names what it returns when it is left; a wrapper passed as an argument
    // — `private((def x; end))`, `memoize(begin … end)`, `memoize(x =
    // def y; end)` — holds statements that run at class scope exactly
    // as a bare one does, so the `def`s inside are declared. What the
    // wrapper *evaluates to* — the value of its last statement, which
    // Ruby hands to the call (`private((def a; end; def b; end))`
    // demotes `b` alone, measured) — is not resolved: the `def`s take
    // the body-wide flag and the enclosing keyword re-files nothing
    // through them. Counting them at all is the correction the #1401
    // review asked for; their visibility under a wrapping keyword is the
    // approximation recorded in `ruby_def_in_a_wrapper_argument_counts`.
    fn argument(
        &mut self,
        argument: &Node<'a>,
        shape: RubyBodyShape,
        call: &mut RubyOpenCall<Self::Gathered>,
        body: &mut RubyBodyState<'a, '_>,
    ) {
        let names = &mut call.gathered;
        match shape {
            RubyBodyShape::Definition => {
                names.extend(self.declare(argument, call.keyword, body.visibility()));
            }
            RubyBodyShape::Call | RubyBodyShape::Wrapper => {}
            RubyBodyShape::Leaf => {
                names.extend(ruby_argument_names(argument, self.code).flatten());
            }
        }
    }

    // A visibility keyword governs a nested `def` only when it names that
    // method family: `private def self.x` demotes the *instance* method
    // `x` (a NameError in practice), never the singleton, and only
    // `private_class_method` reaches a `def self.`. A symbol argument
    // names a method already recorded in that family and re-files it.
    fn leave_call(
        &mut self,
        call: RubyOpenCall<Self::Gathered>,
        outer: Option<&mut RubyOpenCall<Self::Gathered>>,
    ) {
        ruby_leave_visibility_call(call, outer, |name, keyword| {
            self.methods.refile(
                &(name, keyword.targets_singleton),
                keyword.visibility == RubyVisibility::Public,
            );
        });
    }
}

// Ruby `Method` and `SingletonMethod` declared inside a `Class` or
// `SingletonClass` body count as methods, whether they stand as direct
// children of the body or nested in a visibility call's argument list
// (`private def x`, which is the whole of that method's declaration).
//
// Visibility follows Ruby's own rules, which the keyword-marker flag
// alone does not cover (#1255):
// - a bare `private` / `public` / `protected` `Identifier` child of the
//   body sets the default for every subsequent *instance* method;
// - `def self.x` is a singleton method and ignores that flag — only
//   `private_class_method` demotes one;
// - the argument forms do not touch the flag, but do govern what they
//   name: `private def x` declares a private `x`, and `private :foo`
//   re-files a method declared earlier in the same body;
// - the five `RUBY_AUTO_PRIVATE_METHODS` names are private from the
//   moment they are defined, over the top of the flag (#1400).
//
// `Module` bodies are not classes (the getter routes them to
// `SpaceKind::Namespace`); they do not contribute to `Npm` so a
// module-only file reports zero methods.
impl Npm for RubyCode {
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
        // A `body_statement` is never the root, so the guard only keeps
        // this total.
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };
        // role-bypass: allow(Container) — which container's body this is,
        // for the bucket; not whether it opens a space
        if !matches!(parent_kind, Class | SingletonClass) {
            return;
        }

        // role-bypass: allow(Container) — the singleton-class bucket
        let mut ledger = RubyMethodLedger::new(code, matches!(parent_kind, SingletonClass));
        RubyBodyState::walk(code, ancestors.of_children(node), node, &mut ledger);

        let (count, public) = ledger.tally();
        stats.class_nm += count;
        stats.class_npm += public;
    }
}
