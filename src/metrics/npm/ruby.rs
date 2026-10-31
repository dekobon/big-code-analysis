//! `Npm` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::npa::{
    RubyVisibility, RubyVisibilityCall, RubyVisibilityEffect, ruby_call_argument_list,
    ruby_declaration_is_public, ruby_method_name, ruby_symbol_name, ruby_visibility_effect,
    ruby_visibility_marker, ruby_wrapped_is_public,
};
use std::collections::HashMap;

use super::*;
use crate::node::Children;

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
// to re-file and, where Ruby returns them, to hand up (`close_call`).
type RubyArgumentNames<'a> = Vec<&'a str>;

// What the walk is inside while it reads a node's children: the body
// or a wrapper whose statements run at class scope — a conditional, a
// loop body, a `begin`, a `case` arm, the statement under a modifier —
// or a call's argument list, where a `def` is a declaration (`private
// def x`, `memoize def x`) and a symbol names one.
enum RubyFrameKind<'a> {
    Statements,
    Arguments {
        // The visibility keyword the call spells, `None` for an
        // ordinary call.
        keyword: Option<RubyVisibilityCall>,
        // What the arguments read so far name, for the keyword to
        // re-file once the list is read, and for the enclosing call to
        // be handed if the keyword returns them.
        names: RubyArgumentNames<'a>,
    },
}

// One node the walk is reading the children of, and those children
// still to read. Each wrapper, call and argument list is a frame on an
// explicit stack rather than a Rust frame, so a body nested or called
// arbitrarily deep costs no stack (#1401 review: 5,000 nested calls
// overflowed the recursive first draft), and each child list is read
// once through a held iterator rather than re-sought per step.
struct RubyFrame<'a> {
    node: Node<'a>,
    children: Children<'a>,
    kind: RubyFrameKind<'a>,
}

// The statement shapes of a Ruby class body, read by the member walk
// below and nothing else of it: which wrappers hold statements that
// run at class scope, and which children of each are those statements.
// Free of the walk's state so the attribute walk can share them
// (FIXME(#1719)).

// Whether `node` is a wrapper the walk reads through for the
// statements under it: a conditional and its arms, a `case` and
// its clauses, a loop and its `do` body, a `begin` with its
// `rescue` / `ensure` clauses, a statement modifier, a
// parenthesised statement list, and the right side of an
// assignment — `x = def b; end` and `@h = private def h; end`
// define `b` and `h` as any bare `def` would (measured), and so do
// both of `a, b = def x; end, def y; end`, whose right side is a
// `right_assignment_list` of them. A `def`'s
// or a block's children are a space of their own and are not read:
// a `def` inside `def inner` or an `items.each do … end` is that
// space's method, as `nom` counts it, not the class's.
//
// FIXME(#1719): the attribute walk, `ruby_walk_class_body` in
// `npa/shared.rs`, still reads the body's direct children only, and
// reads a bare keyword as the marker only there — without the
// expression-position reading or the shadow table the ledger keeps —
// so the two Ruby member walks can disagree on one body's visibility.
// This inventory, `is_statement_of` and `RubyMethodLedger`'s marker
// rules are what it should share.
fn reads_through(kind: u16) -> bool {
    use Ruby::*;

    // role-bypass: allow(Branch, Loop, Catch, CaseArm) — the wrappers
    // whose statements run at class scope, read for their children;
    // not whether they branch, loop or arm a case, which is R07's and
    // the `CaseArm` row's question
    matches!(
        kind.into(),
        If | Unless
            | Elsif
            | Else
            | Then
            | Case
            | CaseMatch
            | When
            | InClause
            | While
            | Until
            | For
            | Do
            | Begin
            | Rescue
            | Ensure
            | IfModifier
            | UnlessModifier
            | WhileModifier
            | UntilModifier
            | RescueModifier
            | RescueModifier2
            | RescueModifier3
            | ParenthesizedStatements
            | Assignment
            | Assignment2
            | OperatorAssignment
            | OperatorAssignment2
            | RightAssignmentList
    )
}

fn is_assignment(kind: u16) -> bool {
    use Ruby::*;

    matches!(
        kind.into(),
        Assignment | Assignment2 | OperatorAssignment | OperatorAssignment2
    )
}

