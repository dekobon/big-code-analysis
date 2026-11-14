//! The C++ family's reading of a class member's declarators: whether a
//! member declares a function (`cpp_declares_function`, asked by `Npm`)
//! and how many data members it declares (`cpp_field_data_members`,
//! `cpp_recovered_declaration_data_members` and
//! `cpp_template_declares_data_member`, asked by `Npa`), including what
//! the pinned grammars' error recovery makes of a pointer to member.
//!
//! One module for both consumers, so the two readings stay beside each
//! other. Every helper matches on node-kind names, so it serves the
//! upstream `Cpp` grammar and the Mozilla `Mozcpp` fork alike.
#![allow(clippy::wildcard_imports)]

use super::*;
use crate::node::Children;

// Name-based for cross-grammar correctness (see `cpp_declarator_names`).
//
// A `function_definition` child is accepted outright rather than
// looked into: it *is* a function, whatever declarator it carries.
// Looking into it would miss a conversion operator with an inline body,
// whose declarator is an `operator_cast` and not a
// `function_declarator` (#1258).
//
// A conversion operator declared *without* a body has no
// `function_definition` to accept either — it parses as a bare
// `declaration > operator_cast`, or `template_declaration >
// declaration > operator_cast` in the templated form, with only an
// `abstract_function_declarator` underneath. `operator_cast` is
// therefore matched in its own right; matching the
// `abstract_function_declarator` instead would also claim ordinary
// function-pointer type positions (#1298).
//
// A `function_declarator`, by contrast, is *not* accepted outright:
// the same node spells two opposite things. In a clean parse only its
// `declarator` field tells them apart: a name (`realMethod()`) or an
// `operator_name` (`operator->()`) is a member function; a
// `parenthesized_declarator` may be either, so its children are asked
// as `DeclaratorQuestion::Parenthesized`, which reads each with
// `cpp_parenthesized_child_verdict` (#1300). One holding a parse error
// may instead be the recovery of a member-pointer data member, which
// is read from its parameter lists as described below (#1807, #1885).
//
// Neither `function_definition` nor `operator_cast` can be reached
// from `Npa`'s `field_declaration` call site: per both grammars'
// `node-types.json`, neither `field_declaration` nor the
// `pointer_declarator` / `reference_declarator` wrappers this looks
// through can carry a `function_definition`, an `operator_cast`, or a
// `declaration` — so there is no path to either at any depth. `Npa`'s
// other call site, `cpp_template_declares_data_member`, hands over the
// `declaration` under a `template_declaration`, where `operator_cast`
// is live (`template<class T> operator T();`) and is what declines the
// templated conversion operator as a data member. `function_definition`
// stays live only for `Npm`'s `template_declaration` children, because
// that helper hands over the `declaration` payload alone and never the
// wrapper.
//
// Two of the arms read what the grammars' error recovery makes of a
// non-templated member pointer at class scope (#1807, #1885), shapes the
// `cpp_recovered_*` helpers below describe. The child walk tracks
// whether the previous non-comment sibling was the recovered `:` — an
// `ERROR` spelling that one token — which is what tells the recovered
// `bitfield_clause` of `int S::*getPd();` from a genuine bit-field's.
// The flag belongs to `node`'s own child list: a nested question walks
// a child's list and starts its own, and a `bitfield_clause` is only
// ever a `field_declaration`'s direct child, so no nested question can
// meet one.
//
// Every arm that looks below its child asks a further question rather
// than calling back in, and `cpp_answer` keeps the open questions on a
// heap stack: how deep the declarators these readers follow nest is the
// source's choice — one level per parenthesis in `T (S::*const
// (S::*const g(int))(int));` — so it costs no call depth (#1829).
pub(crate) fn cpp_declares_function(node: &Node) -> bool {
    cpp_answer(DeclaratorQuestion::any_child(node))
}

// The recovery's own `ERROR`: one holding a `:` token first. An `ERROR`
// over anything else beside a bit-field — a macro, a GNU attribute —
// is a genuine bit-field's, and so is a clean sibling.
fn cpp_is_recovered_colon(node: &Node) -> bool {
    cpp_error_opens_with(node, ":")
}

// An `ERROR` whose first child is the `token` the recovery stranded.
fn cpp_error_opens_with(node: &Node, token: &str) -> bool {
    node.is_error()
        && node
            .children()
            .next()
            .is_some_and(|first| first.kind() == token)
}

fn cpp_child_declares_function(child: &Node, after_colon: bool) -> bool {
    match cpp_child_verdict(child, after_colon) {
        Verdict::Known(answer) => answer,
        Verdict::Ask(question) => cpp_answer(question),
    }
}

