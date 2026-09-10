//! `Npm` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::npa::{
    RubyVisibility, RubyVisibilityCall, RubyVisibilityEffect, ruby_call_named_arguments,
    ruby_declaration_is_public, ruby_method_name, ruby_symbol_name, ruby_visibility_effect,
    ruby_visibility_marker, ruby_wrapped_is_public,
};
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

// One method declared by a Ruby class body. Visibility cannot be settled
// arm-by-arm: `private :foo` demotes a method declared *earlier* in the
// same body, so the tally is taken once the whole body has been read
// (#1255). The name is borrowed from the source bytes for that match —
// Ruby spells the target as a symbol, so identity is in the bytes
// (grammar-dispatch rule 10).
struct RubyMethodDecl<'a> {
    name: Option<&'a str>,
    singleton: bool,
    public: bool,
}

// The state one pass over a Ruby class body carries: the source, the
// running body-wide visibility flag, and the declarations seen so far.
// Every arm of the walk needs at least two of the three, and the
// retroactive symbol form needs to reach back into the third, so they
// travel together rather than through a parameter list.
struct RubyClassBody<'a> {
    code: &'a [u8],
    // `class << x` rather than `class X`. Ruby's automatic-private rule
    // does not reach a singleton class, so the body needs to know which
    // of the two it is walking.
    in_singleton_class: bool,
    visibility: RubyVisibility,
    methods: Vec<RubyMethodDecl<'a>>,
}

impl<'a> RubyClassBody<'a> {
    fn new(code: &'a [u8], in_singleton_class: bool) -> Self {
        Self {
            code,
            in_singleton_class,
            // Ruby class bodies open in default-public state, whatever
            // the previous body's trailing visibility was.
            visibility: RubyVisibility::Public,
            methods: Vec::new(),
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
    // the body-wide flag otherwise.
    fn declare(&mut self, method: &Node<'a>, singleton: bool, keyword: Option<RubyVisibilityCall>) {
        let name = ruby_method_name(method, self.code);
        let named_by_keyword = keyword.is_some_and(|kw| kw.governs(singleton));
        let public = keyword.map_or_else(
            || ruby_declaration_is_public(singleton, self.visibility),
            |kw| ruby_wrapped_is_public(kw, singleton, self.visibility),
        );
        self.methods.push(RubyMethodDecl {
            name,
            singleton,
            public: public && (named_by_keyword || !self.is_auto_private(name, singleton)),
        });
    }

    // Re-files every already-declared method of `name` in the family the
    // keyword names. Ruby's `private :foo` reaches back over the body,
    // and only over the body: a `def foo` written *after* the call keeps
    // the visibility it was declared with.
    fn refile(&mut self, name: &str, singleton: bool, public: bool) {
        for target in self
            .methods
            .iter_mut()
            .filter(|m| m.singleton == singleton && m.name == Some(name))
        {
            target.public = public;
        }
    }

    // Re-files every method one visibility-call argument names. A
    // `%i[a b]` word array names several; every other resolvable
    // spelling names one.
    fn refile_argument(&mut self, arg: &Node<'a>, singleton: bool, public: bool) {
        let code = self.code;
        if matches!(arg.kind_id().into(), Ruby::SymbolArray) {
            for name in arg
                .children()
                .filter_map(|sym| ruby_symbol_name(&sym, code))
            {
                self.refile(name, singleton, public);
            }
        } else if let Some(name) = ruby_symbol_name(arg, code) {
            self.refile(name, singleton, public);
        }
    }

    // Applies a visibility call's arguments (`private def x`,
    // `private :foo`, `private_class_method :factory`).
    //
    // A nested `def` is a declaration this walk would otherwise never
    // see — the `method` node hangs off the call's argument list, not
    // off the body — so it is recorded here. A symbol argument names a
    // method already recorded and re-files it.
    //
    // Either way the keyword governs the declaration only when it names
    // that method family: `private def self.x` demotes the *instance*
    // method `x` (a NameError in practice), never the singleton, and
    // only `private_class_method` reaches a `def self.`.
    fn apply_visibility_call(&mut self, call: &Node<'a>, keyword: RubyVisibilityCall) {
        use Ruby::*;

        let public = keyword.visibility == RubyVisibility::Public;
        for arg in ruby_call_named_arguments(call) {
            let singleton = match arg.kind_id().into() {
                Method => false,
                SingletonMethod => true,
                _ => {
                    self.refile_argument(&arg, keyword.targets_singleton, public);
                    continue;
                }
            };
            self.declare(&arg, singleton, Some(keyword));
        }
    }

    // Reads one direct child of the class body.
    fn visit(&mut self, child: &Node<'a>) {
        use Ruby::*;

        if let Some(marker) = ruby_visibility_marker(child, self.code) {
            self.visibility = marker;
            return;
        }
        let kind = child.kind_id().into();
        match kind {
            Method | SingletonMethod => {
                self.declare(child, matches!(kind, SingletonMethod), None);
            }
            Call | Call2 | Call3 | Call4 => match ruby_visibility_effect(child, self.code) {
                Some(RubyVisibilityEffect::Flag(flag)) => self.visibility = flag,
                Some(RubyVisibilityEffect::Arguments(keyword)) => {
                    self.apply_visibility_call(child, keyword);
                }
                None => {}
            },
            _ => {}
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
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };
        if !matches!(parent_kind, Class | SingletonClass) {
            return;
        }

        let mut body = RubyClassBody::new(code, matches!(parent_kind, SingletonClass));
        for child in node.children() {
            body.visit(&child);
        }

        stats.class_nm += body.methods.len();
        stats.class_npm += body.methods.iter().filter(|m| m.public).count();
    }
}
