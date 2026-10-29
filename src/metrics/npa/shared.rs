//! Cross-language `Npa` helper functions shared by the per-language
//! submodules and by sibling metrics (`npm`, `checker`).
//!
//! The public metric `Stats`, the `Npa` trait, and the
//! `impl_npa_java_like!` / `ts_npa_compute!` / `ts_member_is_public!` /
//! `js_npa_compute!` macros stay in the parent module; only the
//! cross-language predicate helpers live here so the parent clears the
//! 800-SLOC self-scan limit (#976).
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Distinguishes interface-like containers (interface, trait, annotation
// type) — whose members are implicitly public — from class-like
// containers (class, enum, record) that need an explicit `public`
// modifier. The dekobon grammar models all of these bodies as
// `class_body`, so the discriminant lives on the parent. Shared with
// `impl Npm for GroovyCode` (`metrics::npm`).
pub(crate) fn groovy_body_is_interface_like<'a>(
    body: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Groovy::*;
    ancestors.parent(body).is_some_and(|p| {
        // role-bypass: allow(Container) — which container the body belongs to,
        // for the interface bucket; not whether it opens a space
        matches!(
            p.kind_id().into(),
            InterfaceDeclaration | TraitDeclaration | AnnotationTypeDeclaration
        )
    })
}

// Detects an explicit `public` modifier on a class member declaration.
// The dekobon grammar flattens the `_modifier` rule, so modifier
// tokens appear as direct children of the declaration — no `Modifiers`
// wrapper to descend into. Shared with `impl Npm for GroovyCode`.
pub(crate) fn groovy_has_explicit_public(declaration: &Node) -> bool {
    declaration.first_child(|id| id == Groovy::Public).is_some()
}

// C# uses individual `Modifier` nodes (not wrapped under a single
// `modifiers` node like Java); detecting `public` requires scanning
// every Modifier child of the declaration for a `public` keyword.
//
// The wrapper-plus-leaf shape this and the sibling below assume is the
// only one that reaches them. Two other rules alias a bare keyword to
// `modifier`, which makes those childless (#1418):
// `_parameter_type_with_modifiers` (six keywords, under `parameter`)
// and `_lambda_expression_init` / `anonymous_method_expression`
// (`static` / `async`, under a lambda). Neither is a direct child of a
// declaration — `parameter_list` intervenes for the first, and a lambda
// is an expression — and both helpers are handed a class member or an
// `accessor_declaration` only. They also use `first_child`, so a
// childless node would answer `false` rather than mis-answer even if
// one did arrive.
pub(crate) fn csharp_is_explicit_public(declaration: &Node) -> bool {
    declaration.children().any(|child| {
        // role-bypass: allow(Modifier) — which modifier, not whether the node is one
        matches!(child.kind_id().into(), Csharp::Modifier)
            && child.first_child(|id| id == Csharp::Public).is_some()
    })
}

// A C# member or accessor node carries an explicit visibility-narrowing
// modifier (`private` / `protected`). The single source of truth for the
// C# narrowing rule, shared by the npa interface-member gate below and the
// npm accessor gate (`csharp_member_public_method_count`).
pub(crate) fn csharp_node_has_private_or_protected_modifier(node: &Node) -> bool {
    node.children().any(|child| {
        // role-bypass: allow(Modifier) — which modifier, not whether the node is one
        matches!(child.kind_id().into(), Csharp::Modifier)
            && child
                .first_child(|id| id == Csharp::Private || id == Csharp::Protected)
                .is_some()
    })
}

// C# 8+ interface members default to public, so a field is a public
// attribute UNLESS it carries an explicit `private` or `protected`
// modifier. This is the inverse of `csharp_is_explicit_public` (the
// class rule, which requires an explicit `public`): missing modifier
// means public here, matching `ts_member_is_public!`'s default-public
// gate for TypeScript class members.
pub(crate) fn csharp_interface_member_is_public(declaration: &Node) -> bool {
    !csharp_node_has_private_or_protected_modifier(declaration)
}