// Whether `child` of a wrapper `parent` is one of its statements
// rather than its condition, value, pattern, header or handler.
//
// Only statement positions are read. Ruby does evaluate an `if`'s
// condition, and a bare `private` there is the marker call unless a
// local shadows it, so `if private … end` demotes what follows in Ruby
// and not here: reading conditions would mean reading every expression
// for a keyword, which this inventory of statement positions does not
// attempt. The grammar exposes each consequence as
// a dedicated node or a field, so the arms pick by role rather than
// by index (grammar-dispatch rule 3): `then` / `else` / `elsif`
// under a conditional, the clauses under a `case`, the `then` of a
// clause or a `rescue`, the `do` under a loop, the `body` field of
// a modifier, and the `right` field of an assignment, whose `left`
// is the target. The `rescue` modifier is read on both sides: its
// `body` runs, and its `handler` runs when the body raises — a
// `def` in either position is a definition at class scope, and the
// walk counts every branch. An `else`, a `then`, a `do`, an
// `ensure`, a `begin` and a parenthesised list hold statements
// directly. `rescue_modifier` is three kind ids for one rule
// (grammar-dispatch rule 1): the parser emits `RescueModifier3` in
// every position measured — statement, assignment right side,
// method body — and the other two are aliases the grammar declares
// that no spelling produced; listed defensively per rule 2, and
// pinned unreachable by `ruby_rescue_modifier_aliases_are_not_emitted`.
//
// The exclusions are chosen by role, and no fixture pins them (the
// perturbation sweep recorded on #1401): a widened arm that read a
// condition, a loop header, a `case` value or a `when` pattern as a
// statement would find, in every valid fixture, either a local that
// shadows the keyword — which the ledger already ignores — or a
// bare keyword that Ruby evaluates as the marker call, which the
// widened arm would then read *correctly* where this arm does not.
// A `when` pattern and a `rescue` class list are wrapped (`pattern`,
// `exceptions`) besides. The arms stay as the statement inventory
// they are; the fixture that would discriminate them is the one
// whose answer they get wrong, and `is_statement_of`'s comment
// above records that gap rather than pinning it.
fn is_statement_of(parent: &Node<'_>, parent_kind: u16, child: &Node<'_>) -> bool {
    use Ruby::*;

    // role-bypass: allow(CaseArm) — which child of a `case` is a clause
    // and which of a clause is its body, for traversal; not whether
    // the arm pays a decision, which is the row's question
    child.is_named()
        && match parent_kind.into() {
            If | Unless | Elsif => matches!(child.kind_id().into(), Then | Else | Elsif),
            Case | CaseMatch => matches!(child.kind_id().into(), When | InClause | Else),
            When | InClause | Rescue => matches!(child.kind_id().into(), Then),
            While | Until | For => matches!(child.kind_id().into(), Do),
            IfModifier | UnlessModifier | WhileModifier | UntilModifier => parent
                .child_by_field_name("body")
                .is_some_and(|body| body.id() == child.id()),
            RescueModifier | RescueModifier2 | RescueModifier3 => true,
            _ if is_assignment(parent_kind) => parent
                .child_by_field_name("right")
                .is_some_and(|right| right.id() == child.id()),
            _ => true,
        }
}

