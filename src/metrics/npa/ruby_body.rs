//! The walk over a Ruby class body that `Npm` and `Npa` share (#1719):
//! which statements the body holds — through the wrappers they sit in
//! and the argument lists of the calls among them — and the visibility
//! state those statements set as they run. Each consumer reads the
//! statements and arguments it is handed for its own declarations;
//! this module counts nothing.
//!
//! The protocol is three events. A *statement* is a node the body or a
//! wrapper runs at class scope; an *argument* is a named child of a
//! call's argument list, handed with the call it belongs to. Both are
//! read before the walk enters them, so a call is entered at the event
//! that reaches it, and its arguments follow as events of their own.
//! *Leaving* a call comes once its arguments are all read, which is
//! when a consumer that acts on what they named — `npm`'s re-filing of
//! `private :foo` — can act. A bare visibility marker is the walk's own
//! and reaches no consumer.
//!
//! `npm` (`npm/ruby.rs`) declares the `def`s it is handed; `npa`
//! (`npa/ruby.rs`) counts the `attr_*` calls and the instance- and
//! class-variable assignments. Because both read one walk, they agree
//! on which statements a body holds and on the visibility each runs
//! under (#1719; grammar-dispatch rule 7).
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::node::Children;

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
fn reads_through(kind: u16) -> bool {
    use Ruby::*;

    // role-bypass: allow(Branch, Switch, Loop, Catch, ElseIf, CaseArm, DocTarget) — the
    // wrappers whose statements run at class scope, read for their
    // children; not whether they branch, switch, loop, chain or arm a
    // case, which is the control-flow rows' and the `CaseArm` row's
    // question (#1697), nor whether an assignment takes a doc comment
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

    // role-bypass: allow(DocTarget) — an assignment statement for the
    // class-scope ledger; not whether it takes a doc comment
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
// shadows the keyword — which the walk already ignores — or a
// bare keyword that Ruby evaluates as the marker call, which the
// widened arm would then read *correctly* where this arm does not.
// A `when` pattern and a `rescue` class list are wrapped (`pattern`,
// `exceptions`) besides. The arms stay as the statement inventory
// they are; the fixture that would discriminate them is the one
// whose answer they get wrong, and the comment above records that
// gap rather than pinning it.
fn is_statement_of(parent: &Node<'_>, parent_kind: u16, child: &Node<'_>) -> bool {
    use Ruby::*;

    // role-bypass: allow(Branch, Switch, Loop, ElseIf, Catch, CaseArm) — which
    // child of a wrapper is its clause and which of a clause is its
    // body, for traversal; not whether the wrapper branches or the arm
    // pays a decision, which is the rows' question (#1697)
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

/// What the walk does with a node once a consumer has read it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RubyBodyShape {
    /// A `def` — the `FuncOpener` row's callable. Its body is a space
    /// of its own and is not read.
    Definition,
    /// A call, whose arguments are read next; a parenthesised bare
    /// keyword (`private()`) has none and sets the flag instead.
    Call,
    /// A wrapper whose statements run at class scope, read next.
    Wrapper,
    /// Anything else, which the walk does not enter.
    Leaf,
}

/// A call whose arguments the walk is reading.
pub(crate) struct RubyOpenCall<T> {
    /// The visibility keyword the call spells, `None` for an ordinary
    /// call.
    pub(crate) keyword: Option<RubyVisibilityCall>,
    /// What the consumer has gathered from the arguments read so far.
    pub(crate) gathered: T,
}

/// A reader of the statements and arguments a Ruby class body holds.
/// See the module documentation for the order of the events.
pub(crate) trait RubyBodyConsumer<'a> {
    /// What the consumer gathers over one call's arguments, for
    /// [`leave_call`](Self::leave_call).
    type Gathered: Default;

    /// A statement of the body or of a wrapper under it, other than a
    /// bare visibility marker, before the walk enters it.
    fn statement(
        &mut self,
        statement: &Node<'a>,
        shape: RubyBodyShape,
        body: &mut RubyBodyState<'a, '_>,
    );

    /// One argument of `call`, other than a bare visibility marker,
    /// before the walk enters it.
    fn argument(
        &mut self,
        argument: &Node<'a>,
        shape: RubyBodyShape,
        call: &mut RubyOpenCall<Self::Gathered>,
        body: &mut RubyBodyState<'a, '_>,
    );

    /// `call`'s arguments are all read. `outer` is the call whose
    /// argument it was, if it was one. A consumer that gathers nothing
    /// from a call's arguments has nothing to do here.
    fn leave_call(
        &mut self,
        _call: RubyOpenCall<Self::Gathered>,
        _outer: Option<&mut RubyOpenCall<Self::Gathered>>,
    ) {
    }
}