// PHP's strict-explicit visibility rule (mirroring Java's pattern): a
// declaration is treated as public only when it carries an explicit
// `public` modifier. Modifier-less declarations — deprecated for
// properties since PHP 8 and merely conventional for methods — are NOT
// counted, even though PHP semantically defaults methods to public.
pub(crate) fn php_is_explicit_public(declaration: &Node) -> bool {
    declaration.children().any(|child| {
        matches!(child.kind_id().into(), Php::VisibilityModifier)
            && child.first_child(|id| id == Php::Public).is_some()
    })
}

// Named arguments of a Ruby `Call`, in source order. The grammar hangs
// them off an `argument_list` — aliased to two visible ids — whose
// children include the `(`, `)` and `,` tokens of the parenthesised
// spelling, so anonymous nodes are filtered out. A call written without
// an argument list (`private`, spelled as a `Call` only when it has a
// receiver) yields nothing.
pub(crate) fn ruby_call_named_arguments<'a>(call: &Node<'a>) -> impl Iterator<Item = Node<'a>> {
    ruby_call_argument_list(call)
        .into_iter()
        .flat_map(|args| args.children().filter(Node::is_named))
}

/// The `argument_list` of a call, for a consumer that needs the list
/// node itself as the arguments' parent.
pub(crate) fn ruby_call_argument_list<'a>(call: &Node<'a>) -> Option<Node<'a>> {
    use Ruby::*;

    call.children()
        .find(|c| matches!(c.kind_id().into(), ArgumentList | ArgumentList2))
}

// How many attributes / methods one argument to an `attr_*` or
// visibility macro names. A `%i[a b]` word array is one argument naming
// several; every other symbol spelling names exactly one. `HashKeySymbol`
// (`a:`) reaches no argument list at the pinned grammar — it is only ever
// a `pair` or `keyword_pattern` key — but is kept as a defensive arm
// (grammar-dispatch rule 2), pinned by
// `ruby_hash_key_symbol_declares_no_attribute`.
fn ruby_symbol_argument_count(arg: &Node) -> usize {
    use Ruby::*;

    // role-bypass: allow(StringLit) — how many names a symbol argument spells; the
    // row owns the symbol kinds as literals, a different question
    match arg.kind_id().into() {
        SimpleSymbol | DelimitedSymbol | HashKeySymbol | BareSymbol => 1,
        SymbolArray => arg
            .children()
            // role-bypass: allow(StringLit) — the array's element kind the row `wraps`
            .filter(|c| matches!(c.kind_id().into(), BareSymbol))
            .count(),
        _ => 0,
    }
}

// Counts the number of symbol arguments passed to an `attr_accessor` /
// `attr_reader` / `attr_writer` macro `Call` node. `attr_accessor :a,
// :b, :c` exposes three attributes; an `attr_*` call with no arguments
// is ill-formed Ruby but defensively returns zero rather than one.
pub(crate) fn ruby_attr_macro_symbol_count(call: &Node) -> usize {
    ruby_call_named_arguments(call)
        .map(|arg| ruby_symbol_argument_count(&arg))
        .sum()
}

// The method name a `:foo` argument to `private` / `public` / … names.
// `simple_symbol` carries the whole literal including the leading colon
// (`:ok?`, `:==`); `delimited_symbol` (`:"val="`) and the `bare_symbol`
// elements of a `%i[…]` array both wrap their text in a `string_content`
// child. A `%i[…]` array names several methods rather than one, so its
// callers expand it into these elements first. Every other argument
// shape — a splat (`private *SYMS`), a bare identifier (`private foo`),
// a string (`private "foo"`) — names nothing this walk can resolve and
// yields `None` (#1399).
//
// A delimited symbol is resolvable only when `string_content` is its
// *whole* content: `:"get_#{suffix}"` also carries an `interpolation`
// child, and returning the literal half of it would name — and demote —
// a different method called `get_`.
pub(crate) fn ruby_symbol_name<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    // Which symbol spelling names a method, read from the bytes; not
    // whether the node is a literal, which the `StringLit` row owns.
    match node.kind_id().into() {
        // role-bypass: allow(StringLit) — the `:name` spelling
        Ruby::SimpleSymbol => node.utf8_text(source)?.strip_prefix(':'),
        // role-bypass: allow(StringLit) — the delimited and bare spellings
        Ruby::DelimitedSymbol | Ruby::BareSymbol => {
            let mut content = node.children().filter(Node::is_named);
            let first = content.next()?;
            (first.kind_id() == Ruby::StringContent as u16 && content.next().is_none())
                .then_some(first)?
                .utf8_text(source)
        }
        _ => None,
    }
}

