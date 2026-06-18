//! `Npa` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Rust attribute counting.
//
// Rust's "class" maps to a `struct` plus its `impl` blocks. Since each
// `impl` block opens its own func_space (`SpaceKind::Impl`), the
// natural place to record attributes per "class" is at the impl space
// and at the struct itself:
//
// 1. `StructItem`: every direct child in the struct's
//    `field_declaration_list` (named fields) or
//    `ordered_field_declaration_list` (tuple-struct positional fields)
//    is one attribute. Because `struct_item` is NOT a func_space, the
//    fields are attributed to whichever func_space is on the stack
//    when the StructItem is visited (typically `Unit`). The enclosing
//    space is marked as a class space so the npa metric is emitted.
//
// 2. `ImplItem`: every `ConstItem` and `StaticItem` direct child of the
//    impl's `declaration_list` is one associated attribute. These
//    accumulate on the Impl space (which is itself a class-style
//    func_space).
//
// 3. `TraitItem`: every `ConstItem`, `StaticItem`, and `AssociatedType`
//    direct child of the trait's `declaration_list` is one attribute.
//    Trait members are always visible to implementers, so they are
//    counted as public (`interface_npa == interface_na`), mirroring
//    Java's interface-body rule.
//
// Limitations (documented):
// - Multiple `impl Foo` blocks each open their own Impl space and
//   accumulate independently. Their `_sum` accumulators roll up to
//   the parent during finalisation, so the file-level
//   `class_npa_sum` is the sum across every impl.
// - Struct fields are attributed to the enclosing func_space (usually
//   Unit), not to a per-struct space. Two structs in the same module
//   therefore contribute to the same `class_na` bucket on that Unit.
//   This matches the issue's intent of "count struct fields + impl
//   associated consts" without inventing a synthetic per-struct
//   space.
// - Enum variants are NOT counted as attributes (they are sum-type
//   tags, not data fields), mirroring Kotlin's `enum_class_body`
//   treatment.
impl Npa for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Rust::*;

        // Mark Impl / Trait spaces as class spaces so the metric is
        // emitted on them.
        if matches!(node.kind_id().into(), ImplItem | TraitItem) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            // Counted on the StructItem so each struct's fields are
            // tallied exactly once. The enclosing func_space (Unit or
            // nested) is the recipient — marking it a class space
            // makes the npa metric visible.
            StructItem => rust_count_struct_attrs(node, stats),
            // Associated const/static declared in an `impl` block.
            // The current top-of-stack is the Impl space (because we
            // are inside its body), so attribution lands there.
            ConstItem | StaticItem => rust_count_assoc_const(node, stats),
            // `type Foo;` inside a trait body is an associated type —
            // a placeholder bound that the implementer must supply.
            // Counted as an interface attribute, public by default.
            AssociatedType => rust_count_assoc_type(node, stats),
            _ => {}
        }
    }
}

// Counts the fields of a Rust `struct_item` and records them on the
// enclosing func_space. Empty structs (`attrs == 0`) record nothing and
// do not mark the space as a class space, so a fieldless marker struct
// never emits a spurious npa metric.
fn rust_count_struct_attrs(node: &Node, stats: &mut Stats) {
    use Rust::*;

    let mut attrs = 0;
    let mut public_attrs = 0;
    for body in node.children() {
        match body.kind_id().into() {
            // Named-field struct: each `field_declaration`
            // is one attribute. Visibility is the
            // `visibility_modifier` first child.
            FieldDeclarationList => {
                for field in body
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                {
                    attrs += 1;
                    if rust_item_is_public(&field) {
                        public_attrs += 1;
                    }
                }
            }
            // Tuple struct: the field count is positional.
            // The grammar emits each field as either a
            // type-bearing node (`primitive_type`,
            // `type_identifier`, `generic_type`, ...) or a
            // `visibility_modifier` followed by such a
            // node. We count one attribute per non-token
            // child that is not a delimiter, comma, or
            // visibility modifier.
            OrderedFieldDeclarationList => {
                let (count, public) = rust_count_tuple_struct_fields(&body);
                attrs += count;
                public_attrs += public;
            }
            _ => {}
        }
    }
    if attrs > 0 {
        if stats.is_disabled() {
            stats.is_class_space = true;
        }
        stats.class_na += attrs;
        stats.class_npa += public_attrs;
    }
}

// Counts an associated `const`/`static` declared directly in an `impl`
// or `trait` body. Impl members count toward the class attribute totals
// (public if `pub`); trait members are always visible to implementers,
// so `interface_npa` tracks `interface_na`.
fn rust_count_assoc_const(node: &Node, stats: &mut Stats) {
    use Rust::*;

    let Some(parent) = node.parent() else {
        return;
    };
    let Some(grand) = parent.parent() else {
        return;
    };
    match grand.kind_id().into() {
        ImplItem if matches!(parent.kind_id().into(), DeclarationList) => {
            stats.class_na += 1;
            if rust_item_is_public(node) {
                stats.class_npa += 1;
            }
        }
        TraitItem if matches!(parent.kind_id().into(), DeclarationList) => {
            stats.interface_na += 1;
            stats.interface_npa = stats.interface_na;
        }
        _ => {}
    }
}

// Counts a trait-body `associated_type` (`type Foo;`) as one interface
// attribute, public by default — the implementer must supply it.
fn rust_count_assoc_type(node: &Node, stats: &mut Stats) {
    use Rust::*;

    let Some(parent) = node.parent() else {
        return;
    };
    let Some(grand) = parent.parent() else {
        return;
    };
    if matches!(grand.kind_id().into(), TraitItem)
        && matches!(parent.kind_id().into(), DeclarationList)
    {
        stats.interface_na += 1;
        stats.interface_npa = stats.interface_na;
    }
}

// Counts positional fields inside an `ordered_field_declaration_list`
// (tuple struct). Each non-token child that is a type node represents
// one field. A leading `visibility_modifier` may decorate the field;
// counts that field as public. Returns `(total_count, public_count)`.
fn rust_count_tuple_struct_fields(list: &Node) -> (usize, usize) {
    use Rust::*;

    let mut total = 0;
    let mut public = 0;
    let mut pending_pub = false;
    for child in list.children() {
        match child.kind_id().into() {
            // Open / close parens and comma separators — skipped.
            LPAREN | RPAREN | COMMA => {
                pending_pub = false;
            }
            // `pub` / `pub(crate)` / `pub(super)` / `pub(in <path>)` —
            // applies to the next type child. `pub(self)` / `pub(in self)`
            // restrict to the current module (private) and must not count
            // as public (issue #460): the modifier has a direct `Zelf`
            // child for exactly those forms.
            VisibilityModifier => {
                pending_pub = rust_visibility_modifier_is_public(&child);
            }
            // `attribute_item` decorates the next field but does not
            // contribute to visibility. Skip without resetting the
            // pending-pub flag. `line_comment` / `block_comment` may
            // sit between fields (e.g. `pub struct Foo(/* x */ i32);`)
            // and similarly must not count as a field.
            AttributeItem | LineComment | BlockComment => {}
            // Any other child is treated as a positional field type
            // (primitive_type, type_identifier, generic_type,
            // reference_type, tuple_type, ...). One increment per
            // type child.
            _ => {
                total += 1;
                if pending_pub {
                    public += 1;
                }
                pending_pub = false;
            }
        }
    }
    (total, public)
}