// What the walk is inside while it reads a node's children: the body
// or a wrapper whose statements run at class scope, or a call's
// argument list.
enum RubyFrameKind<T> {
    Statements,
    Arguments(RubyOpenCall<T>),
}

// One node the walk is reading the children of, and those children
// still to read. Each wrapper, call and argument list is a frame on an
// explicit stack rather than a Rust frame, so a body nested or called
// arbitrarily deep costs no stack (#1401 review: 5,000 nested calls
// overflowed the recursive first draft), and each child list is read
// once through a held iterator rather than re-sought per step.
struct RubyFrame<'a, T> {
    node: Node<'a>,
    children: Children<'a>,
    kind: RubyFrameKind<T>,
}

/// The state one pass over a Ruby class body carries: the body-wide
/// visibility flag, the keyword names a local has shadowed, and the
/// chain of whichever node's children are being read, moved down and
/// back up as the walk enters and leaves a frame so a row's gate reads
/// each node under its own parent (#1696) without a chain copy per
/// level.
pub(crate) struct RubyBodyState<'a, 'chain> {
    code: &'a [u8],
    chain: ChildAncestors<'a, 'chain>,
    visibility: RubyVisibility,
    // The visibility keywords a local of the same name has shadowed:
    // after `private = false`, a bare `private` anywhere later in the
    // body is that variable, and Ruby reads it as the marker call only
    // where no such local is in scope (measured: `x = private` with no
    // local demotes what follows; with the local assigned first it does
    // not). A Ruby local exists from its binding to the end of its scope,
    // decided at parse time, and the only scope here is the class body:
    // none of the wrappers the walk reads through opens one, and the
    // scopes that do (a `def`, a block, a nested class) are not read. A
    // binding inside a branch therefore shadows the rest of the body
    // whether or not the branch runs (measured: `if false; private = 1;
    // end; private` leaves what follows public), and never unshadowing
    // is exact.
    shadowed: [bool; RubyVisibility::COUNT],
}

