//! Cross-language `Npa` helper functions shared by the per-language
//! submodules and by sibling metrics (`npm`, `checker`).
//!
//! The public metric `Stats`, the `Npa` trait, and the
//! `impl_npa_java_like!` / `ts_npa_compute!` / `ts_member_is_public!` /
//! `js_npa_compute!` macros stay in the parent module; only the
//! cross-language predicate helpers live here so the parent clears the
//! 800-SLOC self-scan limit (#976). The C++ family's declarator reader
//! is `cpp_declarators.rs`.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Distinguishes interface-like containers (interface, trait, annotation
// type) — whose members are public unless they say otherwise
// (`groovy_interface_member_is_public`) — from class-like containers
// (class, enum, record) that need an explicit `public` modifier. The
// dekobon grammar models all of these bodies as `class_body`, so the
// discriminant lives on the parent. Shared with `impl Npm for
// GroovyCode` (`metrics::npm`).
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

// The one rule for every member of an interface-like body, read off the
// same flattened modifiers: public unless declared `private` or
// `protected`. groovyc accepts a `private` method or field in a trait and
// keeps it off the trait's interface; a private interface method needs
// Groovy 5 (GROOVY-10060). `protected` compiles on a nested type alone,
// a member record among them, not on a method or a field, and reads as
// not public everywhere rather than as an exception to the rule.
pub(crate) fn groovy_interface_member_is_public(declaration: &Node) -> bool {
    declaration
        .first_child(|id| {
            // role-bypass: allow(Modifier) — which modifier, not whether the token is one
            matches!(id.into(), Groovy::Private | Groovy::Protected)
        })
        .is_none()
}

// Whether a record's header, its implicit canonical constructor, counts
// as a public method (#1842). This is the metric's reading of a Groovy
// member, not groovyc's: the compiler makes that constructor public
// whatever the record spells, as it does every unmodified member, and
// `npm` bills `public` only where the source says so — on the record's
// own `public` in a class, and inside an interface, trait or annotation
// type by the rule every member there reads. A `private` record is
// therefore not public in either, and an unmodified one is public only
// in the second. Java's arm asks the same questions of its grammar
// (`npm.rs`).
pub(crate) fn groovy_record_is_public<'a>(record: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    let interface_member = ancestors
        .iter(record)
        .next()
        .is_some_and(|(body, above)| groovy_body_is_interface_like(&body, above));
    if interface_member {
        groovy_interface_member_is_public(record)
    } else {
        groovy_has_explicit_public(record)
    }
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

// A C# member or accessor node carries an access modifier that narrows
// it below public: `private`, `protected` or `internal`, alone or in the
// `protected internal` and `private protected` pairs. `internal` is
// assembly-wide, not part of the public surface. The single source of
// truth for the C# narrowing rule, shared by the interface-member gate
// below and the npm accessor gate (`csharp_member_public_method_count`).
pub(crate) fn csharp_has_non_public_access_modifier(node: &Node) -> bool {
    node.children().any(|child| {
        // role-bypass: allow(Modifier) — which modifier, not whether the node is one
        matches!(child.kind_id().into(), Csharp::Modifier)
            && child
                .first_child(|id| {
                    id == Csharp::Private || id == Csharp::Protected || id == Csharp::Internal
                })
                .is_some()
    })
}

// C# 8+ interface members default to public, so a member is public
// UNLESS it carries a narrowing access modifier. This is the inverse of
// `csharp_is_explicit_public` (the class rule, which requires an explicit
// `public`): missing modifier means public here, matching
// `ts_member_is_public!`'s default-public gate for TypeScript class
// members.
pub(crate) fn csharp_interface_member_is_public(declaration: &Node) -> bool {
    !csharp_has_non_public_access_modifier(declaration)
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
) -> impl Iterator<Item = Option<&'a str>> + use<'a> {
    use Ruby::*;

    // role-bypass: allow(StringLit) — how many names an argument spells; the
    // row owns the symbol and string kinds as literals, a different question
    let (one, array) = match argument.kind_id().into() {
        SimpleSymbol | DelimitedSymbol | HashKeySymbol | BareSymbol | String => {
            (Some(ruby_name_argument(argument, source)), None)
        }
        SymbolArray => (None, Some(argument.children())),
        _ => (None, None),
    };
    one.into_iter().chain(
        array
            .into_iter()
            .flatten()
            // role-bypass: allow(StringLit) — the array's element kind the row `wraps`
            .filter(|c| matches!(c.kind_id().into(), BareSymbol))
            .map(move |element| ruby_name_argument(&element, source)),
    )
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
fn ruby_name_argument<'a>(node: &Node<'a>, source: &'a [u8]) -> Option<&'a str> {
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
