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
pub(crate) fn csharp_is_explicit_public(declaration: &Node) -> bool {
    declaration.children().any(|child| {
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

// Counts the number of symbol arguments passed to an `attr_accessor` /
// `attr_reader` / `attr_writer` macro `Call` node. `attr_accessor :a,
// :b, :c` exposes three attributes; an `attr_*` call with no arguments
// is ill-formed Ruby but defensively returns zero rather than one.
pub(crate) fn ruby_attr_macro_symbol_count(call: &Node) -> usize {
    use Ruby::*;

    call.children()
        .find(|c| matches!(c.kind_id().into(), ArgumentList | ArgumentList2))
        .map_or(0, |args| {
            args.children()
                .filter(|c| {
                    matches!(
                        c.kind_id().into(),
                        SimpleSymbol | DelimitedSymbol | HashKeySymbol | BareSymbol
                    )
                })
                .count()
        })
}

// Ruby class-body visibility state. `private` / `public` / `protected`
// keywords flip this flag for every subsequent declaration in the same
// body until another marker overrides them. The default at the top of
// every class body is `Public`.
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

// Identifies the `attr_*` macro family on a Ruby `Call` node. Each
// macro takes a list of attribute symbols and synthesises the matching
// reader / writer / accessor methods on the enclosing class.
pub(crate) fn ruby_attr_macro_name(call: &Node, source: &[u8]) -> Option<&'static str> {
    let ident = call
        .children()
        .find(|c| matches!(c.kind_id().into(), Ruby::Identifier))?;
    match ident.utf8_text(source)? {
        "attr_accessor" => Some("attr_accessor"),
        "attr_reader" => Some("attr_reader"),
        "attr_writer" => Some("attr_writer"),
        _ => None,
    }
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
// keywords (`private :foo`, `private def x`) does not flip the body-
// wide flag — matching Ruby's runtime behaviour.
//
// Attribute assignments to instance/class variables are visible only
// via the methods that wrap them, so the visibility flag at the point
// of declaration is what `npa` should reflect.
pub(crate) fn ruby_walk_class_body(body: &Node, source: &[u8], stats: &mut Stats) {
    // bca: suppress(cognitive) — grammar dispatch match, clearest whole
    // One pass over the body carrying Ruby's body-wide visibility flag, with
    // an arm per attribute-declaring shape (`@x = …`, `attr_*` macros). The
    // flag must be readable at every arm, so the arms cannot be lifted out
    // without threading it back through each helper.
    use Ruby::*;

    let mut visibility = RubyVisibility::Public;
    for child in body.children() {
        if let Some(marker) = ruby_visibility_marker(&child, source) {
            visibility = marker;
            continue;
        }
        match child.kind_id().into() {
            Assignment | Assignment2 => {
                let Some(lhs) = child.children().next() else {
                    continue;
                };
                if matches!(lhs.kind_id().into(), InstanceVariable | ClassVariable) {
                    stats.class_na += 1;
                    if visibility == RubyVisibility::Public {
                        stats.class_npa += 1;
                    }
                }
            }
            Call | Call2 | Call3 | Call4 if ruby_attr_macro_name(&child, source).is_some() => {
                let count = ruby_attr_macro_symbol_count(&child);
                stats.class_na += count;
                if visibility == RubyVisibility::Public {
                    stats.class_npa += count;
                }
            }
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
pub(crate) fn cpp_has_function_declarator(node: &Node) -> bool {
    node.children().any(|child| match child.kind() {
        "function_declarator" => true,
        // Recurse through declarator wrappers that can sit above the
        // function_declarator (`Foo* operator->()`,
        // `template<...> T fn();`, constructor / destructor
        // `declaration`s inside a class body).
        "pointer_declarator" | "reference_declarator" | "declaration" => {
            cpp_has_function_declarator(&child)
        }
        _ => false,
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
            "pointer_declarator"
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

// Returns `true` if `pat` contains exactly one `UNDERSCORE` token
// (identified by `underscore_id`) and no other named children.
// Anonymous tokens such as a leading `|` in a Rust or-pattern
// (`| _ => ...`) are skipped — they do not change the semantic
// meaning of the pattern.
//
// Shared between languages whose `default:`-equivalent wildcard
// pattern is a single `_`:
//   - Rust `match_pattern` (`Cyclomatic` and `Abc` for `RustCode`)
//   - Python `case_pattern` (`Abc` for `PythonCode`)
//
// The Rust caller passes its grammar's `UNDERSCORE` kind id; Python
// passes its own. Guard handling is the caller's responsibility —
// in Rust the guard is a sibling inside `match_pattern` and so adds
// a named child here (this helper returns `false`); in Python the
// guard is an `if_clause` sibling on the enclosing `case_clause`,
// so the caller must check the surrounding node separately.
pub(crate) fn pattern_is_bare_underscore(pat: &Node, underscore_id: u16) -> bool {
    let mut found_underscore = false;
    for child in pat.children() {
        if child.kind_id() == underscore_id {
            if found_underscore {
                return false;
            }
            found_underscore = true;
        } else if child.is_named() {
            return false;
        }
        // else: anonymous non-`_` token (like `|`) — skip.
    }
    found_underscore
}

// Returns `true` iff a Python `case_clause` should count as a
// non-trivial decision: either the pattern is not a bare `_`, or
// the clause carries an `if`-guard (`case _ if g:`).
//
// Shared between the `Cyclomatic` and `Abc` implementations for
// `PythonCode`. The bare wildcard without a guard is Python's
// `default:`-equivalent and is filtered out, matching Rust's bare-`_`
// MatchArm rule and Java/C#'s `default:` rule.
//
// `underscore_id` is the grammar's `Python::UNDERSCORE` kind id,
// passed in so the helper does not assume a particular module-path
// to the language enum.
pub(crate) fn python_case_clause_counts(node: &Node, underscore_id: u16) -> bool {
    let mut bare_underscore = false;
    for child in node.children() {
        match child.kind_id().into() {
            Python::IfClause => return true,
            Python::CasePattern => {
                bare_underscore = pattern_is_bare_underscore(&child, underscore_id);
                if !bare_underscore {
                    return true;
                }
            }
            _ => {}
        }
    }
    !bare_underscore
}

// Returns `true` iff a Ruby `in_clause` pattern-match arm (`case … in`)
// counts as a non-trivial decision: either its pattern is not a bare
// `_`, or the arm carries an `if` / `unless` guard. A bare wildcard
// `in _` with no guard is Ruby's `case … in` default arm and is
// filtered out, mirroring Rust's bare-`_` `MatchArm` rule and Python's
// `case _:` rule (#977).
//
// Shared between the `Cyclomatic` and `Abc` implementations for
// `RubyCode`. Ruby surfaces the wildcard as an `identifier` whose
// source text is `_` (there is no dedicated underscore token, unlike
// Rust / Python), so the pattern check reads the byte slice rather than
// matching a kind id. The pattern is the first *named* child — the `in`
// keyword token is anonymous and the guard / body follow the pattern.
pub(crate) fn ruby_in_clause_counts(in_clause: &Node, source: &[u8]) -> bool {
    let mut pattern: Option<Node> = None;
    for child in in_clause.children() {
        match child.kind_id().into() {
            // `_guard` is the hidden supertype; `if_guard` / `unless_guard`
            // are the concrete nodes the grammar emits (lesson #34).
            Ruby::Guard | Ruby::IfGuard | Ruby::UnlessGuard => return true,
            _ if pattern.is_none() && child.is_named() => pattern = Some(child),
            _ => {}
        }
    }
    pattern.is_none_or(|pat| {
        !(matches!(pat.kind_id().into(), Ruby::Identifier) && pat.utf8_text(source) == Some("_"))
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

// Single normalization point for Python's aliased `block` kind_ids.
//
// tree-sitter-python lists two `kind_id`s that both stringify to
// `"block"`: `Block` (135, the hidden `_block` supertype) and
// `Block2` (160, the concrete production). Empirically only `Block2`
// is ever emitted for real block bodies (function, class, if/for,
// while/try/with), so `Block` is dead today — but a future grammar
// bump could promote the supertype to a concrete node. Routing every
// "is this a block?" check through here means such a bump is handled
// at one site instead of silently undercounting at several (issue
// #419; lesson 2 / 34 / 56 in docs/development/lessons_learned.md).
pub(crate) fn python_is_block(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Block | Python::Block2)
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
            matches!(
                id.into(),
                Kotlin::Private | Kotlin::Protected | Kotlin::Internal
            )
        })
        .is_none()
}
