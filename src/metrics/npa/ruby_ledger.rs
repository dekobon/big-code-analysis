//! What a Ruby class body has declared so far and what each declaration
//! is worth, for the two consumers of the body walk (`ruby_body.rs`):
//! `npm` keeps methods here, `npa` the accessors an `attr_*` macro
//! generates. Each declaration is one slot with its own public bit, and
//! a visibility call's symbol arguments (`private :foo`) re-file by
//! name whatever was declared before them — the one mechanism both
//! consumers need, so it lives once (#1255, #1399, #1719).
//!
//! The re-filing cannot be settled slot by slot — `private :foo` demotes
//! a declaration *earlier* in the same body — so the bits are resolved
//! once the whole body has been read. A name's re-file records "the
//! first `covered` declarations of this name read `public`" in one
//! write, and `resolved` applies it: a body of `W` `private def m`
//! re-files `W` times at one lookup each where a position scan was `W²`
//! (0.32 s against 0.18 s on `main` at 32,000; #1401 review).
#![allow(clippy::wildcard_imports)]

use super::*;
use std::collections::HashMap;
use std::hash::Hash;

// The five instance methods Ruby makes private the moment they are
// defined: `Class#new` calls `initialize`, and the other four are
// dispatched by the runtime rather than by a caller, so `obj.initialize`
// raises `NoMethodError` (#1400). An `attr_reader :initialize` is one
// such definition (measured on ruby 3.0.2: the reader is private, the
// writer of `attr_accessor :initialize` public).
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

/// Whether `RUBY_AUTO_PRIVATE_METHODS` covers an instance declaration
/// of `name` in a body that is a `class << x` when `in_singleton_class`.
pub(crate) fn ruby_is_auto_private(name: &str, in_singleton_class: bool) -> bool {
    !in_singleton_class && RUBY_AUTO_PRIVATE_METHODS.contains(&name)
}

/// The declarations a Ruby class body has made so far, each a slot with
/// its own public bit, and the re-filing the visibility keywords applied
/// to the ones they named. `K` is what a keyword's argument names: a
/// method with its family for `npm`, an attribute's accessor for `npa`.
pub(crate) struct RubyLedger<K> {
    // Every declaration so far, in order: its own public bit.
    public: Vec<bool>,
    // Every named declaration by name: the slots, and the bit a keyword
    // call last re-filed the name to, with how many of the name's slots
    // it covered. The name is borrowed from the source bytes: Ruby spells
    // the target as a symbol, so identity is in the bytes
    // (grammar-dispatch rule 10).
    by_name: HashMap<K, RubyNameEntry>,
}

// One name's declarations in a Ruby class body and the last re-file
// that reached them.
#[derive(Default)]
struct RubyNameEntry {
    slots: Vec<usize>,
    // `Some((covered, public))`: the first `covered` entries of `slots`
    // were re-filed to `public`, over their own bits.
    refiled: Option<(usize, bool)>,
}

impl<K: Hash + Eq> RubyLedger<K> {
    pub(crate) fn new() -> Self {
        Self {
            public: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    /// The declarations so far, which is the next slot.
    pub(crate) fn len(&self) -> usize {
        self.public.len()
    }

    /// Records one declaration with its own bit, under `name` when the
    /// walk could resolve one. Answers the slot.
    pub(crate) fn declare(&mut self, name: Option<K>, public: bool) -> usize {
        let slot = self.public.len();
        if let Some(name) = name {
            self.by_name.entry(name).or_default().slots.push(slot);
        }
        self.public.push(public);
        slot
    }

    /// Re-files every already-declared slot of `name`. Ruby's `private
    /// :foo` reaches back over the body, and only over the body: a
    /// declaration written *after* the call keeps the bit it was
    /// declared with.
    pub(crate) fn refile(&mut self, name: &K, public: bool) {
        if let Some(entry) = self.by_name.get_mut(name) {
            entry.refiled = Some((entry.slots.len(), public));
        }
    }

    /// Every slot's bit once each name's last re-file is applied over
    /// the slots it covered, in declaration order.
    pub(crate) fn resolved(&self) -> Vec<bool> {
        let mut public = self.public.clone();
        for entry in self.by_name.values() {
            if let Some((covered, bit)) = entry.refiled {
                for &slot in &entry.slots[..covered] {
                    public[slot] = bit;
                }
            }
        }
        public
    }
}

/// Leaves a call whose arguments have all been read, for a consumer
/// that gathered the names they spelled: the keyword, if any, re-files
/// each through `refile`, and what the call returns is handed to the
/// enclosing call when that is what the call is an argument of.
///
/// What a nested visibility call returns is its arguments: `private
/// public def x` demotes `x` because `public` returns its argument —
/// Ruby 3.1+, where `set_visibility` answers `argv[0]` for one argument
/// and an array for several; 3.0 raised a `TypeError` on the nested
/// call — and the outer `private` re-files what it is handed. The
/// class-method pair returns the receiver, not its arguments, so an
/// outer call is handed nothing; nor is it from an ordinary call
/// (`memoize def x`), whose result is the decorator's own and not
/// assumed to be the name.
///
/// Each level of a nested keyword chain re-files every name it is
/// handed and passes them all up, so `D` nested `private(public(…))`
/// around `W` symbols is `D × W` lookups on an input of size `D + W`
/// (batch perf audit: 16,000 × 16,000 took 20 s). The semantics
/// require each governing level to act, real code nests two deep, and
/// no probe reads the shape; recorded, not restructured.
pub(crate) fn ruby_leave_visibility_call<N: Copy>(
    call: RubyOpenCall<Vec<N>>,
    outer: Option<&mut RubyOpenCall<Vec<N>>>,
    mut refile: impl FnMut(N, RubyVisibilityCall),
) {
    let Some(keyword) = call.keyword else {
        return;
    };
    for &name in &call.gathered {
        refile(name, keyword);
    }
    if keyword.targets_singleton {
        return;
    }
    if let Some(outer) = outer {
        outer.gathered.extend(call.gathered);
    }
}