// The methods a Ruby class body has declared so far and what each is
// worth: the running body-wide visibility flag, the keyword names a
// local has shadowed, and the declarations with their public bit.
// Visibility cannot be settled arm-by-arm — `private :foo` demotes a
// method declared *earlier* in the same body — so the tally is taken
// once the whole body has been read (#1255).
struct RubyMethodLedger<'a> {
    code: &'a [u8],
    // `class << x` rather than `class X`. Ruby's automatic-private rule
    // does not reach a singleton class, so the ledger needs to know
    // which of the two it is keeping.
    in_singleton_class: bool,
    visibility: RubyVisibility,
    // The visibility keywords a local of the same name has shadowed:
    // after `private = false`, a bare `private` anywhere later in the
    // body is that variable, and Ruby reads it as the marker call only
    // where no such local is in scope (measured: `x = private` with no
    // local demotes what follows; with the local assigned first it does
    // not). A Ruby local lives to the end of the body it was assigned
    // in, which here is the class body or a wrapper under it — the
    // walk does not unshadow at a wrapper's end, which over-shadows
    // only the body after an assignment made *inside* a branch.
    shadowed: [bool; RubyVisibility::COUNT],
    // Every declaration so far, in order: the position of the body's
    // visibility *epoch* when it was declared and its own public bit.
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
            // Ruby class bodies open in default-public state, whatever
            // the previous body's trailing visibility was.
            visibility: RubyVisibility::Public,
            shadowed: [false; RubyVisibility::COUNT],
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

    // A bare `private` / `public` / `protected` sets the flag — Ruby
    // evaluates it as the marker call wherever it stands — unless a
    // local of that name shadows it. The walk asks this of every node
    // it reads as a statement or a call argument, and of nothing else:
    // a keyword in a condition, a loop header or a `case` value is
    // evaluated by Ruby and not seen here (`is_statement_of` records
    // the gap). Answers whether `node` was one, shadowed or not.
    fn apply_marker(&mut self, node: &Node<'a>) -> bool {
        let Some(marker) = ruby_visibility_marker(node, self.code) else {
            return false;
        };
        if !self.shadowed[marker as usize] {
            self.visibility = marker;
        }
        true
    }

    // Records every visibility keyword's name a binding target binds,
    // so the bare word is a local from here on: the `left` of an
    // assignment or the `pattern` of a `for`, directly (`private = 1`)
    // or inside a destructuring list (`private, other = …`, `(a,
    // private) = …`, `*private = …`), which the grammar nests as
    // `left_assignment_list` / `destructured_left_assignment` /
    // `rest_assignment` holding the identifiers. An `@private = 1` or
    // `obj.private = 2` target is not an `identifier` and binds no
    // local, so a later bare `private` is still the call (measured).
    fn shadow_bindings_of(&mut self, target: &Node<'a>) {
        use Ruby::*;

        let mut stack = vec![*target];
        while let Some(node) = stack.pop() {
            if let Some(marker) = ruby_visibility_marker(&node, self.code) {
                self.shadowed[marker as usize] = true;
                continue;
            }
            if matches!(
                node.kind_id().into(),
                LeftAssignmentList
                    | DestructuredLeftAssignment
                    | RestAssignment
                    | ExceptionVariable
            ) {
                stack.extend(node.children().filter(Node::is_named));
            }
        }
    }

    // Records the locals a statement binds before its body is read: an
    // assignment's `left`, a `for`'s `pattern`, a `rescue` clause's
    // `=> e` variable (an `exception_variable` wrapping the target).
    // All three bind at class scope; a block's parameters bind in the
    // block, a space the walk does not enter.
    fn shadow_statement_bindings(&mut self, statement: &Node<'a>, kind: u16) {
        use Ruby::*;

        let field = if is_assignment(kind) {
            "left"
        } else if kind == For as u16 {
            "pattern"
        } else if kind == Rescue as u16 {
            "variable"
        } else {
            return;
        };
        if let Some(target) = statement.child_by_field_name(field) {
            self.shadow_bindings_of(&target);
        }
    }

    // Records a `method` / `singleton_method` node, taking its
    // visibility from `keyword` when a visibility call wraps it and from
    // the body-wide flag otherwise. Answers the declared name, for the
    // call it sits in to pass on.
    fn declare(
        &mut self,
        method: &Node<'a>,
        singleton: bool,
        keyword: Option<RubyVisibilityCall>,
    ) -> Option<&'a str> {
        let name = ruby_method_name(method, self.code);
        let named_by_keyword = keyword.is_some_and(|kw| kw.governs(singleton));
        let public = keyword.map_or_else(
            || ruby_declaration_is_public(singleton, self.visibility),
            |kw| ruby_wrapped_is_public(kw, singleton, self.visibility),
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

// One pass over a Ruby class body: the ledger it fills, and the chain
// of whichever node's children are being read, moved down and back up
// as the walk enters and leaves a frame so a row's gate reads each node
// under its own parent (#1696) without a chain copy per level.
struct RubyClassBody<'a, 'chain> {
    ledger: RubyMethodLedger<'a>,
    chain: ChildAncestors<'a, 'chain>,
}

impl<'a, 'chain> RubyClassBody<'a, 'chain> {
    fn new(code: &'a [u8], in_singleton_class: bool, chain: ChildAncestors<'a, 'chain>) -> Self {
        Self {
            ledger: RubyMethodLedger::new(code, in_singleton_class),
            chain,
        }
    }

    // Opens a call: a bare keyword call sets the flag and is done; any
    // other call with an argument list is pushed, with the keyword it
    // spells, so its arguments are read as the walk reaches them.
    fn open_call(&mut self, call: &Node<'a>, stack: &mut Vec<RubyFrame<'a>>) {
        let keyword = match ruby_visibility_effect(call, self.ledger.code) {
            Some(RubyVisibilityEffect::Flag(flag)) => {
                self.ledger.visibility = flag;
                return;
            }
            Some(RubyVisibilityEffect::Arguments(keyword)) => Some(keyword),
            None => None,
        };
        // The arguments sit under the call's `argument_list`: each is
        // asked under its own chain, the call's plus the call and the
        // list, so a gate the row grows later reads the right parent.
        let Some(list) = ruby_call_argument_list(call) else {
            return;
        };
        self.chain.descend(call);
        self.chain.descend(&list);
        stack.push(RubyFrame {
            node: *call,
            children: list.children(),
            kind: RubyFrameKind::Arguments {
                keyword,
                names: RubyArgumentNames::new(),
            },
        });
    }

    // Closes a call whose arguments have all been read: the keyword, if
    // any, re-files what they named, and what the call returns is handed
    // to the enclosing call's frame when that is what the parent is.
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
    fn close_call(
        &mut self,
        keyword: Option<RubyVisibilityCall>,
        names: RubyArgumentNames<'a>,
        stack: &mut [RubyFrame<'a>],
    ) {
        self.chain.ascend();
        self.chain.ascend();
        let Some(keyword) = keyword else {
            return;
        };
        let public = keyword.visibility == RubyVisibility::Public;
        for name in &names {
            self.ledger.refile(name, keyword.targets_singleton, public);
        }
        if keyword.targets_singleton {
            return;
        }
        if let Some(RubyFrame {
            kind: RubyFrameKind::Arguments { names: outer, .. },
            ..
        }) = stack.last_mut()
        {
            outer.extend(names);
        }
    }

    // Reads one argument of the call on top of the stack: a `def`
    // declares and names itself to the call, a nested call is opened,
    // and a symbol names a method.
    fn read_argument(&mut self, arg: &Node<'a>, stack: &mut Vec<RubyFrame<'a>>) {
        use crate::roles::ruby::func_opener;
        use Ruby::*;

        let code = self.ledger.code;
        let Some(RubyFrame {
            kind: RubyFrameKind::Arguments { keyword, .. },
            ..
        }) = stack.last()
        else {
            return;
        };
        let keyword = *keyword;
        // An argument is evaluated as any expression is: a bare keyword
        // there is the marker call (`foo(private)` demotes what follows,
        // measured), a binding there (`foo(private = 1)`) shadows it from
        // then on. Read before anything else, as a statement's are.
        if self.ledger.apply_marker(arg) {
            return;
        }
        let kind = arg.kind_id();
        self.ledger.shadow_statement_bindings(arg, kind);
        // A `def` passed to a call (`private def x; end`, `memoize def
        // x; end`) is the `FuncOpener` row's callable, as a bare one is.
        // Which of its two callables decides the singleton bucket is
        // this consumer's reading. A nested call is opened as a frame of
        // its own; the names it returns reach this one when it closes.
        let named: Vec<&'a str> = if func_opener::is_callable(arg, code, self.chain.get()) {
            // role-bypass: allow(FuncOpener) — the singleton bucket
            self.ledger
                .declare(arg, kind == SingletonMethod as u16, keyword)
                .into_iter()
                .collect()
        } else {
            match kind.into() {
                // role-bypass: allow(NullSafe) — a call nested in an argument
                // list, which the row's safe-navigation owner is a kind of;
                // read as the body's own calls are
                Call | Call2 | Call3 | Call4 => return self.open_call(arg, stack),
                // role-bypass: allow(StringLit) — a `%i[]` array names several
                // methods; the row owns it as a literal, a different question
                SymbolArray => arg
                    .children()
                    .filter_map(|sym| ruby_symbol_name(&sym, code))
                    .collect(),
                // A wrapper passed as an argument — `private((def x; end))`,
                // `memoize(begin … end)`, `memoize(x = def y; end)` — holds
                // statements that run at class scope exactly as a bare one
                // does, so the `def`s inside are declared. What the wrapper
                // *evaluates to* — the value of its last statement, which
                // Ruby hands to the call (`private((def a; end; def b;
                // end))` demotes `b` alone, measured) — is not resolved: the
                // `def`s take the body-wide flag and the enclosing keyword
                // re-files nothing through them. Counting them at all is the
                // correction the #1401 review asked for; their visibility
                // under a wrapping keyword is the approximation recorded in
                // `ruby_def_in_a_wrapper_argument_counts`.
                _ if reads_through(kind) => return self.open_wrapper(arg, stack),
                _ => ruby_symbol_name(arg, code).into_iter().collect(),
            }
        };
        if let Some(RubyFrame {
            kind: RubyFrameKind::Arguments { names, .. },
            ..
        }) = stack.last_mut()
        {
            names.extend(named);
        }
    }

    // Reads one statement of the body or of a wrapper, under the chain
    // of its siblings: a bare visibility marker sets the flag, a `def`
    // declares, a call is opened for its arguments, and a wrapper is
    // pushed so its statements are read in turn.
    fn read_statement(&mut self, child: &Node<'a>, stack: &mut Vec<RubyFrame<'a>>) {
        use crate::roles::ruby::func_opener;
        use Ruby::*;

        // A bare `private` is the marker call wherever an expression is
        // evaluated — `x = private` and `y = public` set the flag
        // (measured) — unless a local of that name shadows it. The
        // assignment or `for` that creates the local is read as a
        // wrapper (for the `def` its right side or body may hold), and
        // shadows from its target before that is read.
        if self.ledger.apply_marker(child) {
            return;
        }
        let kind = child.kind_id();
        self.ledger.shadow_statement_bindings(child, kind);
        // A `def` — the `FuncOpener` row's callable — declares a method;
        // the row's two callables differ in the bucket (`def self.` is
        // the singleton), which is this consumer's reading.
        if func_opener::is_callable(child, self.ledger.code, self.chain.get()) {
            // role-bypass: allow(FuncOpener) — the singleton bucket
            self.ledger
                .declare(child, kind == SingletonMethod as u16, None);
            return;
        }
        match kind.into() {
            // role-bypass: allow(NullSafe) — the visibility macro calls and
            // the decorator-style ones, which the row's safe-navigation
            // owner is a kind of; each is told by its name
            Call | Call2 | Call3 | Call4 => self.open_call(child, stack),
            _ if reads_through(kind) => self.open_wrapper(child, stack),
            _ => {}
        }
    }

    // Opens a statement wrapper: its statements are read as the walk
    // reaches them, under its own chain.
    fn open_wrapper(&mut self, wrapper: &Node<'a>, stack: &mut Vec<RubyFrame<'a>>) {
        self.chain.descend(wrapper);
        stack.push(RubyFrame {
            node: *wrapper,
            children: wrapper.children(),
            kind: RubyFrameKind::Statements,
        });
    }

    // Reads every statement of the body, of each wrapper under it and
    // of each call's argument list, in source order. The body's direct
    // children are all statements; a wrapper's are the ones
    // `is_statement_of` picks; an argument list's are its named
    // children.
    fn walk(&mut self, body: &Node<'a>) {
        let floor = self.chain.get().depth();
        let mut stack = vec![RubyFrame {
            node: *body,
            children: body.children(),
            kind: RubyFrameKind::Statements,
        }];
        // The top frame is taken off to read its next child and put
        // back before the child is, so a reader that opens a frame of
        // its own stacks it on top; a frame with no child left closes.
        while let Some(mut frame) = stack.pop() {
            let parent = frame.node;
            let parent_kind = parent.kind_id();
            let arguments = matches!(frame.kind, RubyFrameKind::Arguments { .. });
            let child = frame.children.find(|child| {
                if arguments || parent.id() == body.id() {
                    child.is_named()
                } else {
                    is_statement_of(&parent, parent_kind, child)
                }
            });
            let Some(child) = child else {
                self.close_frame(frame, body, &mut stack);
                continue;
            };
            stack.push(frame);
            if arguments {
                self.read_argument(&child, &mut stack);
            } else {
                self.read_statement(&child, &mut stack);
            }
        }
        // Every frame descended is ascended on its pop: a frame kind
        // that forgot would leave every later gate reading the wrong
        // parent, which nothing else can see.
        debug_assert_eq!(
            self.chain.get().depth(),
            floor,
            "the member walk left the chain off the body's floor"
        );
    }

    // Leaves a frame whose children are all read: a call closes with
    // its keyword's re-filing, a wrapper gives back the chain level it
    // took, and the body — the one frame the chain was built for —
    // took none.
    fn close_frame(&mut self, done: RubyFrame<'a>, body: &Node<'a>, stack: &mut [RubyFrame<'a>]) {
        match done.kind {
            RubyFrameKind::Arguments { keyword, names } => {
                self.close_call(keyword, names, stack);
            }
            RubyFrameKind::Statements => {
                if done.node.id() != body.id() {
                    self.chain.ascend();
                }
            }
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
        let mut body = RubyClassBody::new(
            code,
            matches!(parent_kind, SingletonClass),
            ancestors.of_children(node),
        );
        body.walk(node);

        let (count, public) = body.ledger.tally();
        stats.class_nm += count;
        stats.class_npm += public;
    }
}
