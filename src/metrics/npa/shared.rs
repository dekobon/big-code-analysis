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

// The names one argument of an `attr_*` or visibility call spells, one
// entry per name: a symbol or a string names one, a `%i[a b]` word
// array names each of its elements, and every other argument shape — a
// splat (`private *SYMS`), a bare identifier (`private foo`), a nested
// call (`private send(:x)`) — names nothing this walk can resolve
// (#1399). An entry is `None` where the name is spelled but not
// resolvable (`:"get_#{suffix}"`): `npa` still counts that attribute,
// and neither consumer can re-file it. `HashKeySymbol` (`a:`) reaches
// no argument list at the pinned grammar — it is only ever a `pair` or
// `keyword_pattern` key — but is kept as a defensive arm
// (grammar-dispatch rule 2), pinned by
// `ruby_hash_key_symbol_declares_no_attribute`.
pub(crate) fn ruby_argument_names<'a>(
    argument: &Node<'a>,
    source: &'a [u8],
) -> Vec<Option<&'a str>> {
    use Ruby::*;

    // role-bypass: allow(StringLit) — how many names an argument spells; the
    // row owns the symbol and string kinds as literals, a different question
    match argument.kind_id().into() {
        SimpleSymbol | DelimitedSymbol | HashKeySymbol | BareSymbol | String => {
            vec![ruby_name_argument(argument, source)]
        }
        SymbolArray => argument
            .children()
            // role-bypass: allow(StringLit) — the array's element kind the row `wraps`
            .filter(|c| matches!(c.kind_id().into(), BareSymbol))
            .map(|element| ruby_name_argument(&element, source))
            .collect(),
        _ => Vec::new(),
    }
}

