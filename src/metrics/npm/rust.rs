//! `Npm` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Rust method counting.
//
// A "method" in Rust is a `function_item` direct child of an `impl`
// block's `declaration_list`. In a `trait_item`, both `function_item`
// (default-body methods) and `function_signature_item` (signature-only
// methods that implementers must provide) count toward the trait's
// interface methods. Trait methods are always visible to implementers
// and are therefore counted as public, matching Java's interface rule
// (`interface_npm == interface_nm`).
//
// `pub` / `pub(crate)` / `pub(super)` / `pub(in ...)` mark an impl
// method as public; absence of any visibility modifier means private.
// The `pub(crate)` form is intentionally counted as public because
// it's externally callable from the crate's perspective — narrower
// distinctions are tracked by `npa` / `npm` only as a binary public /
// private flag.
impl Npm for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Rust::*;

        // A method is a `function_item` or `function_signature_item`
        // whose parent is the `declaration_list` of an `impl` or
        // `trait`. Gating on the kind, parent, and grandparent keeps
        // free-standing functions out of the count without needing to
        // walk the parent list eagerly.
        if !matches!(node.kind_id().into(), FunctionItem | FunctionSignatureItem) {
            return;
        }
        let mut climb = ancestors.iter(node);
        let Some((parent, _)) = climb.next() else {
            return;
        };
        if !matches!(parent.kind_id().into(), DeclarationList) {
            return;
        }
        let Some((grand, _)) = climb.next() else {
            return;
        };
        match grand.kind_id().into() {
            ImplItem => {
                stats.class_nm += 1;
                if super::npa::rust_item_is_public(node) {
                    stats.class_npm += 1;
                }
            }
            TraitItem => {
                stats.interface_nm += 1;
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}
