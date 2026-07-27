pub(crate) mod color;
pub use color::ColorMode;

pub(crate) mod dump;
pub use dump::*;

pub(crate) mod dump_metrics;
pub use dump_metrics::*;

pub(crate) mod dump_ops;
pub use dump_ops::*;

pub mod offenders;
pub use offenders::{OffenderRecord, Severity, TOOL_ID};

pub(crate) mod numfmt;

pub mod checkstyle;
pub use checkstyle::write_checkstyle;

pub(crate) mod funcspace_row;

pub mod csv;
pub use csv::{CSV_EXTENSION, CSV_HEADER, defang_formula, write_csv, write_csv_aggregate};

pub mod sarif;
pub use sarif::{write_sarif, write_sarif_with_suppressed};

pub mod code_climate;
pub use code_climate::write_code_climate;

pub mod warning_line;
pub use warning_line::{write_clang_warning, write_msvc_warning};

/// The `(child, own)` ASCII prefix pair for one line of the `metrics` and
/// `ops` text trees: what the node contributes to its children's
/// indentation, and the connector on its own line. `` `-  `` closes a
/// subtree, `|- ` continues it.
///
/// Shared by [`dump_metrics`] and [`dump_ops`], which render the same
/// glyph vocabulary over two different trees, so the two mirrored walks
/// cannot drift apart. The AST dump ([`dump`]) deliberately uses Unicode
/// box-drawing glyphs instead and has its own connector type.
pub(crate) const fn branch_glyphs(last: bool) -> (&'static str, &'static str) {
    if last { ("   ", "`- ") } else { ("|  ", "|- ") }
}