// A declaration specifier only a function takes: `virtual`, `explicit`
// (with or without its condition) and `consteval`, which the grammars
// spell as a `type_qualifier`. A class-scope macro call one precedes
// (`virtual FOO(N::*x);`, `explicit CTOR(S::*x);`) is the method it
// expands to, whatever its argument reads as. `static`, `inline` and
// `constexpr` precede a data member too (`static inline T (S::*pe) =
// nullptr;`) and decide nothing. Both consumers ask this one test, so
// neither reads such a declaration as data.
fn cpp_is_function_only_specifier(child: &Node) -> bool {
    match child.kind() {
        "virtual" | "explicit_function_specifier" => true,
        "type_qualifier" => child
            .children()
            // role-bypass: allow(Modifier) — which qualifier only a function
            // takes; not whether the token is a modifier
            .any(|token| token.kind() == "consteval"),
        _ => false,
    }
}

fn cpp_child_verdict<'a>(child: &Node<'a>, after_colon: bool) -> Verdict<'a> {
    if cpp_is_function_only_specifier(child) {
        return Verdict::Known(true);
    }
    match child.kind() {
        "function_declarator" => match cpp_parenthesized_name(child) {
            Some(inner) => Verdict::Ask(DeclaratorQuestion::Parenthesized(inner.children())),
            // Any other `function_declarator` may be the recovery of a
            // parenthesised member-pointer data member, whose return
            // type became the function's name and whose member pointer
            // became a parameter list: one directly under another for
            // `static constexpr int (S::*fp)(int) = nullptr;` (#1807),
            // or alone, named by the type, for `T (S::*pd);` (#1885).
            // A declarator with no recovered member pointer — every
            // ordinary method — is a function, and so is a comma list
            // with a method among its declarators, whose data
            // declarators are the attribute consumer's through
            // `cpp_recovered_declaration_data_members`. The recovery
            // always leaves an `ERROR` or a MISSING name slot below the
            // declarator, which `has_error` reads off the subtree's
            // cached error cost, so a clean one is answered unwalked.
            None if !child.has_error() => Verdict::Known(true),
            None => Verdict::Ask(DeclaratorQuestion::RecoveredChain {
                pointers: Box::new(cpp_recovered_member_pointers(child)),
                seen: false,
            }),
        },
        "function_definition" | "operator_cast" => Verdict::Known(true),
        // A parenthesised value is a member function template's parameter
        // list, read by the grammar as an expression list when every
        // parameter parses as one: `template<class T> void connect(T* obj,
        // void (T::*slot)());` is `init_declarator(connect,
        // argument_list(…))`. A member-declarator takes only a
        // brace-or-equal-initializer ([class.mem.general]), so at class
        // scope, the only scope these helpers read, that value is never
        // an initialiser.
        "init_declarator"
            if child
                .child_by_field_name("value")
                .is_some_and(|value| value.kind() == "argument_list") =>
        {
            Verdict::Known(true)
        }
        // Look through declarator wrappers that can sit above the
        // function_declarator (`Foo* operator->()`,
        // `template<...> T fn();`, constructor / destructor
        // `declaration`s inside a class body). `init_declarator` is the
        // deleted templated member `template<class T> void f(T) =
        // delete;` — the one `= …` a function declarator takes, which
        // the grammar reads as an initialiser only in the templated
        // form (the plain `void f(int) = delete;` is a
        // `function_definition` with a `delete_method_clause`). The
        // batch review found the attribute consumer counting it as a
        // data member once #1299 taught it the `template_declaration`
        // payload. A field's own initialiser is its `default_value`
        // field, never an `init_declarator`, so the `field_declaration`
        // call site is unaffected.
        "pointer_declarator" | "reference_declarator" | "declaration" | "init_declarator" => {
            Verdict::Ask(DeclaratorQuestion::any_child(child))
        }
        // A member pointer outside parentheses: `template<class T>
        // static int S::*getPd();` and, under a `pointer_declarator`,
        // `template<class T> int (*S::*getPp())(T);` are methods
        // returning one (#1718). Any other qualified name — the `std::`
        // of a field's type among them — declares no function.
        "qualified_identifier" => cpp_member_pointer_verdict(child, false),
        // The unparenthesised non-templated member pointer, which the
        // grammars recover as a bit-field (#1807).
        "bitfield_clause" => cpp_recovered_bitfield_verdict(child, after_colon),
        _ => Verdict::Known(false),
    }
}

// The `parenthesized_declarator` naming a `function_declarator`, which
// `cpp_parenthesized_child_verdict` reads; one named any other way is
// read by `cpp_recovered_member_pointers`. Both consumers split on this
// one test, so neither reads a recovered member pointer the other skips:
// only source that is not C++ (`(f)(S::*p);`) puts one beside a
// parenthesised name, and it is then a method and not also data.
fn cpp_parenthesized_name<'a>(declarator: &Node<'a>) -> Option<Node<'a>> {
    declarator
        .child_by_field_name("declarator")
        .filter(|inner| inner.kind() == "parenthesized_declarator")
}

