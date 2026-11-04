//! `Npm` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::npa::{
    RubyBodyConsumer, RubyBodyShape, RubyBodyState, RubyOpenCall, RubyVisibility,
    RubyVisibilityCall, ruby_declaration_is_public, ruby_method_name, ruby_symbol_name,
    ruby_wrapped_is_public,
};
use std::collections::HashMap;

use super::*;

// The five instance methods Ruby makes private the moment they are
// defined: `Class#new` calls `initialize`, and the other four are
// dispatched by the runtime rather than by a caller, so `obj.initialize`
// raises `NoMethodError` (#1400).
//
// Measured on ruby 3.0.2 rather than assumed. Three properties of the
// rule that the spelling alone does not give away:
// - it beats the body-wide flag, so an explicit `public` marker above
//   `def initialize` still yields a private method;
// - it loses to a keyword that names the declaration directly, so
//   `public def initialize` is public;
// - it is instance-only, so both `def self.initialize` and a
//   `def initialize` inside `class << self` stay public.
const RUBY_AUTO_PRIVATE_METHODS: [&str; 5] = [
    "initialize",
    "initialize_copy",
    "initialize_dup",
    "initialize_clone",
    "respond_to_missing?",
];

// The method names a call's arguments spell or declare, for the call
// to re-file and, where Ruby returns them, to hand up (`leave_call`).
type RubyArgumentNames<'a> = Vec<&'a str>;

// The methods a Ruby class body has declared so far and what each is
// worth: the declarations with their public bit, and the re-filing the
// visibility keywords applied to them. The statements, the wrappers
// they sit in and the visibility flag are the shared body walk's
// (`npa/ruby_body.rs`); this is what `npm` reads off them. Visibility
// cannot be settled arm-by-arm — `private :foo` demotes a method
// declared *earlier* in the same body — so the tally is taken once the
// whole body has been read (#1255).
struct RubyMethodLedger<'a> {
    code: &'a [u8],
    // `class << x` rather than `class X`. Ruby's automatic-private rule
    // does not reach a singleton class, so the ledger needs to know
    // which of the two it is keeping.
    in_singleton_class: bool,
    // Every declaration so far, in order: its own public bit.
    public: Vec<bool>,
    // Every named declaration, by name and family (instance or
    // singleton): the declarations' indices into `public`, and the bit
    // a keyword call (`private :foo`, `private def foo`) last re-filed
    // the name to, with how many of the name's declarations it covered.
    // `private :foo` reaches every `foo` declared so far — Ruby's method
    // table holds one and the call re-files it — so a re-file records
    // "the first `covered` declarations of this name read `public`" in
    // one write, and the tally applies it; a body of `W` `private def
    // m` re-files `W` times at one lookup each where a position scan
    // was `W²` (0.32 s against 0.18 s on `main` at 32,000; #1401
    // review). The name is borrowed from the source bytes: Ruby spells
    // the target as a symbol, so identity is in the bytes
    // (grammar-dispatch rule 10).
    by_name: HashMap<(&'a str, bool), RubyNameEntry>,
}

// One name's declarations in a Ruby class body and the last re-file
// that reached them.
#[derive(Default)]
struct RubyNameEntry {
    indices: Vec<usize>,
    // `Some((covered, public))`: the first `covered` entries of
    // `indices` were re-filed to `public`, over their own bits.
    refiled: Option<(usize, bool)>,
}

