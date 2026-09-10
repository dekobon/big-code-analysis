//! The kinds of space the walk opens: functions and the containers
//! (class, struct, trait, impl, namespace, interface) plus the file unit.
//!
//! Defined beside the classifiers because `Getter::get_space_kind`
//! returns it; `spaces` re-exports it under its historical public path
//! (#1376).

use std::fmt;

use serde::{Deserialize, Serialize};

/// The list of supported space kinds.
// New space kinds land as languages are added (a future module-, mixin-,
// or enum-style space), so this is marked `#[non_exhaustive]` to keep
// such additions additive rather than a 2.0 break. CLI/web consumers
// matching on it already carry a `_ =>` arm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SpaceKind {
    /// An unknown space
    #[default]
    Unknown,
    /// A function space
    Function,
    /// A class space
    Class,
    /// A struct space
    Struct,
    /// A `Rust` trait space
    Trait,
    /// A `Rust` implementation space
    Impl,
    /// A general space
    Unit,
    /// A `C/C++` namespace
    Namespace,
    /// An interface
    Interface,
}

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
    /// `metric_catalog::MetricScope::admits`). A round-trip test
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
