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
    visibility: RubyVisibility,
    methods: Vec<RubyMethodDecl<'a>>,
}

impl<'a> RubyClassBody<'a> {
    fn new(code: &'a [u8]) -> Self {
        Self {
            code,
            // Ruby class bodies open in default-public state, whatever
            // the previous body's trailing visibility was.
            visibility: RubyVisibility::Public,
            methods: Vec::new(),
        }
    }

    // Records a `method` / `singleton_method` node.
    fn declare(&mut self, method: &Node<'a>, singleton: bool, public: bool) {
        let name = ruby_method_name(method, self.code);
        self.methods.push(RubyMethodDecl {
            name,
            singleton,
            public,
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
            let declared_public = ruby_wrapped_is_public(keyword, singleton, self.visibility);
            self.declare(&arg, singleton, declared_public);
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
                let singleton = matches!(kind, SingletonMethod);
                let public = ruby_declaration_is_public(singleton, self.visibility);
                self.declare(child, singleton, public);
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
//   re-files a method declared earlier in the same body.
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

        let mut body = RubyClassBody::new(code);
        for child in node.children() {
            body.visit(&child);
        }

        stats.class_nm += body.methods.len();
        stats.class_npm += body.methods.iter().filter(|m| m.public).count();
    }
}
