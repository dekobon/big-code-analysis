//! Inherent and `Display` impl blocks for [`super::SpaceKind`].
//!
//! Split out of `spaces.rs` to keep that module focused on the public
//! API type definitions. The blocks are moved verbatim; method and
//! trait resolution is by type, so `crate::spaces::SpaceKind`'s methods
//! and `Display` impl resolve unchanged.

use super::*;

impl SpaceKind {
    /// Parse a [`SpaceKind`] from its lowercase serialized form — the
    /// `#[serde(rename_all = "lowercase")]` representation that appears in
    /// the JSON / wire `kind` field. An unrecognized string maps to
    /// [`SpaceKind::Unknown`] so a JSON-walking front-end degrades
    /// gracefully on a future kind rather than erroring.
    ///
    /// This is the single source of truth for the string-to-kind mapping a
    /// consumer needs when it reads a serialized `kind` (the Python
    /// `to_sarif` binding uses it to apply per-metric threshold scope via
    /// [`crate::metric_catalog::MetricScope::admits`]). A round-trip test
    /// pins it against the serde representation so the two cannot drift.
    #[must_use]
    pub fn from_serialized(serialized: &str) -> Self {
        match serialized {
            "function" => Self::Function,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "trait" => Self::Trait,
            "impl" => Self::Impl,
            "unit" => Self::Unit,
            "namespace" => Self::Namespace,
            "interface" => Self::Interface,
            _ => Self::Unknown,
        }
    }

    /// Whether the object-oriented member metrics — `wmc`, `npm`, `npa` —
    /// are meaningful on a space of this kind.
    ///
    /// True for every container (which owns methods and attributes) and
    /// for the file [`Unit`](SpaceKind::Unit) (which aggregates its
    /// containers' counts into a whole-file roll-up). False for a
    /// function space, which owns neither, and for
    /// [`Unknown`](SpaceKind::Unknown).
    ///
    /// This is the single definition the three metrics share. `wmc`
    /// carried it alone as an inline `matches!`; `npm` and `npa` enabled
    /// themselves from `Checker::is_func_space` instead, which admits
    /// function spaces and so gave a Kotlin `<get>` or a JavaScript method
    /// an all-zero block its sibling method did not have (#1197).
    ///
    /// Phrased as an exclusion so that a future [`SpaceKind`] variant —
    /// the enum is `#[non_exhaustive]` — defaults to *carrying* the
    /// metrics. A new kind is far likelier to be another container than
    /// another callable, and an extra roll-up is a milder wrong answer
    /// than a silently missing one.
    #[must_use]
    pub(crate) fn is_member_scope(self) -> bool {
        !matches!(self, Self::Function | Self::Unknown)
    }
}

impl fmt::Display for SpaceKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            SpaceKind::Unknown => "unknown",
            SpaceKind::Function => "function",
            SpaceKind::Class => "class",
            SpaceKind::Struct => "struct",
            SpaceKind::Trait => "trait",
            SpaceKind::Impl => "impl",
            SpaceKind::Unit => "unit",
            SpaceKind::Namespace => "namespace",
            SpaceKind::Interface => "interface",
        };
        write!(f, "{s}")
    }
}
