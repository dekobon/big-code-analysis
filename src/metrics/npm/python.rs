//! `Npm` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Python method counting.
//
// A "method" is a `FunctionDefinition` direct child of a class body
// (the `block` under a `ClassDefinition`), including decorated
// methods such as `@property`, `@staticmethod`, `@classmethod` and
// user decorators — those wrap the inner function in a
// `DecoratedDefinition` node, so we unwrap and count once.
//
// Python has no visibility keyword. The PEP-8 convention `_x` for
// "internal" and `__x` for "name-mangled private" is purely advisory
// and not represented in the AST. `Npm::compute` is also called
// without source bytes, so reading the identifier text is not
// possible from this trait. We therefore treat every class method as
// public — `class_npm == class_nm`.
//
// Nested classes and async functions are handled naturally:
// `async def m(self):` still parses as `FunctionDefinition`, so the
// `is_func` check covers it without special-casing. Nested
// `ClassDefinition` children of a class body are skipped here — they
// open their own class space, where their methods will be counted.
impl Npm for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Python::*;

        // Gate on `ClassDefinition` specifically so the flag is not
        // set on plain function or module spaces.
        if !matches!(node.kind_id().into(), ClassDefinition) {
            return;
        }

        if stats.is_disabled() {
            stats.is_class_space = true;
        }

        // The class body is the `block` child; route through
        // `python_is_block` so both aliased kind_ids (`Block` /
        // `Block2`) are accepted at one normalization point (#419).
        let Some(body) = node.children().find(python_is_block) else {
            return;
        };

        // Count direct-child method declarations. Decorated methods
        // appear under a `DecoratedDefinition` wrapper; walk into
        // that wrapper to find the inner `FunctionDefinition`.
        let count = body
            .children()
            .filter(|stmt| match stmt.kind_id().into() {
                FunctionDefinition => true,
                DecoratedDefinition => stmt.children().any(|c| c.kind_id() == FunctionDefinition),
                _ => false,
            })
            .count();

        stats.class_nm += count;
        // No visibility modifier in Python — every method is "public".
        stats.class_npm += count;
    }
}