// Whether the member pointer a qualified name spells declares a
// function, and `without_pointer` when it spells none.
fn cpp_member_pointer_verdict<'a>(qualified: &Node<'a>, without_pointer: bool) -> Verdict<'a> {
    cpp_member_pointer(qualified).map_or(Verdict::Known(without_pointer), |pointer| {
        Verdict::Ask(DeclaratorQuestion::any_child(&pointer))
    })
}

// What both grammars make of `int S::*getPd();` at class scope: a
// `field_declaration` whose `_field_declarator` admits no qualified
// name, so the scope `S` is read as the field, the first `:` as an
// `ERROR`, and `:*getPd()` as a `bitfield_clause` holding the width
// expression `*getPd()` — a `pointer_expression` over a
// `call_expression`. A data member (`int S::*pd;`) is the same with an
// `identifier` under the pointer; an initialiser or pure-virtual
// `= 0` wraps the pointer in an `assignment_expression`; a trailing
// `const` moves the `ERROR` inside the clause, around the pointer; and a
// nested scope (`int N::T::*getPt();`) leaves `T::*getPt()` as the
// `qualified_identifier` → `pointer_type_declarator` shape
// `cpp_member_pointer` reads. The recovery is the pinned grammars'
// (`tree-sitter-cpp` in the root `Cargo.toml`, and the Mozcpp fork of
// it); `*_member_pointer_recovery_shapes_are_still_emitted` in `npm`'s
// tests names the day it stops.
//
// A genuine bit-field's width can spell the same expression —
// `int y : *f();` with a `constexpr` `f` is valid — and its clause
// neither follows a recovered `:` nor opens with one, so the clause
// counts only when one of those holds. Any other `ERROR` beside the
// clause — a macro or a GNU attribute between the name and the width —
// is a genuine bit-field's too. Comments are `extra` nodes and can sit
// between any two of these.
fn cpp_recovered_bitfield_verdict<'a>(clause: &Node<'a>, after_colon: bool) -> Verdict<'a> {
    let Some(expression) = cpp_recovered_member_pointer_in_bitfield(clause, after_colon) else {
        return Verdict::Known(false);
    };
    if expression.kind() == "qualified_identifier" {
        return cpp_member_pointer_verdict(&expression, false);
    }
    // role-bypass: allow(Call) — whether the recovered width expression is a
    // call, which is the only thing telling `*getPd()` from `*pd`
    Verdict::Known(
        expression.kind() == "pointer_expression"
            && expression
                .child_by_field_name("argument")
                .is_some_and(|argument| argument.kind() == "call_expression"),
    )
}

// The member-pointer expression a recovered `bitfield_clause` holds —
// the pointer itself, or the left side of its initialiser — and `None`
// for a genuine bit-field, whose clause neither follows a recovered `:`
// nor opens with one. After the recovered `:` the pointer may itself
// sit in a second `ERROR` (`int S:: /* c */ *get() noexcept;` puts a
// trailing specifier's `ERROR` around it), which is unwrapped; a clause
// that follows no recovered `:` is read only when it opens with one.
fn cpp_recovered_member_pointer_in_bitfield<'a>(
    clause: &Node<'a>,
    after_colon: bool,
) -> Option<Node<'a>> {
    // role-bypass: allow(Comment) — stepping over a comment inside the
    // clause; not whether the node is one
    let payload = clause
        .children()
        .find(|child| child.is_named() && child.kind() != "comment")?;
    if !after_colon && !cpp_is_recovered_colon(&payload) {
        return None;
    }
    let expression = if payload.is_error() {
        payload.children().find(Node::is_named)?
    } else {
        payload
    };
    if expression.kind() == "assignment_expression" {
        return expression.child_by_field_name("left");
    }
    Some(expression)
}