// The declared name of a Ruby `method` / `singleton_method`. Read from
// the grammar's required `name` field rather than by child index: the
// name is the *second* identifier of `def obj.x`, and it is a `setter`
// (`val=`) or an `operator` (`==`) node rather than an `identifier` for
// those two spellings (grammar-dispatch rule 3).
pub(crate) fn ruby_method_name<'a>(method: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    method.child_by_field_name("name")?.utf8_text(source)
}

// Ruby class-body visibility state. `private` / `public` / `protected`
// keywords flip this flag for every subsequent declaration in the same
// body until another marker overrides them. The default at the top of
// every class body is `Public`. The discriminants index `npm`'s
// per-keyword shadow table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RubyVisibility {
    Public,
    Private,
    Protected,
}

// Recognises a bare visibility-keyword `identifier` child of a Ruby
// class body (`private` / `public` / `protected` with no arguments).
// tree-sitter-ruby emits the keyword-form as a literal `identifier`
// token; the argument-form (`private :foo`, `private def bar`) is a
// `Call` node instead and does NOT flip the body-wide flag.
pub(crate) fn ruby_visibility_marker(node: &Node, source: &[u8]) -> Option<RubyVisibility> {
    if !matches!(node.kind_id().into(), Ruby::Identifier) {
        return None;
    }
    match node.utf8_text(source)? {
        "private" => Some(RubyVisibility::Private),
        "public" => Some(RubyVisibility::Public),
        "protected" => Some(RubyVisibility::Protected),
        _ => None,
    }
}

// A Ruby visibility keyword used in call position. `targets_singleton`
// records *which* method family the keyword names: the bare keywords
// govern instance methods only, and demoting a `def self.x` takes the
// dedicated `private_class_method` / `public_class_method` pair (#1255).
#[derive(Clone, Copy)]
pub(crate) struct RubyVisibilityCall {
    pub(crate) visibility: RubyVisibility,
    pub(crate) targets_singleton: bool,
}

impl RubyVisibilityCall {
    // Whether this keyword names the given method family, and so decides
    // the visibility of a declaration in its argument list. `private def
    // self.x` does not govern the singleton it wraps, and
    // `private_class_method def x` does not govern the instance method.
    pub(crate) fn governs(self, singleton: bool) -> bool {
        self.targets_singleton == singleton
    }
}

// What a visibility-keyword `Call` in a Ruby class body does. Shared by
// `Npm` and `Npa` so the two walkers cannot drift on the same Ruby rule
// (grammar-dispatch rule 7).
pub(crate) enum RubyVisibilityEffect {
    // `private()` — the explicit-parens spelling of the bare keyword,
    // which sets the default for every later instance-level declaration
    // exactly as `private` on its own line does. (The bare spelling is
    // an `identifier`, not a `Call`, and is matched by
    // `ruby_visibility_marker`.)
    Flag(RubyVisibility),
    // `private def x` / `private :foo` / `private attr_accessor :b` —
    // the keyword governs only what its argument list names, leaving the
    // body-wide flag alone.
    Arguments(RubyVisibilityCall),
}

// Classifies a Ruby class-body `Call` whose callee is a visibility
// keyword.
//
// An argument-less `private_class_method` is an ArgumentError in Ruby
// rather than a flag flip, so it yields no effect at all.
pub(crate) fn ruby_visibility_effect(call: &Node, source: &[u8]) -> Option<RubyVisibilityEffect> {
    let (visibility, targets_singleton) = match ruby_callee_name(call, source)? {
        "private" => (RubyVisibility::Private, false),
        "public" => (RubyVisibility::Public, false),
        "protected" => (RubyVisibility::Protected, false),
        "private_class_method" => (RubyVisibility::Private, true),
        "public_class_method" => (RubyVisibility::Public, true),
        _ => return None,
    };
    if ruby_call_named_arguments(call).next().is_none() {
        return (!targets_singleton).then_some(RubyVisibilityEffect::Flag(visibility));
    }
    Some(RubyVisibilityEffect::Arguments(RubyVisibilityCall {
        visibility,
        targets_singleton,
    }))
}

