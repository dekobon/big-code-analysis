//! Stack-depth bounds for the crate's recursive types.
//!
//! [`FuncSpace`](crate::FuncSpace), [`Ops`](crate::Ops), and
//! [`AstNode`](crate::AstNode) are trees whose nesting depth is
//! caller-controlled: nested functions, nested closures, and nested
//! expressions are ordinary legal constructs in every supported language.
//! Issues #700 / #709 converted every AST *traversal* to an explicit work
//! stack, but two implicit recursions survived on the types themselves —
//! `Serialize` and the compiler-generated `Drop` glue. Overflowing the
//! stack in either is not a catchable panic: the runtime aborts the
//! process with `SIGABRT`, taking every in-flight `bca-web` request with
//! it (#1056).
//!
//! `Drop` is made iterative outright by [`impl_iterative_drop`].
//! `Serialize` cannot be: `serde` offers no way to emit a tree without one
//! native frame per level, because `serialize_field` must run the child's
//! `Serialize` to completion before it returns. So it is bounded instead —
//! [`serialize_bounded`] refuses to descend past a per-type limit and
//! returns an ordinary serializer error, mirroring the 128-level recursion
//! limit `serde_json`'s `Deserializer` already applies to the same shapes.

use std::cell::Cell;

use serde::{Serialize, Serializer, ser::Error as _};

thread_local! {
    /// Levels of bounded nesting currently being serialized on this thread.
    ///
    /// A thread-local rather than a parameter because `serde`'s
    /// `serialize_with` hook receives only the field and the serializer.
    /// It is shared by every bounded type; the crate's recursive types
    /// never nest inside one another, and if they ever did, sharing the
    /// counter is the conservative direction.
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Restores [`DEPTH`] when a bounded level finishes, including when the
/// serializer returns an error or a `Serialize` impl further down panics.
struct DepthGuard;

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Serializes a recursive type's child collection one level deeper.
///
/// `limit` is the greatest number of nested child levels below the root
/// that will be emitted; `type_name` names the offending type in the
/// error. A deeper tree fails with a serializer error — reported by the
/// caller like any other output failure — rather than recursing far enough
/// to overflow the thread stack.
pub(crate) fn serialize_bounded<S, T>(
    children: &[T],
    limit: usize,
    type_name: &str,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    // A leaf's empty child list recurses no further, so it costs no depth.
    // Skipping it keeps `limit` an exact count of *nested* levels and
    // spares the counter on the majority of nodes in any real tree.
    if children.is_empty() {
        return children.serialize(serializer);
    }
    let depth = DEPTH.with(|d| {
        let entered = d.get() + 1;
        d.set(entered);
        entered
    });
    let _guard = DepthGuard;
    if depth > limit {
        return Err(S::Error::custom(format!(
            "{type_name} nesting is deeper than the serialization limit of {limit} levels"
        )));
    }
    children.serialize(serializer)
}

/// Implements `Drop` for a tree type so teardown costs no stack depth.
///
/// `$children` names the field holding the node's children. Every
/// descendant is hoisted into one flat work list, so a node is dropped
/// only after its own children have been moved out of it and its
/// compiler-generated glue finds an empty list — the recursion is one
/// level deep regardless of the tree's shape.
macro_rules! impl_iterative_drop {
    ($ty:ty, $children:ident) => {
        impl Drop for $ty {
            fn drop(&mut self) {
                let mut pending = ::std::mem::take(&mut self.$children);
                while let Some(mut node) = pending.pop() {
                    pending.append(&mut node.$children);
                }
            }
        }
    };
}

pub(crate) use impl_iterative_drop;

#[cfg(test)]
mod tests {
    use super::*;

    /// The guard must restore the counter on the error path, or one
    /// rejected tree would poison every later serialization on the thread.
    #[test]
    fn depth_counter_is_restored_after_a_rejection() {
        let deep = vec![vec![vec![0_u8]]];
        // Limit 0 rejects immediately at the first bounded level.
        let mut out = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut out);
        let err = serialize_bounded(&deep, 0, "Probe", &mut ser).expect_err("limit 0 must reject");
        assert!(
            err.to_string().contains("serialization limit of 0 levels"),
            "error must name the limit, got: {err}"
        );
        assert_eq!(DEPTH.with(Cell::get), 0, "counter must unwind to zero");

        // The same thread must still serialize a shallow value afterwards.
        let mut out = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut out);
        serialize_bounded(&deep, 1, "Probe", &mut ser).expect("limit 1 accepts one level");
        assert_eq!(DEPTH.with(Cell::get), 0, "counter must unwind to zero");
        assert_eq!(String::from_utf8(out).expect("utf-8"), "[[[0]]]");
    }
}