// What both grammars make of a parenthesised member pointer whose
// return type is read as an identifier at class scope: a
// constructor-shaped `declaration` whose `function_declarator` is named
// by that return type, with `(S::*pd)` read as its parameter list. A
// user type does it alone (`T (S::*pd);`, `N::Q`, `V<int>`, #1885), and
// so do two or more declaration specifiers before a primitive (`static
// inline int (S::*pe) = nullptr;`). A member-function pointer (`static
// constexpr int (S::*fp)(int) = nullptr;`, #1807) nests a further
// `function_declarator` per parameter list, and a comma list one per
// declarator. The member pointer is then a `parameter_declaration`
// whose `type` is a `qualified_identifier` ending in a
// `type_identifier` the grammar could not read cleanly: `S::*fp` leaves
// the `*` as an `ERROR` beside the name, and `S::*const fp` leaves a
// zero-width name with the `*const fp` as a `pointer_declarator` beside
// it. A method returning one — `(S::*get())` — adds an
// `abstract_function_declarator` to the parameter, and `(S::*const
// get())` a `function_declarator` under the `pointer_declarator`. One
// specifier before a primitive (`static int (S::*fp)(int);`) recovers
// differently, into the parenthesised shape
// `cpp_parenthesized_child_verdict` reads, and the templated form
// parses cleanly (#1718). The recovery is the pinned grammars';
// `*_member_pointer_recovery_shapes_are_still_emitted` in `npm`'s tests
// names the day it stops.
//
// This yields the recovered member pointers among the parameters of a
// `function_declarator` and of each one nested in its `declarator`
// field, outermost first. The chain is walked by an iterator so a long
// comma list cannot deepen the stack.
//
// The recovery puts each member pointer alone in its level's parameter
// list (a comma list nests one level per declarator) and puts nothing a
// function declarator alone takes on that level: `int (S::*fc)() const`
// puts the `const` on the outer level. A recovered pointer beside a
// second parameter or on a qualified level is a class-scope macro
// call's argument (`DEFINE_GETTER(int, S::*x);`, `FOO(N::*x) override;`),
// and the call stays the method it reads as.
fn cpp_recovered_member_pointers<'a>(outer: &Node<'a>) -> impl Iterator<Item = Node<'a>> + use<'a> {
    std::iter::successors(Some(*outer), |level| {
        level.child_by_field_name("declarator")
    })
    .take_while(|level| level.kind() == "function_declarator")
    .filter(|level| !cpp_level_takes_function_syntax(level))
    .filter_map(|level| level.child_by_field_name("parameters"))
    .filter_map(|list| cpp_sole_parameter(&list))
    .filter(|parameter| {
        parameter.kind() == "parameter_declaration" && cpp_is_recovered_member_pointer(parameter)
    })
}

// Whether a `function_declarator` carries a child only a function's
// declarator takes (`node-types.json` lists these beside the
// attributes and the GNU asm label, which a variable takes too), or is
// named by a destructor, which no member's type spells: `~DTOR(S::*x);`
// is the destructor the macro expands to.
fn cpp_level_takes_function_syntax(level: &Node) -> bool {
    level.children().any(|child| {
        matches!(
            child.kind(),
            "destructor_name"
                | "type_qualifier"
                | "ref_qualifier"
                | "virtual_specifier"
                | "noexcept"
                | "throw_specifier"
                | "trailing_return_type"
                | "requires_clause"
        )
    })
}

// The parameter of a list that holds exactly one. The Mozcpp fork reads
// an annotation macro after the declarator as a `macro_annotation`
// inside the list, after its `)`. One that only a variable takes
// (`T (S::*pc) MOZ_INIT_OUTSIDE_CTOR = nullptr;`) is no parameter and is
// stepped over as a comment is. Any other stays counted, as every one
// was before the recovery was read: `FOO(S::*p) MOZ_CAN_RUN_SCRIPT;` is
// then no lone member pointer, and the call stays the method it reads
// as.
fn cpp_sole_parameter<'a>(list: &Node<'a>) -> Option<Node<'a>> {
    // role-bypass: allow(Comment) — stepping over a comment between two
    // parameters; not whether the node is one
    let mut parameters = list.children().filter(|child| {
        child.is_named() && child.kind() != "comment" && !cpp_is_variable_only_annotation(child)
    });
    let sole = parameters.next()?;
    parameters.next().is_none().then_some(sole)
}

// A Mozcpp `macro_annotation` that only a variable takes, told by its
// one token: the fork gives each annotation macro a token of its own.
// Mozilla's `mfbt/Attributes.h` documents these three for a class
// member or a pointer declaration and for nothing else. Two more that a
// variable can carry are left out, because a function carries them too
// and they prove nothing: `MOZ_MAYBE_UNUSED`, documented for a
// function, and `MOZ_NORETURN_PTR`, the `noreturn` of a function
// pointer. A data member under either still reads as a method on this
// grammar (pinned, FIXME(#1829)). `MOZ_UNSAFE_REF(…)` is a pointer's
// too, but takes an argument: the grammars recover a member under it
// in other shapes, which this does not read.
fn cpp_is_variable_only_annotation(node: &Node) -> bool {
    node.kind() == "macro_annotation"
        && node.children().next().is_some_and(|name| {
            matches!(
                name.kind(),
                "MOZ_INIT_OUTSIDE_CTOR" | "MOZ_NON_OWNING_REF" | "MOZ_OWNING_REF"
            )
        })
}

