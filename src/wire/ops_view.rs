//! `crate::Ops`'s serialize path: a borrowed mirror of [`super::Ops`].
//!
//! Every other compute type serializes by materializing its owned wire
//! projection first, and [`super`]'s module docs explain why that is the
//! right default. `Ops` is the exception, because there the projection is
//! the expensive part of the operation rather than a rounding error on
//! it. `ops::finalize` merges each child space's Halstead maps into its
//! parent, so a parent's vocabulary is a superset of every descendant's
//! and an owned projection re-clones an entry once per enclosing space.
//! Building the owned tree was 80% of `serde_json::to_string` on a
//! hundred-level nest of functions with distinct identifiers, and *all*
//! of it on a tree past [`MAX_SPACE_SERIALIZE_DEPTH`] — 2 000 levels
//! cloned in full and then dropped unserialized (#1110).
//!
//! The price is a second field list, which is the drift the parent
//! module exists to prevent, so [`OpsView`]'s fields must match
//! [`super::Ops`]'s in name, order, type and `skip_serializing_if`.
//! `borrowed_and_owned_ops_projections_serialize_alike` pins that in all
//! four output formats, and its sibling covers the two fields a parsed
//! fixture cannot reach.

use serde::{Serialize, Serializer};

use super::{MAX_SPACE_SERIALIZE_DEPTH, SpaceKind, ops};

/// Borrowed, serialize-only mirror of [`super::Ops`].
#[derive(Serialize)]
struct OpsView<'a> {
    name: &'a Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    name_was_lossy: bool,
    start_line: usize,
    end_line: usize,
    kind: SpaceKind,
    #[serde(serialize_with = "serialize_ops_view_spaces")]
    spaces: &'a [ops::Ops],
    operands: &'a [String],
    operators: &'a [String],
}

/// Serializes a `crate::Ops` node's children one level deeper, under
/// the bound [`super::serialize_ops_spaces`] applies to the owned tree.
/// Each child re-enters [`OpsView`], so nothing below the emitted levels
/// is ever visited.
fn serialize_ops_view_spaces<S: Serializer>(
    spaces: &&[ops::Ops],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    crate::recursion::serialize_bounded(spaces, MAX_SPACE_SERIALIZE_DEPTH, "Ops", serializer)
}

impl Serialize for ops::Ops {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        OpsView {
            name: &self.name,
            name_was_lossy: self.name_was_lossy,
            start_line: self.start_line,
            end_line: self.end_line,
            kind: self.kind,
            spaces: &self.spaces,
            operands: &self.operands,
            operators: &self.operators,
        }
        .serialize(serializer)
    }
}