// Whether a declaration counts as public when no visibility keyword
// names it: an instance method or attribute takes the body-wide flag,
// while a singleton method (`def self.x`) ignores that flag entirely —
// Ruby's `private` governs instance methods only (#1255).
pub(crate) fn ruby_declaration_is_public(singleton: bool, body_flag: RubyVisibility) -> bool {
    singleton || body_flag == RubyVisibility::Public
}

// Whether a declaration nested in a visibility call's argument list is
// public. The keyword governs it only when it names that method family,
// so `private def self.x` leaves the singleton alone and
// `private_class_method` never reaches an instance method or an
// attribute; an ungoverned declaration falls back to its own default.
// The single source of the rule for both `Npm` and `Npa` (#1255).
pub(crate) fn ruby_wrapped_is_public(
    keyword: RubyVisibilityCall,
    singleton: bool,
    body_flag: RubyVisibility,
) -> bool {
    if keyword.governs(singleton) {
        keyword.visibility == RubyVisibility::Public
    } else {
        ruby_declaration_is_public(singleton, body_flag)
    }
}

// The bare name a Ruby `Call` invokes on the enclosing class body.
//
// A receiver other than `self` puts the call on another object, so
// `Other.private :x` and `@cfg.attr_accessor :y` name nothing in *this*
// class and yield `None`. Both halves are read from the grammar's
// `receiver` / `method` fields rather than by scanning for the leading
// `identifier` (grammar-dispatch rule 3): the scan answers correctly
// only when the receiver happens to be a local variable, and reports
// the method name as the callee for a `constant`, an instance variable,
// a global, or a chained call — all of which are `_primary` receivers.
fn ruby_callee_name<'a>(call: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    if let Some(receiver) = call.child_by_field_name("receiver")
        && receiver.kind_id() != Ruby::Zelf as u16
    {
        return None;
    }
    call.child_by_field_name("method")?.utf8_text(source)
}

// Whether a Ruby `Call` node invokes the `attr_*` macro family on the
// enclosing class. Each macro takes a list of attribute symbols and
// synthesises the matching reader / writer / accessor methods.
pub(crate) fn ruby_is_attr_macro(call: &Node, source: &[u8]) -> bool {
    matches!(
        ruby_callee_name(call, source),
        Some("attr_accessor" | "attr_reader" | "attr_writer")
    )
}

// Books `count` attributes against the class tallies, all of them
// public or none. Kept as a helper so the declaration shapes one
// walker recognises — Ruby's three, the C++ family's two — cannot
// disagree on the rule.
pub(crate) fn add_attributes(stats: &mut Stats, count: usize, public: bool) {
    stats.class_na += count;
    if public {
        stats.class_npa += count;
    }
}

// Attributes declared by an `attr_*` macro nested in a visibility call's
// argument list: `private attr_accessor :b` parses as a `private` call
// whose sole argument is the `attr_accessor` call, so the macro is never
// a direct child of the body (#1255).
fn ruby_wrapped_attr_count(call: &Node, source: &[u8]) -> usize {
    use Ruby::*;

    // role-bypass: allow(NullSafe) — an `attr_*` macro call, which the row's safe-navigation
    // owner is a kind of; the macro is told by its name
    ruby_call_named_arguments(call)
        .filter(|arg| {
            matches!(arg.kind_id().into(), Call | Call2 | Call3 | Call4)
                && ruby_is_attr_macro(arg, source)
        })
        .map(|arg| ruby_attr_macro_symbol_count(&arg))
        .sum()
}