// Whether a parameter is a recovered member pointer that names a
// member. A clean parameter — `int`, or `S::T *arg` with a written type
// name and no stray token — is not. The name slot is the innermost
// `qualified_identifier`'s `type_identifier`; it marks a recovery when
// the grammar invented it (zero-width, tested by width rather than
// `is_missing()` — see `docs/development/grammar_dispatch.md`) or left
// the pointer's `*` beside it as an `ERROR` opening with that token.
// Since #1885 this runs on every unparenthesised `function_declarator`,
// so an `ERROR` over any other token — the `~` of `R(S::~S);`, the `&`
// of `FOO(N::&x);` — is a class-scope macro call's argument, and the
// call stays the method it reads as.
//
// An invented slot leaves the name to the `pointer_declarator` beside
// it (`S::*const pc` puts `*const pc` in one), which a member pointer's
// `*` always opens. When the declarator is abstract (`DECL_METHOD(S::*);`,
// `FOO(N::&);`, `FOO(N::[3]);`), of another kind, or absent, nothing
// anywhere names a member — the argument list of a method-declaring
// macro — so the parameter is not one either, and the macro call stays
// the method it reads as rather than becoming an attribute with no name.
fn cpp_is_recovered_member_pointer(parameter: &Node) -> bool {
    let Some(qualified) = parameter
        .child_by_field_name("type")
        .filter(|ty| ty.kind() == "qualified_identifier")
    else {
        return false;
    };
    let mut star_seen = false;
    let mut recovered_slot = None;
    for child in cpp_innermost_qualified(&qualified).children() {
        star_seen |= cpp_error_opens_with(&child, "*");
        // role-bypass: allow(DeclKeyword, Primitive) — the name slot of a C++
        // qualified name, which Groovy's rows claim by the same spelling
        if child.kind() == "type_identifier" {
            let invented = child.start_byte() == child.end_byte();
            if star_seen || invented {
                recovered_slot = Some(invented);
            }
        }
    }
    recovered_slot.is_some_and(|invented| {
        !invented
            || parameter
                .child_by_field_name("declarator")
                .is_some_and(|declarator| declarator.kind() == "pointer_declarator")
    })
}

// The data members a class-scope `declaration` declares through the
// recovery above: one per recovered member pointer that declares no
// function, so a comma list counts each of its data declarators beside
// any method it also spells (#1807). A constructor, destructor or
// conversion-operator declaration — what a valid class-scope
// `declaration` otherwise is — holds no recovered member pointer and so
// counts nothing here. The method consumer reads the same declarators
// through `cpp_declares_function`. The two share the pointers
// (`cpp_recovered_member_pointers`), the verdict on each
// (`DeclaratorQuestion::RecoveredPointer`) and the function-only
// specifier test, and each gates and sums them itself, so a test added
// to one side is added to the other;
// `assert_no_declarator_is_claimed_twice` holds them together. A
// declaration with no parse error holds no recovery, and its children
// are not read.
pub(crate) fn cpp_recovered_declaration_data_members(declaration: &Node) -> usize {
    if !declaration.has_error()
        || declaration
            .children()
            .any(|child| cpp_is_function_only_specifier(&child))
    {
        return 0;
    }
    declaration
        .children()
        .filter(|child| {
            child.kind() == "function_declarator"
                && child.has_error()
                && cpp_parenthesized_name(child).is_none()
        })
        .flat_map(|declarator| cpp_recovered_member_pointers(&declarator))
        .filter(|pointer| !cpp_answer(DeclaratorQuestion::RecoveredPointer(pointer.children())))
        .count()
}

// The data members a `field_declaration` declares: its comma-separated
// declarators are each a function or data on their own, and only the
// data ones count, so `int S::*getPd(), S::*pd;` and `int f(), x;` are
// one method and one attribute rather than a method alone (#1807). The
// method consumer reads the same declaration through
// `cpp_declares_function` — one method however many declarators spell
// one — and the recovered-`:` flag is tracked here exactly as there.
pub(crate) fn cpp_field_data_members(field: &Node) -> usize {
    let mut total = 0;
    let mut segment = DeclaratorSegment::default();
    for child in field.children() {
        if child.kind() == "," {
            total += segment.data_members();
            segment = DeclaratorSegment::default();
            continue;
        }
        // role-bypass: allow(Comment) — stepping over a comment between two
        // declarator parts; not whether the node is one
        if child.kind() == "comment" {
            continue;
        }
        segment.declares_function |= cpp_child_declares_function(&child, segment.after_colon);
        if !segment.declares_function {
            segment.names += cpp_declarator_names(&child);
        }
        segment.after_colon = cpp_is_recovered_colon(&child);
    }
    total + segment.data_members()
}

