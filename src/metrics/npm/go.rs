//! `Npm` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npm for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Go as G;

        match node.kind_id().into() {
            // Each receiver method contributes one to the per-space
            // count; `compute_sum` rolls it into `class_nm_sum`, and
            // the parent's merge bubbles it up to the Unit, which is
            // where a Go file's npm is reported — Go has no container
            // space to attach it to.
            G::MethodDeclaration => {
                stats.class_nm += 1;
                // Go's export rule is lexical (issue #458): the method
                // is public iff its own name's first character is an
                // uppercase Unicode letter — the receiver's visibility
                // is irrelevant. The method name is the
                // `FieldIdentifier` child of the declaration.
                let exported = node
                    .children()
                    .find(|c| matches!(c.kind_id().into(), G::FieldIdentifier))
                    .and_then(|name| name.utf8_text(code))
                    .is_some_and(super::npa::go_is_exported);
                if exported {
                    stats.class_npm += 1;
                }
            }
            // `interface { Foo(); Bar() int }` declares method
            // signatures via `MethodElem` children of an
            // `InterfaceType`. Interfaces have no func_space of
            // their own, so the count lands on the enclosing space
            // (typically Unit). Embedded interfaces (`io.Reader`)
            // parse as `TypeElem`, not `MethodElem`, so they are not
            // counted here. Go's export rule is lexical and applies
            // to interface method names too (issue #471, the
            // interface-side analog of #458): a signature is public
            // iff its name's first character is an uppercase Unicode
            // letter. `interface_nm` counts every method element;
            // `interface_npm` only the exported ones.
            G::InterfaceType => {
                let mut methods = 0_usize;
                let mut exported = 0_usize;
                for method in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), G::MethodElem))
                {
                    methods += 1;
                    // The method name is the `FieldIdentifier` child,
                    // mirroring the `MethodDeclaration` arm; a missing
                    // name is treated as not-exported.
                    let is_exported = method
                        .children()
                        .find(|c| matches!(c.kind_id().into(), G::FieldIdentifier))
                        .and_then(|name| name.utf8_text(code))
                        .is_some_and(super::npa::go_is_exported);
                    if is_exported {
                        exported += 1;
                    }
                }
                stats.interface_nm += methods;
                stats.interface_npm += exported;
            }
            _ => {}
        }
    }
}