// Walks the direct children of a Ruby class / singleton-class body
// (`BodyStatement` under `Class` / `SingletonClass`) tallying:
// - class-scope assignments to `@var` (`InstanceVariable`) and
//   `@@var` (`ClassVariable`) — one attribute per assignment, regardless
//   of whether the RHS is a constant or another expression.
// - `attr_accessor` / `attr_reader` / `attr_writer` macros — one
//   attribute per symbol argument.
//
// Visibility flags follow Ruby's keyword-marker convention: a bare
// `private` / `public` / `protected` identifier flips the default for
// every subsequent declaration in the body. The default visibility at
// the top of every class body is `public`. The argument-form of those
// keywords (`private :foo`, `private attr_accessor :b`) does not flip
// the body-wide flag — matching Ruby's runtime behaviour — but it does
// govern the declarations it wraps, so an `attr_*` macro nested in a
// visibility call takes the keyword's visibility (#1255).
//
// The retroactive symbol form (`private :b` after `attr_accessor :b`)
// is deliberately not modelled here. It renames the visibility of the
// generated *reader* only, leaving `b=` public, so there is no single
// answer for the attribute — unlike `Npm`, where the symbol names one
// method (see #1399).
//
// Attribute assignments to instance/class variables are visible only
// via the methods that wrap them, so the visibility flag at the point
// of declaration is what `npa` should reflect.
pub(crate) fn ruby_walk_class_body(body: &Node<'_>, source: &[u8], stats: &mut Stats) {
    // bca: suppress(cognitive) — grammar dispatch match, clearest whole
    // One pass over the body carrying Ruby's body-wide visibility flag, with
    // an arm per attribute-declaring shape (`@x = …`, `attr_*` macros). The
    // flag must be readable at every arm, so the arms cannot be lifted out
    // without threading it back through each helper.
    use crate::roles::ruby::member;
    use Ruby::*;

    let mut visibility = RubyVisibility::Public;
    for child in body.children() {
        if let Some(marker) = ruby_visibility_marker(&child, source) {
            visibility = marker;
            continue;
        }
        match child.kind_id().into() {
            // A class-scope `@x = …` / `@@n = …`: the `Member` row's
            // variable where it is an assignment target. The lhs's chain
            // is the body's plus the statement, which is all the gate
            // reads.
            Assignment | Assignment2 => {
                let Some(lhs) = child.children().next() else {
                    continue;
                };
                let under = [*body, child];
                if member::owns(&lhs, source, Ancestors::known(&under)) {
                    add_attributes(stats, 1, ruby_declaration_is_public(false, visibility));
                }
            }
            // role-bypass: allow(NullSafe) — the `attr_*` and visibility macro calls, which the
            // row's safe-navigation owner is a kind of; each is told by its name
            Call | Call2 | Call3 | Call4 if ruby_is_attr_macro(&child, source) => {
                add_attributes(
                    stats,
                    ruby_attr_macro_symbol_count(&child),
                    ruby_declaration_is_public(false, visibility),
                );
            }
            // role-bypass: allow(NullSafe) — the visibility macro calls, told by name
            Call | Call2 | Call3 | Call4 => match ruby_visibility_effect(&child, source) {
                Some(RubyVisibilityEffect::Flag(flag)) => visibility = flag,
                // `private attr_accessor :b`. An attribute is always an
                // instance-level declaration, so a class-method keyword
                // never governs one: the macro it wraps still declares
                // its attributes, at the body-wide flag.
                Some(RubyVisibilityEffect::Arguments(keyword)) => {
                    add_attributes(
                        stats,
                        ruby_wrapped_attr_count(&child, source),
                        ruby_wrapped_is_public(keyword, false, visibility),
                    );
                }
                None => {}
            },
            _ => {}
        }
    }
}

// Go's lexical export rule: an identifier names an exported (public)
// member iff its first character is an uppercase Unicode letter
// (`unicode.IsUpper`). Uses `char::is_uppercase` on the first `char`
// (not `is_ascii_uppercase`) so non-ASCII exports such as `Ärger`
// are recognised. The blank identifier `_` and lowercase names are
// unexported. Shared by `Npa` (field names) and `Npm` (method
// names); see issue #458.
pub(crate) fn go_is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

// Extracts the declared name of a Go struct field for visibility
// purposes. A named field carries one or more `FieldIdentifier`
// children; an embedded field has none and instead embeds a type,
// whose base identifier (`TypeIdentifier`) is the field's name —
// `io.Reader` embeds as `Reader`, `*Foo` as `Foo`. Returns the
// embedded type's name node so callers can read its text from
// `code`.
pub(crate) fn go_embedded_field_name<'a>(declaration: &Node<'a>) -> Option<Node<'a>> {
    use self::Go::{PointerType, QualifiedType, TypeIdentifier};

    declaration
        .children()
        .find_map(|child| match child.kind_id().into() {
            // `*Foo` (pointer embed) and `pkg.Foo` (qualified embed)
            // wrap the type; the base name is the final TypeIdentifier.
            PointerType | QualifiedType => child
                .children()
                .filter(|n| matches!(n.kind_id().into(), TypeIdentifier))
                .last(),
            TypeIdentifier => Some(child),
            _ => None,
        })
}