// One comma-separated declarator of a `field_declaration`, read child
// by child: whether it spells a function, how many names it declares,
// and whether the last child read was the recovered `:`.
#[derive(Default)]
struct DeclaratorSegment {
    declares_function: bool,
    names: usize,
    after_colon: bool,
}

impl DeclaratorSegment {
    fn data_members(&self) -> usize {
        if self.declares_function {
            0
        } else {
            self.names
        }
    }
}

// The `pointer_type_declarator` of a member-pointer declarator, which
// both grammars spell as a `qualified_identifier` whose `name` holds it:
// `S::*fp` is `qualified_identifier(scope: S, name:
// pointer_type_declarator(*fp))`. A nested or global scope (`N::S::*`,
// `::S::*`) nests a further `qualified_identifier` in `name`, so the walk
// follows `name` rather than reading one level. Anything else — a plain
// qualified name — has no member pointer and answers `None`.
//
// A loop, not recursion: every qualified *type* a member declaration
// holds is asked too, and `A::A::…::B` nests one `qualified_identifier`
// per `::`, so a recursive walk overflowed the stack on a deep enough
// name that every other metric reads (#1718 review).
fn cpp_member_pointer<'a>(qualified: &Node<'a>) -> Option<Node<'a>> {
    let name = cpp_innermost_qualified(qualified).child_by_field_name("name")?;
    (name.kind() == "pointer_type_declarator").then_some(name)
}

// The innermost `qualified_identifier` of a nested one (`N::S::*fp` is
// `N :: (S :: *fp)`), reached by a loop for the reason above.
fn cpp_innermost_qualified<'a>(qualified: &Node<'a>) -> Node<'a> {
    let mut innermost = *qualified;
    while let Some(name) = innermost
        .child_by_field_name("name")
        .filter(|name| name.kind() == "qualified_identifier")
    {
        innermost = name;
    }
    innermost
}

// One child of the `parenthesized_declarator` in a
// `function_declarator`'s `declarator` field, read for whether the
// enclosing declaration is still a function. Parentheses alone do not
// make it a function *pointer* — what does is an indirection interposed
// inside them:
//
// - `void (f)();` and `int (operator+)(int);` are ordinary member
//   functions written with redundant parentheses. This is the
//   macro-defence idiom (`int (max)(int, int);`), which suppresses
//   expansion of a function-like macro of the same name.
// - `int (*fp)(int);` interposes a `*`, so the parameter list belongs
//   to the pointee and the member is data.
// - `int (*getFp(int))(int);` interposes a `*` too, but wraps a
//   further `function_declarator`, so it is a function *returning* a
//   function pointer. Neither language lets a function return a
//   function type, so that inner nesting is the only reading — the
//   same grammar fact `crate::c_declarator` relies on.
// - `template<class T> static int (S::*fp)(T);` interposes a member
//   pointer, `template<class T> int (S::*get())(T);` the same around a
//   further `function_declarator`: a data member and a method returning
//   a member-function pointer, told apart exactly as the two above
//   (#1718). Only the templated form gets here. Both grammars parse the
//   non-templated `int (S::*pmf)(int);` and `static int (S::*fp)(int);`
//   into an `ERROR` over `S::` beside a plain `pointer_declarator`,
//   which the first arm already reads correctly. With two specifiers
//   before the return type (`static constexpr int (S::*fp)(int) =
//   nullptr;`) or a user return type (`T (S::*pd);`) the grammars
//   recover into a `function_declarator` named by that type instead,
//   which never reaches here: `cpp_child_verdict` hands that shape to
//   `cpp_recovered_member_pointers` (#1807, #1885).
//
// Hence: every child must still read as a function. An indirection
// qualifies only when a function sits under it; a bare name, an
// `operator_name`, a qualified name with no member pointer, or an
// `ms_call_modifier` (`int (__cdecl *f)(int)`) never disqualifies on
// its own.
fn cpp_parenthesized_child_verdict<'a>(child: &Node<'a>) -> Verdict<'a> {
    match child.kind() {
        "pointer_declarator" | "reference_declarator" => {
            Verdict::Ask(DeclaratorQuestion::any_child(child))
        }
        "parenthesized_declarator" => {
            Verdict::Ask(DeclaratorQuestion::Parenthesized(child.children()))
        }
        "qualified_identifier" => cpp_member_pointer_verdict(child, true),
        _ => Verdict::Known(true),
    }
}

