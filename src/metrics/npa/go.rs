//! `Npa` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Go attribute counting.
//
// Go has no `class` concept; struct types declared at file scope
// (`type Foo struct { … }`) play that role. Methods live separately
// as `MethodDeclaration` nodes attached to a receiver type. Because
// `StructType` is NOT a func_space (per `Checker::is_func_space`),
// the iterator visits it with the enclosing func_space's stats
// (typically the file-level `Unit`). Each declared field NAME counts
// as one attribute: a grouped declaration `A, b int` declares two
// names (two attributes), and an embedded type (`io.Reader`, `*Foo`)
// declares one attribute whose name is the embedded type's base
// identifier (`Reader`, `Foo`).
//
// Visibility (issue #458): Go's export rule is purely lexical — an
// identifier is exported (public) iff its first character is an
// uppercase Unicode letter (`unicode.IsUpper`). `Npa::compute`
// receives the source bytes as `code`, so we read each field name's
// text and gate the public count (`class_npa`) on `go_is_exported`.
// The total count (`class_na`) stays unconditional. The blank
// identifier `_` is never exported.
//
// Limitations:
// - Struct fields are attributed to the enclosing func_space (the
//   file's `Unit`, or a local function space for `type T struct{…}`
//   declared inside a function body). Multiple structs at the same
//   level contribute to the same `class_na` bucket. This mirrors the
//   Rust impl's "fields land on the enclosing space" approach.
// - Interface methods (`interface { Foo() }`) are not attributes —
//   they are method signatures, counted by Npm under
//   `interface_nm`, not by Npa.
impl Npa for GoCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Go as G;

        if !matches!(node.kind_id().into(), G::StructType) {
            return;
        }

        // The struct body is the `field_declaration_list` direct
        // child. An empty struct (`struct{}`) has the list with no
        // FieldDeclaration children → 0 attributes.
        let Some(body) = node
            .children()
            .find(|c| matches!(c.kind_id().into(), G::FieldDeclarationList))
        else {
            return;
        };

        // Tally per declared NAME: named fields carry one or more
        // `FieldIdentifier` children (`A, b int` → two names), while
        // an embedded field carries none and contributes the embedded
        // type's base name. Public count is gated on Go's lexical
        // export rule; the total is unconditional.
        let mut total = 0usize;
        let mut public = 0usize;
        for declaration in body
            .children()
            .filter(|c| matches!(c.kind_id().into(), G::FieldDeclaration))
        {
            let mut names = declaration
                .children()
                .filter(|c| matches!(c.kind_id().into(), G::FieldIdentifier))
                .peekable();
            if names.peek().is_some() {
                for name in names {
                    total += 1;
                    if name.utf8_text(code).is_some_and(go_is_exported) {
                        public += 1;
                    }
                }
            } else if let Some(name) = go_embedded_field_name(&declaration) {
                total += 1;
                if name.utf8_text(code).is_some_and(go_is_exported) {
                    public += 1;
                }
            }
        }

        if total == 0 {
            return;
        }

        if stats.is_disabled() {
            stats.is_class_space = true;
        }
        stats.class_na += total;
        stats.class_npa += public;
    }
}