impl<'a, 'chain> RubyBodyState<'a, 'chain> {
    /// Reads every statement of `body` — a class or singleton-class
    /// body, whose children `chain` describes — into `consumer`.
    pub(crate) fn walk<C: RubyBodyConsumer<'a>>(
        code: &'a [u8],
        chain: ChildAncestors<'a, 'chain>,
        body: &Node<'a>,
        consumer: &mut C,
    ) {
        let mut state = Self {
            code,
            chain,
            // Ruby class bodies open in default-public state, whatever
            // the previous body's trailing visibility was.
            visibility: RubyVisibility::Public,
            shadowed: [false; RubyVisibility::COUNT],
        };
        state.read_body(body, consumer);
    }

    /// The body-wide visibility flag where the walk stands.
    pub(crate) fn visibility(&self) -> RubyVisibility {
        self.visibility
    }

    /// Answers `read` with the chain of `node`'s children, for a gate
    /// on a child the walk does not read itself (an assignment's
    /// target). `node` is the statement or argument being read.
    pub(crate) fn below<R>(
        &mut self,
        node: &Node<'a>,
        read: impl FnOnce(Ancestors<'a, '_>) -> R,
    ) -> R {
        self.chain.descend(node);
        let answer = read(self.chain.get());
        self.chain.ascend();
        answer
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
    //
    // Only nodes the walk reads are asked, so a binding anywhere else
    // creates the local unseen: in a condition, a `case` value, an
    // operand or a literal element (`if (private = false)`, `x =
    // [private = 1]`), or by a pattern match or a regexp named capture.
    // A later bare keyword is then read as the marker though Ruby reads
    // the local (measured on ruby 3.0.2); the same statement-position
    // limit `is_statement_of` records.
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

    // What the walk will do with `node` once it is read: a `def` — the
    // `FuncOpener` row's callable — is a space of its own; a call has
    // arguments to read; a wrapper has statements to read.
    fn shape(&self, node: &Node<'a>, kind: u16) -> RubyBodyShape {
        use crate::roles::ruby::{call, func_opener};

        if func_opener::is_callable(node, self.code, self.chain.get()) {
            return RubyBodyShape::Definition;
        }
        // The `Call` row is ungated, so its kind table is the whole
        // answer: the visibility macros and the decorator-style calls are
        // each told by name once the walk reads the call.
        if call::lists_kind(kind) {
            RubyBodyShape::Call
        } else if reads_through(kind) {
            RubyBodyShape::Wrapper
        } else {
            RubyBodyShape::Leaf
        }
    }

    // Reads one statement of the body or of a wrapper, or one argument
    // of the call on top of the stack, under the chain of its siblings:
    // a bare visibility marker sets the flag, anything else is handed to
    // the consumer and then entered if it is a call or a wrapper.
    //
    // A bare `private` is the marker call wherever an expression is
    // evaluated — `x = private` and `y = public` set the flag
    // (measured), and so does `foo(private)` — unless a local of that
    // name shadows it. The assignment, `for` or `rescue` that creates
    // the local is read as a wrapper (for the `def` its right side or
    // body may hold), and shadows from its target before that is read;
    // a binding in an argument (`foo(private = 1)`) shadows as well.
    fn read<C: RubyBodyConsumer<'a>>(
        &mut self,
        node: &Node<'a>,
        consumer: &mut C,
        stack: &mut Vec<RubyFrame<'a, C::Gathered>>,
    ) {
        if self.apply_marker(node) {
            return;
        }
        let kind = node.kind_id();
        self.shadow_statement_bindings(node, kind);
        let shape = self.shape(node, kind);
        match stack.last_mut() {
            Some(RubyFrame {
                kind: RubyFrameKind::Arguments(call),
                ..
            }) => consumer.argument(node, shape, call, self),
            _ => consumer.statement(node, shape, self),
        }
        match shape {
            RubyBodyShape::Call => self.open_call(node, stack),
            RubyBodyShape::Wrapper => self.open_wrapper(node, stack),
            RubyBodyShape::Definition | RubyBodyShape::Leaf => {}
        }
    }

    // Opens a call: a bare keyword call sets the flag and is done; any
    // other call with an argument list is pushed, with the keyword it
    // spells, so its arguments are read as the walk reaches them.
    fn open_call<T: Default>(&mut self, call: &Node<'a>, stack: &mut Vec<RubyFrame<'a, T>>) {
        let keyword = match ruby_visibility_effect(call, self.code) {
            Some(RubyVisibilityEffect::Flag(flag)) => {
                self.visibility = flag;
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
            kind: RubyFrameKind::Arguments(RubyOpenCall {
                keyword,
                gathered: T::default(),
            }),
        });
    }

    // Opens a statement wrapper: its statements are read as the walk
    // reaches them, under its own chain.
    fn open_wrapper<T>(&mut self, wrapper: &Node<'a>, stack: &mut Vec<RubyFrame<'a, T>>) {
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
    fn read_body<C: RubyBodyConsumer<'a>>(&mut self, body: &Node<'a>, consumer: &mut C) {
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
            let arguments = matches!(frame.kind, RubyFrameKind::Arguments(_));
            let child = frame.children.find(|child| {
                if arguments || parent.id() == body.id() {
                    child.is_named()
                } else {
                    is_statement_of(&parent, parent_kind, child)
                }
            });
            let Some(child) = child else {
                self.close_frame(frame, body, consumer, &mut stack);
                continue;
            };
            stack.push(frame);
            self.read(&child, consumer, &mut stack);
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

    // Leaves a frame whose children are all read: a call gives back the
    // two chain levels it took and is handed to the consumer with the
    // call it is an argument of, a wrapper gives back the level it
    // took, and the body — the one frame the chain was built for —
    // took none.
    fn close_frame<C: RubyBodyConsumer<'a>>(
        &mut self,
        done: RubyFrame<'a, C::Gathered>,
        body: &Node<'a>,
        consumer: &mut C,
        stack: &mut [RubyFrame<'a, C::Gathered>],
    ) {
        match done.kind {
            RubyFrameKind::Arguments(call) => {
                self.chain.ascend();
                self.chain.ascend();
                let outer = match stack.last_mut() {
                    Some(RubyFrame {
                        kind: RubyFrameKind::Arguments(outer),
                        ..
                    }) => Some(outer),
                    _ => None,
                };
                consumer.leave_call(call, outer);
            }
            RubyFrameKind::Statements => {
                if done.node.id() != body.id() {
                    self.chain.ascend();
                }
            }
        }
    }
}