// A question the readers above ask of a node, with how far its answer
// has got: each walks its node's children and reads every one as a
// `Verdict`.
enum DeclaratorQuestion<'a> {
    // Whether any child declares a function — `cpp_declares_function` —
    // and whether the last non-comment child read was the recovered `:`.
    AnyChild {
        children: Children<'a>,
        after_colon: bool,
    },
    // Whether every child of a `parenthesized_declarator` still reads as
    // a function.
    Parenthesized(Children<'a>),
    // Whether a `function_declarator` is a function: it is unless it
    // holds recovered member pointers and every one is data. `seen`
    // records whether it holds any.
    RecoveredChain {
        pointers: Box<dyn Iterator<Item = Node<'a>> + 'a>,
        seen: bool,
    },
    // Whether a function sits under one recovered member pointer.
    RecoveredPointer(Children<'a>),
}

// What one child says towards its question: an answer read off the
// child itself, or a further question about it to answer first.
enum Verdict<'a> {
    Known(bool),
    Ask(DeclaratorQuestion<'a>),
}

impl<'a> DeclaratorQuestion<'a> {
    fn any_child(node: &Node<'a>) -> Self {
        Self::AnyChild {
            children: node.children(),
            after_colon: false,
        }
    }

    // The child answer that settles the question at once: `true` for the
    // questions asking whether any child holds a function, `false` for
    // the one asking whether every child still reads as one.
    fn decisive(&self) -> bool {
        match self {
            Self::AnyChild { .. } | Self::RecoveredChain { .. } | Self::RecoveredPointer(_) => true,
            Self::Parenthesized(_) => false,
        }
    }

    // The answer once every child is read and none was decisive.
    fn undecided(&self) -> bool {
        match self {
            Self::Parenthesized(_) => true,
            // No recovered member pointer: an ordinary method.
            Self::RecoveredChain { seen, .. } => !seen,
            Self::AnyChild { .. } | Self::RecoveredPointer(_) => false,
        }
    }

    // The next child's verdict, or `None` once every child is read.
    fn next_verdict(&mut self) -> Option<Verdict<'a>> {
        match self {
            Self::AnyChild {
                children,
                after_colon,
            } => {
                // role-bypass: allow(Comment) — stepping over a comment between
                // two declarator parts; not whether the node is one
                let child = children.find(|child| child.kind() != "comment")?;
                let verdict = cpp_child_verdict(&child, *after_colon);
                *after_colon = cpp_is_recovered_colon(&child);
                Some(verdict)
            }
            Self::Parenthesized(children) => children
                .next()
                .map(|child| cpp_parenthesized_child_verdict(&child)),
            Self::RecoveredChain { pointers, seen } => {
                let pointer = pointers.next()?;
                *seen = true;
                Some(Verdict::Ask(Self::RecoveredPointer(pointer.children())))
            }
            Self::RecoveredPointer(children) => children.next().map(|child| match child.kind() {
                "abstract_function_declarator" => Verdict::Known(true),
                "pointer_declarator" => Verdict::Ask(Self::any_child(&child)),
                _ => Verdict::Known(false),
            }),
        }
    }
}

// Answers a question, keeping the questions still waiting on a nested
// one in `waiting` rather than in recursive calls: the nesting is the
// source's (see `cpp_declares_function`), so this holds one entry per
// open question however deep it goes.
fn cpp_answer(question: DeclaratorQuestion<'_>) -> bool {
    let mut current = question;
    let mut waiting = Vec::new();
    let mut returned = None;
    loop {
        let verdict = match returned.take() {
            Some(answer) => Some(Verdict::Known(answer)),
            None => current.next_verdict(),
        };
        let answer = match verdict {
            Some(Verdict::Ask(nested)) => {
                waiting.push(std::mem::replace(&mut current, nested));
                continue;
            }
            Some(Verdict::Known(answer)) if answer != current.decisive() => continue,
            Some(Verdict::Known(answer)) => answer,
            None => current.undecided(),
        };
        let Some(parent) = waiting.pop() else {
            return answer;
        };
        current = parent;
        returned = Some(answer);
    }
}

