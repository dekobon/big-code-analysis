//! Container-qualified symbol names for [`FuncSpace`]s.
//!
//! Both the `bca check` offender output and the `bca report` hotspot
//! tables identify a nested function by its enclosing container so two
//! functions that share a bare AST name stay distinguishable. The
//! motivating case is the per-language `impl Abc for <Lang>Code { fn
//! compute(...) }` family in `src/metrics/abc.rs`: ~20 distinct methods
//! all named `compute`, which the bare AST name collapses into
//! indistinguishable rows.
//!
//! The two surfaces share the [`space_segment`] primitive but differ in
//! how much chain they prefix:
//!
//! - `bca check` uses [`qualified_symbol`] — the full `::`-joined
//!   enclosing chain (`Outer::Inner::method`) — because an offender line
//!   is a precise location key (baseline matching, suppression).
//! - `bca report` prefixes only the *immediate* container
//!   (`RustCode::compute`), keeping each name O(1) so a pathologically
//!   deep AST cannot blow report memory to O(depth²). For the shallow
//!   nesting real code produces the two forms agree.

use big_code_analysis::{FuncSpace, SpaceKind};

/// One `::`-segment for a space in the qualified-symbol chain.
///
/// Named spaces contribute their AST-derived name. Anonymous spaces —
/// closures and lambdas, which every grammar surfaces as the literal
/// `<anonymous>`, plus the `None`-name parse-failure case — collapse to
/// `<anon@L{start_line}>` so they keep a stable-within-a-snapshot
/// identity. Baking the line into the segment means an anonymous function
/// re-keys when it moves (the documented degradation in
/// `recipes/baselines.md`); named functions do not.
pub(crate) fn space_segment(space: &FuncSpace) -> String {
    const ANONYMOUS: &str = "<anonymous>";
    match space.name.as_deref() {
        Some(name) if name != ANONYMOUS => name.to_owned(),
        _ => format!("<anon@L{}>", space.start_line),
    }
}

/// The qualified symbol of `space`, given the `::`-joined symbol of its
/// enclosing chain (`parent_prefix`, empty at file top level).
///
/// The top-level (`Unit`) space is the file itself; it carries no symbol
/// segment (its identity is the `path` key) so it collapses to `<file>`
/// and never prefixes the functions inside it. Everything else appends
/// its [`space_segment`] to the parent prefix.
pub(crate) fn qualified_symbol(space: &FuncSpace, parent_prefix: &str) -> String {
    if matches!(space.kind, SpaceKind::Unit) {
        return "<file>".to_owned();
    }
    let segment = space_segment(space);
    if parent_prefix.is_empty() {
        segment
    } else {
        format!("{parent_prefix}::{segment}")
    }
}