// The method name a `:foo` or `"foo"` argument to `private` / `public` /
// `attr_reader` / … names — `private "foo"` and `attr_reader "foo"` are
// each what the symbol spelling is (#1399). `simple_symbol` carries the
// whole literal including the leading colon (`:ok?`, `:==`);
// `delimited_symbol` (`:"val="`), a `string` and the `bare_symbol`
// elements of a `%i[…]` array wrap their text in a `string_content`
// child. A `%i[…]` array names several methods rather than one, so
// `ruby_argument_names` expands it into these elements first. Every
// other argument shape names nothing this walk can resolve and yields
// `None`.
//
// A delimited symbol or a string is resolvable only when
// `string_content` is its *whole* content: `:"get_#{suffix}"` also
// carries an `interpolation` child, and returning the literal half of
// it would name — and demote — a different method called `get_`. A
// `"b\n"` carries an `escape_sequence` beside the content and names a
// method Ruby cannot find either (measured: `NameError`), and `""`
// carries no content at all.
pub(crate) fn ruby_name_argument<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
    // Which symbol spelling names a method, read from the bytes; not
    // whether the node is a literal, which the `StringLit` row owns.
    match node.kind_id().into() {
        // role-bypass: allow(StringLit) — the `:name` spelling
        Ruby::SimpleSymbol => node.utf8_text(source)?.strip_prefix(':'),
        // role-bypass: allow(StringLit) — the delimited, string and bare spellings
        Ruby::DelimitedSymbol | Ruby::String | Ruby::BareSymbol => {
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
// every class body is `Public`. The discriminants index the shared body
// walk's per-keyword shadow table (`ruby_body.rs`), which is `COUNT`
// long.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RubyVisibility {
    Public,
    Private,
    Protected,
}

impl RubyVisibility {
    /// The number of keywords, for the table indexed by discriminant.
    pub(crate) const COUNT: usize = 3;
}

// A keyword added without the bump would index past the shadow table
// on the first body that spells it; this makes it a build error.
const _: () = assert!(RubyVisibility::Protected as usize + 1 == RubyVisibility::COUNT);

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

// What a visibility-keyword `Call` in a Ruby class body does. Read by
// the body walk `Npm` and `Npa` share (`ruby_body.rs`), so the two
// cannot drift on the same Ruby rule (grammar-dispatch rule 7).
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

// Whether a declaration is public, given the visibility call whose
// argument list it sits in, if any. The keyword governs it only when it
// names that method family, so `private def self.x` leaves the singleton
// alone and `private_class_method` never reaches an instance method or an
// attribute; an unwrapped or ungoverned declaration falls back to its own
// default. The single source of the rule for both `Npm` and `Npa`
// (#1255).
pub(crate) fn ruby_wrapped_is_public(
    keyword: Option<RubyVisibilityCall>,
    singleton: bool,
    body_flag: RubyVisibility,
) -> bool {
    match keyword {
        Some(keyword) if keyword.governs(singleton) => keyword.visibility == RubyVisibility::Public,
        _ => ruby_declaration_is_public(singleton, body_flag),
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

// The two methods an `attr_*` macro can generate for one name: the
// reader `b` and the writer `b=`. A visibility call names each on its
// own — `private :b` reaches the reader only, `private :b=` the writer
// — so a re-file is keyed by the pair, and `split` reads which of the
// two a spelled name is.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RubyAccessor {
    Reader,
    Writer,
}

impl RubyAccessor {
    /// Which accessor `name` spells, with the attribute's own name: `b=`
    /// is the writer of `b`, anything else the reader of itself.
    pub(crate) fn split(name: &str) -> (&str, Self) {
        match name.strip_suffix('=') {
            Some(stem) => (stem, Self::Writer),
            None => (name, Self::Reader),
        }
    }
}

// Which accessors a Ruby `Call` node generates, when it invokes the
// `attr_*` macro family on the enclosing class: each macro takes a list
// of attribute names and synthesises the reader, the writer, or both for
// every one. `None` for any other call.
pub(crate) fn ruby_attr_macro_accessors(
    call: &Node,
    source: &[u8],
) -> Option<&'static [RubyAccessor]> {
    use RubyAccessor::*;

    match ruby_callee_name(call, source)? {
        "attr_accessor" => Some(&[Reader, Writer]),
        "attr_reader" => Some(&[Reader]),
        "attr_writer" => Some(&[Writer]),
        _ => None,
    }
}

// Books `count` attributes against the class tallies, all of them
// public or none. Kept as a helper so the declaration shapes one
// walker recognises — the C++ family's two — cannot disagree on the
// rule; Ruby's walker tallies its ledger once the body is read.
pub(crate) fn add_attributes(stats: &mut Stats, count: usize, public: bool) {
    stats.class_na += count;
    if public {
        stats.class_npa += count;
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
        "pointer_declarator" | "reference_declarator" | "declaration" => {
            cpp_declares_function(&child)
        }
        // A parenthesised value is a member function template's parameter
        // list, read by the grammar as an expression list when every
        // parameter parses as one: `template<class T> void connect(T* obj,
        // void (T::*slot)());` is `init_declarator(connect,
        // argument_list(…))`. A member-declarator takes only a
        // brace-or-equal-initializer ([class.mem.general]), so at class
        // scope, the only scope these helpers read, that value is never
        // an initialiser.
        "init_declarator" => {
            child
                .child_by_field_name("value")
                .is_some_and(|value| value.kind() == "argument_list")
                || cpp_declares_function(&child)
        }
        // A member pointer outside parentheses: `template<class T>
        // static int S::*getPd();` and, under a `pointer_declarator`,
        // `template<class T> int (*S::*getPp())(T);` are methods
        // returning one (#1718). Any other qualified name — the `std::`
        // of a field's type among them — declares no function.
        "qualified_identifier" => {
            cpp_member_pointer(&child).is_some_and(|pointer| cpp_declares_function(&pointer))
        }
        _ => false,
    })
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
    let mut name = qualified.child_by_field_name("name")?;
    while name.kind() == "qualified_identifier" {
        name = name.child_by_field_name("name")?;
    }
    (name.kind() == "pointer_type_declarator").then_some(name)
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
// - `template<class T> static int (S::*fp)(T);` interposes a member
//   pointer, `template<class T> int (S::*get())(T);` the same around a
//   further `function_declarator`: a data member and a method returning
//   a member-function pointer, told apart exactly as the two above
//   (#1718). Only the templated form gets here. Both grammars parse the
//   non-templated `int (S::*pmf)(int);` and `static int (S::*fp)(int);`
//   into an `ERROR` over `S::` beside a plain `pointer_declarator`,
//   which the first arm already reads correctly. Not every
//   non-templated spelling recovers that way: with two specifiers
//   before the return type (`static constexpr int (S::*fp)(int) =
//   nullptr;`, `static inline`, `static const`) a `function_declarator`
//   nests directly in another, so the data member still reads as a
//   method — a recovery of the grammar's that this predicate cannot see
//   past, and older than #1718.
//
// Hence: every child must still read as a function. An indirection
// qualifies only when `cpp_declares_function` finds a function under
// it; a bare name, an `operator_name`, a qualified name with no member
// pointer, or an `ms_call_modifier` (`int (__cdecl *f)(int)`) never
// disqualifies on its own.
fn cpp_parenthesized_declares_function(paren: &Node) -> bool {
    paren.children().all(|child| match child.kind() {
        "pointer_declarator" | "reference_declarator" => cpp_declares_function(&child),
        "parenthesized_declarator" => cpp_parenthesized_declares_function(&child),
        "qualified_identifier" => {
            cpp_member_pointer(&child).is_none_or(|pointer| cpp_declares_function(&pointer))
        }
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
            // The member-pointer data member `(S::*fp)` that
            // `cpp_parenthesized_declares_function` declines (#1718).
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
            // `S`, an `ERROR` over `:` and a `bitfield_clause` — which
            // counts the scope as the one attribute, right for data and
            // wrong for a method (`int S::*getPd();` reads as data; older
            // than #1718). It keeps the invariant above if a grammar
            // stops doing so: whatever the predicate declines, this
            // counts.
            "qualified_identifier" => count += usize::from(cpp_member_pointer(&child).is_some()),
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
