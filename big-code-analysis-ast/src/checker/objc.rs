//! `Checker` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for ObjcCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Objc::Comment
    }

    impl_doxygen_doc_comment!();

    // Objective-C adds `@interface`, `@implementation`, `@protocol`,
    // `@property`, methods and instance variables; `@class C;`,
    // `@protocol P;`, `@synthesize` and `@dynamic` take none (#1613).
    impl_doxygen_doc_binding!();

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // The space tree is declared in `roles/objc.rs` (#1696): C's
    // translation unit and free functions plus the ObjC containers —
    // `@interface` / `@protocol` open `Interface`, `@implementation`
    // `Class` — and method definitions; a block literal (`^{ … }`) is a
    // closure and no space, as the C++ lambda. The rows read no bytes
    // and no chain, so the byte-less spelling forwards with none.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::objc::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // Keep in sync with `is_func_space` and the ObjC getters (#285).
    // A `method_definition` is the `@implementation`-side method with a
    // body; the `@interface`-side `method_declaration` has no body and
    // is therefore not a function.
    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::objc::func_opener::is_callable(node, &[], ancestors)
    }

    // A plain C function written inside `@implementation` / `@interface`
    // / `@protocol` is a file-static helper, not a method: no receiver,
    // absent from the method table, unsendable. `npm` already declines
    // to count it (`src/metrics/npm/objc.rs` counts `method_definition`),
    // so `wmc` must not weight it into the container either (#1356) —
    // the C++ `friend` divergence of #1301 in a sibling language.
    //
    // The node kind is the whole predicate, because an Objective-C
    // method is *always* a `method_definition`. Neither shape of parent
    // check that worked for C++ transfers:
    //
    // - `implementation_definition` wraps `function_definition` and
    //   `method_definition` alike, so a parent check alone answers
    //   `true` for every real method and zeroes the class's WMC
    //   (`.claude/rules/grammar-dispatch.md` §6).
    // - A parent-kind *list* cannot be completed. `@interface` and
    //   `@protocol` take `function_definition` as a direct child with no
    //   wrapper, and both also admit `preproc_if` — so a helper inside
    //   `#if` has a parent kind it shares with a file-scope `#if`.
    //
    // `true` at file scope is deliberate and inert: `wmc::Stats::merge`
    // credits a `Function` child only to a `Class` or `Interface`
    // parent. Both aliases ride together per the #285 contract this file
    // follows in `is_func` / `is_func_space`.
    fn is_non_member_function<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        // role-bypass: allow(FuncOpener) — which of the row's callables
        // the enclosing container does not own (a C function, never a
        // method); not whether it opens a space
        matches!(
            node.kind_id().into(),
            Objc::FunctionDefinition | Objc::FunctionDefinition2
        )
    }

    // ObjC blocks `^{ … }` are the language's closures.
    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::objc::closure::owns(node, &[], ancestors)
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    // `Call` and `StringLit` are the rows of `roles/objc.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(objc, call, is_call, is_call_with_code);
    impl_row_predicates!(objc, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::LPAREN | Objc::LPAREN2 | Objc::COMMA | Objc::RPAREN
        )
    }

    // The `ElseIf` row of `roles/objc.rs`: an `if_statement` whose
    // parent is the `else_clause`, read off the caller's chain. The row
    // reads no bytes, so the empty code is exact (#1697).
    impl_else_if_from_row!(objc);

    // A primitive is the `Primitive` row's kind: `primitive_type`, one
    // kind for `int`, `double`, …, and the two `_Complex` spellings.
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::objc::primitive::OWNERS.contains(&node.kind_id())
    }

    // Keyed by text: a `primitive_type` (one kind, many spellings); a
    // property attribute word (`nonatomic`, `copy`) — an `identifier`
    // the `Modifier` row owns under its position gate, whose kind is no
    // keyword, so keying it by text lists each word; and the `nullable`
    // type qualifier, keyed by text so it and the `nullable` property
    // attribute share one `n1` entry rather than one per map (#1579).
    // Before #1599 the byte-less `is_primitive` answered this by kind
    // alone — every `identifier` — which was right only because the
    // getter bills an identifier as an operator for an attribute word
    // and nothing else; the row's gate says which.
    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        use crate::roles::objc::{modifier, primitive};
        let kind = node.kind_id();
        // role-bypass: allow(Primitive, Modifier) — key selection after the
        // rows have classified the node: which of their kinds is one kind
        // for many spellings, not whether the node is one (the gate cannot
        // see a kind held in a local, so this says so by hand)
        primitive::owns_kind(kind, node, code, ancestors)
            || kind == Objc::Nullable as u16
            || (kind == Objc::Identifier as u16 && modifier::owns_kind(kind, node, code, ancestors))
    }
}