// Name-based for cross-grammar correctness (see `cpp_count_field_identifiers`).
//
// A `function_definition` child is accepted outright rather than
// recursed into: it *is* a function, whatever declarator it carries.
// Recursing would miss a conversion operator with an inline body,
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
// the same node spells two opposite things, and only its `declarator`
// field tells them apart. A name (`realMethod()`) or an
// `operator_name` (`operator->()`) is a member function; a
// `parenthesized_declarator` may be either, so it is handed to
// `cpp_parenthesized_declares_function` (#1300).
//
// Neither `function_definition` nor `operator_cast` can be reached
// from `Npa`'s `field_declaration` call site: per both grammars'
// `node-types.json`, neither `field_declaration` nor the
// `pointer_declarator` / `reference_declarator` wrappers this recurses
// through can carry a `function_definition`, an `operator_cast`, or a
// `declaration` — so there is no path to either at any depth. `Npa`'s
// other call site, `cpp_template_declares_data_member`, hands over the
// `declaration` under a `template_declaration`, where `operator_cast`
// is live (`template<class T> operator T();`) and is what declines the
// templated conversion operator as a data member. `function_definition`
// stays live only for `Npm`'s `template_declaration` children, because
// that helper hands over the `declaration` payload alone and never the
// wrapper.
pub(crate) fn cpp_declares_function(node: &Node) -> bool {
    node.children().any(|child| match child.kind() {
        "function_declarator" => match child.child_by_field_name("declarator") {
            Some(inner) if inner.kind() == "parenthesized_declarator" => {
                cpp_parenthesized_declares_function(&inner)
            }
            _ => true,
        },
        "function_definition" | "operator_cast" => true,
        // Recurse through declarator wrappers that can sit above the
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
            cpp_declares_function(&child)
        }
        _ => false,
    })
}

// Given the `parenthesized_declarator` in a `function_declarator`'s
// `declarator` field, says whether the enclosing declaration is still
// a function. Parentheses alone do not make it a function *pointer* —
// what does is an indirection interposed inside them:
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
//
// Hence: every child must still read as a function. An indirection
// qualifies only when `cpp_declares_function` finds a function under
// it; a bare name, an `operator_name`, or an `ms_call_modifier`
// (`int (__cdecl *f)(int)`) never disqualifies on its own.
fn cpp_parenthesized_declares_function(paren: &Node) -> bool {
    paren.children().all(|child| match child.kind() {
        "pointer_declarator" | "reference_declarator" => cpp_declares_function(&child),
        "parenthesized_declarator" => cpp_parenthesized_declares_function(&child),
        _ => true,
    })
}