impl<'a> RubyMethodLedger<'a> {
    fn new(code: &'a [u8], in_singleton_class: bool) -> Self {
        Self {
            code,
            in_singleton_class,
            public: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    // Whether `RUBY_AUTO_PRIVATE_METHODS` covers a declaration of this
    // name. Two independent ways to be exempt, and the walk sees them at
    // different levels: `def self.x` carries its own singleton flag,
    // while a `def x` inside `class << x` is an ordinary `Method` node
    // that only the enclosing body knows about.
    fn is_auto_private(&self, name: Option<&str>, singleton: bool) -> bool {
        !singleton
            && !self.in_singleton_class
            && name.is_some_and(|n| RUBY_AUTO_PRIVATE_METHODS.contains(&n))
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
        let public = keyword.map_or_else(
            || ruby_declaration_is_public(singleton, body_flag),
            |kw| ruby_wrapped_is_public(kw, singleton, body_flag),
        ) && (named_by_keyword || !self.is_auto_private(name, singleton));
        if let Some(name) = name {
            self.by_name
                .entry((name, singleton))
                .or_default()
                .indices
                .push(self.public.len());
        }
        self.public.push(public);
        name
    }

    // Re-files every already-declared method of `name` in the family the
    // keyword names. Ruby's `private :foo` reaches back over the body,
    // and only over the body: a `def foo` written *after* the call keeps
    // the visibility it was declared with.
    fn refile(&mut self, name: &'a str, singleton: bool, public: bool) {
        if let Some(entry) = self.by_name.get_mut(&(name, singleton)) {
            entry.refiled = Some((entry.indices.len(), public));
        }
    }

    // The declarations so far: how many, and how many public once each
    // name's last re-file is applied over the declarations it covered.
    fn tally(&self) -> (usize, usize) {
        let mut public = self.public.clone();
        for entry in self.by_name.values() {
            if let Some((covered, bit)) = entry.refiled {
                for &index in &entry.indices[..covered] {
                    public[index] = bit;
                }
            }
        }
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
    // to the call, and a symbol names a method. A nested call names
    // what it returns when it is left; a wrapper passed as an argument
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
        use Ruby::*;

        let names = &mut call.gathered;
        match shape {
            RubyBodyShape::Definition => {
                names.extend(self.declare(argument, call.keyword, body.visibility()));
            }
            RubyBodyShape::Call | RubyBodyShape::Wrapper => {}
            RubyBodyShape::Leaf => match argument.kind_id().into() {
                // role-bypass: allow(StringLit) — a `%i[]` array names several
                // methods; the row owns it as a literal, a different question
                SymbolArray => names.extend(
                    argument
                        .children()
                        .filter_map(|sym| ruby_symbol_name(&sym, self.code)),
                ),
                _ => names.extend(ruby_symbol_name(argument, self.code)),
            },
        }
    }

    // Leaves a call whose arguments have all been read: the keyword, if
    // any, re-files what they named, and what the call returns is handed
    // to the enclosing call when that is what the call is an argument of.
    //
    // A visibility keyword governs a nested `def` only when it names that
    // method family: `private def self.x` demotes the *instance* method
    // `x` (a NameError in practice), never the singleton, and only
    // `private_class_method` reaches a `def self.`. A symbol argument
    // names a method already recorded and re-files it; so does what a
    // nested visibility call returns: `private public def x` demotes `x`
    // because `public` returns its argument — Ruby 3.1+, where
    // `set_visibility` answers `argv[0]` for one argument and an array
    // for several; 3.0 raised a `TypeError` on the nested call — and the
    // outer `private` re-files what it is handed. The class-method pair
    // returns the receiver, not its arguments, so an outer call is
    // handed nothing; nor is it from an ordinary call (`memoize def x`),
    // whose result is the decorator's own and not assumed to be the
    // name.
    //
    // Each level of a nested keyword chain re-files every name it is
    // handed and passes them all up, so `D` nested `private(public(…))`
    // around `W` symbols is `D × W` lookups on an input of size `D + W`
    // (batch perf audit: 16,000 × 16,000 took 20 s). The semantics
    // require each governing level to act, real code nests two deep,
    // and no probe reads the shape; recorded, not restructured.
    fn leave_call(
        &mut self,
        call: RubyOpenCall<Self::Gathered>,
        outer: Option<&mut RubyOpenCall<Self::Gathered>>,
    ) {
        let Some(keyword) = call.keyword else {
            return;
        };
        let public = keyword.visibility == RubyVisibility::Public;
        for name in &call.gathered {
            self.refile(name, keyword.targets_singleton, public);
        }
        if keyword.targets_singleton {
            return;
        }
        if let Some(outer) = outer {
            outer.gathered.extend(call.gathered);
        }
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