// The names one child of a declarator subtree declares. Matches on
// node-kind NAMES, not one grammar's enum discriminants, so it is correct
// for every C-family grammar: upstream `Cpp` and the Mozilla `Mozcpp` fork
// (#720) assign *different* kind_ids to the same node kinds, and this
// helper is shared by both impls. (A `kind_id().into()` against a fixed
// `Cpp` enum silently miscounted Mozcpp nodes, zeroing its `npa`.) Each
// declarator's aliases all render to the one base name, so a single
// string arm covers the family. Walked with an explicit stack, the
// wrappers pushed only when met: they nest one level per parenthesis or
// `*` the source writes, and the recursion this replaced overflowed the
// consumer thread's stack on a function-pointer member some 2,000 levels
// deep in a release build, which every other metric reads (#1829).
pub(super) fn cpp_declarator_names(child: &Node) -> usize {
    let mut names = 0;
    let mut pending = Vec::new();
    let mut next = Some(*child);
    while let Some(node) = next.take().or_else(|| pending.pop()) {
        match node.kind() {
            "field_identifier" => names += 1,
            // `function_declarator` / `parenthesized_declarator` are
            // in the set for the function-pointer data member
            // `int (*fp)(int);`, whose name is buried under both
            // (#1300). A member function's name cannot arrive through
            // them: reaching here means `cpp_declares_function`
            // declined the declaration, and it declines a
            // `function_declarator` only when an indirection inside
            // the parentheses makes the name the pointee's, never the
            // function's — `void (f)();` stays a function precisely so
            // that `f` is never counted here — or when its parameter
            // lists are recovered member pointers that are all data,
            // where the name counted stands for the one they declare
            // (`MOZ_X T (S::*pd);`, an annotated data member, and also the
            // macro call `int DECL(S::*p);`, which is the same tree and
            // is pinned as misread, FIXME(#1829)).
            // One spelling still breaks this: a method whose whole
            // declarator is parenthesised (`int (*get());`) reads as no
            // function, because `cpp_child_verdict` does not look inside a
            // `parenthesized_declarator` that names no function, so `get`
            // is counted here (#1891).
            "function_declarator"
            | "parenthesized_declarator"
            | "pointer_declarator"
            | "array_declarator"
            | "init_declarator"
            | "reference_declarator" => pending.extend(node.children()),
            // The member-pointer data member `(S::*fp)` that
            // `cpp_parenthesized_child_verdict` declines (#1718).
            // One member pointer declares one name, so it counts one
            // without a walk: the scope `S` is never a member, and the
            // name's kind does not matter (the templated parse makes it
            // a `type_identifier`, which a walk for `field_identifier`
            // would miss). No `field_declaration` reaches this arm under
            // the pinned grammars, which never leave a member pointer as
            // a direct `qualified_identifier` child of one: the
            // parenthesised `int (S::*pmf)(int);` recovers into an
            // `ERROR` over `S::` beside a `pointer_declarator`, and the
            // unparenthesised `int S::*pd;` into a `field_identifier`
            // `S`, an `ERROR` over `:` and a `bitfield_clause`. That
            // last shape needs no arm: the scope's leading segment is
            // the one `field_identifier` per declarator, so the plain
            // walk counts one data member each for `int S::*pd;`,
            // `int N::T::*pt;` and `int *S::*pd;`, and the clause itself
            // holds no `field_identifier` to over-count. The method
            // spelling of the same recovery (`int S::*getPd();`) never
            // gets here, since `cpp_declares_function` now reads the
            // clause (#1807). The arm keeps the invariant above if a
            // grammar stops recovering so: whatever the predicate
            // declines, this counts.
            "qualified_identifier" => names += usize::from(cpp_member_pointer(&node).is_some()),
            _ => {}
        }
    }
    names
}

// Whether a `template_declaration` child of a class body declares a
// static data member template (`template<class T> static T x;`), the
// one templated payload that is an attribute (#1299).
//
// The payload is the wrapped `declaration`; every other payload the
// grammars reach — `function_definition`, `alias_declaration`, a nested
// `type_specifier`, `friend_declaration`, a nested
// `template_declaration`, and `concept_definition`, which no valid
// class scope holds (`[temp.concept]/1`) — declares no data member of
// this class and answers `false` on its kind. The kind test is the one
// gate those payloads need: a `declaration`'s `declarator` field is
// required by both grammars, so there is nothing to check for, and
// gating on it as well was measured redundant on every payload above
// (#1299 review). A `declaration` whose declarator spells a function
// (`template<class T> T f();`, `template<class T> operator T();`) is
// the method consumer's and is declined through the same predicate
// `Npm` counts it with, so the two can never both claim one member.
//
// The answer is a yes/no rather than a count because `[temp.pre]/5`
// allows a template declaration's `init-declarator-list` at most one
// declarator: `template<class T> static T a, b;` is ill-formed, so the
// `int b, c;` plural the `field_declaration` arm counts by walking
// `field_identifier` leaves cannot arise here. Not walking the declarator
// also keeps the array bound of `static T arr[N]`, the `T` of a `= T(1)`
// initialiser and the parameter name of `static int (*fp)(int a)` — all
// plain `identifier`s, unlike a field's `field_identifier` — from being
// counted as members. Name-based like `cpp_declares_function`, for the
// same Mozcpp reason. A member-pointer data member template,
// `template<class T> static int (S::*fp)(T);`, is a `declaration` that
// helper declines like any other (#1718).
pub(crate) fn cpp_template_declares_data_member(template: &Node) -> bool {
    template
        .children()
        // role-bypass: allow(DocTarget) — the declaration a template wraps, read
        // for its declarator; not whether it takes a doc comment
        .any(|child| child.kind() == "declaration" && !cpp_declares_function(&child))
}