// Matches on node-kind NAMES, not one grammar's enum discriminants, so it
// is correct for every C-family grammar: upstream `Cpp` and the Mozilla
// `Mozcpp` fork (#720) assign *different* kind_ids to the same node kinds,
// and this helper is shared by both impls. (A `kind_id().into()` against a
// fixed `Cpp` enum silently miscounted Mozcpp nodes, zeroing its `npa`.)
// Each declarator's aliases all render to the one base name, so a single
// string arm covers the family.
pub(crate) fn cpp_count_field_identifiers(node: &Node) -> usize {
    let mut count = 0;
    for child in node.children() {
        match child.kind() {
            "field_identifier" => count += 1,
            // `function_declarator` / `parenthesized_declarator` are
            // in the set for the function-pointer data member
            // `int (*fp)(int);`, whose name is buried under both
            // (#1300). A member function's name cannot arrive through
            // them: reaching here means `cpp_declares_function`
            // declined the declaration, and it declines a
            // `function_declarator` only when an indirection inside
            // the parentheses makes the name the pointee's, never the
            // function's — `void (f)();` stays a function precisely so
            // that `f` is never counted here.
            "function_declarator"
            | "parenthesized_declarator"
            | "pointer_declarator"
            | "array_declarator"
            | "init_declarator"
            | "reference_declarator" => {
                count += cpp_count_field_identifiers(&child);
            }
            _ => {}
        }
    }
    count
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
// same Mozcpp reason.
//
// FIXME(#1718): a pointer-to-member-function data member,
// `static int (S::*fp)(T);`, still reads as a method here and in the
// non-templated `int (S::*pmf)(int);` alike — the parenthesised
// declarator holds a `qualified_identifier` wrapping a
// `pointer_type_declarator`, an indirection
// `cpp_parenthesized_declares_function` does not recognise.
pub(crate) fn cpp_template_declares_data_member(template: &Node) -> bool {
    template
        .children()
        .any(|child| child.kind() == "declaration" && !cpp_declares_function(&child))
}

// A `visibility_modifier` node counts as public unless it has a direct
// `Zelf` child — the structural signature of `pub(self)` / `pub(in self)`,
// which restrict visibility to the current module (semantically private,
// issue #460). `pub(in self::inner)` nests `self` inside a
// `scoped_identifier` (not a direct child) and stays public. Shared by
// `rust_item_is_public` and the tuple-struct positional-field path so the
// two never drift.
pub(crate) fn rust_visibility_modifier_is_public(node: &Node) -> bool {
    !node.children().any(|c| c.kind_id() == Rust::Zelf)
}

// Returns true if `node` carries a `visibility_modifier` that makes the
// item public to its definition. Matches Rust's "public-only-when-`pub`"
// model: bare `pub`, `pub(crate)`, `pub(super)`, and `pub(in <path>)`
// widen visibility beyond the current module and count as public.
//
// The exceptions are `pub(self)` and `pub(in self)`, which restrict
// visibility to the current module — semantically identical to no
// modifier (private), so they must NOT count toward npm/npa (issue
// #460). The grammar emits the restriction keyword as a dedicated
// child of the `visibility_modifier`: `self` is a `Zelf` node, `crate`
// a `Crate` node, `super` a `Super` node. `pub(self)` and `pub(in self)`
// are exactly the forms whose `visibility_modifier` has a direct `Zelf`
// child; `pub(in self::inner)` nests `self` inside a `scoped_identifier`
// (not a direct child) and is treated as public, matching the issue's
// "only `self` / `in self` is private" scope. No source-text inspection
// is needed — the structural check is precise.
pub(crate) fn rust_item_is_public(node: &Node) -> bool {
    node.children()
        .filter(|c| c.kind_id() == Rust::VisibilityModifier)
        .any(|vis| rust_visibility_modifier_is_public(&vis))
}

// Kotlin's grammar models classes and interfaces under a single
// `class_declaration` node; the `class` / `interface` keyword child
// disambiguates. A `ClassBody` belongs to an interface iff its parent
// `class_declaration` has an `interface` keyword child.
pub(crate) fn kotlin_class_body_is_interface<'a>(
    class_body: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent(class_body).is_some_and(|p| {
        // role-bypass: allow(Container) — which container the body belongs to,
        // for the interface bucket; not whether it opens a space
        matches!(p.kind_id().into(), Kotlin::ClassDeclaration)
            && p.first_child(|id| id == Kotlin::Interface).is_some()
    })
}

// Kotlin's default visibility is `public`. A declaration is non-public
// only when it carries an explicit `private` / `protected` / `internal`
// modifier under its `Modifiers` child. Returns `true` for missing
// `Modifiers`, missing `VisibilityModifier`, or an explicit `public`
// modifier.
pub(crate) fn kotlin_is_public(decl: &Node) -> bool {
    let Some(modifiers) = decl.first_child(|id| id == Kotlin::Modifiers) else {
        return true;
    };
    let Some(visibility) = modifiers.first_child(|id| id == Kotlin::VisibilityModifier) else {
        return true;
    };
    // The visibility modifier holds exactly one keyword child; absence or
    // an explicit `public` both mean public.
    visibility
        .first_child(|id| {
            // role-bypass: allow(Modifier) — which visibility, not whether the token is a modifier
            matches!(
                id.into(),
                Kotlin::Private | Kotlin::Protected | Kotlin::Internal
            )
        })
        .is_none()
}
