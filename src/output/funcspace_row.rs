// bca: suppress-file(halstead, abc)
// `metric_values` is a flat array literal of metric accessor calls; the
// halstead/abc offenders are call-count aggregation artifacts, not
// per-function logic complexity.

//! Row-shape helpers for the CSV output format.
//!
//! The CSV writer walks the FuncSpace tree and emits a flat per-space
//! metric matrix. The metric tuple and identity-column count live in
//! their own module so the column-count invariant
//! (`IDENTITY_COLUMNS + METRIC_COUNT == CSV_HEADER.len()`) is asserted
//! once at compile time in `csv.rs` rather than scattered across the
//! row-walk site.

// The CSV matrix is `[f64; N]`; integral `u64` metric accessors are widened
// with `as f64` to fit the uniform column type (#530). Each cast is bounded
// by the count it came from.
#![allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::cast_precision_loss
)]

use crate::FuncSpace;

/// Number of identity columns that come before the metric columns:
/// `path`, `space_name`, `space_kind`, `start_line`, `end_line`.
pub(crate) const IDENTITY_COLUMNS: usize = 5;

/// Number of metric columns produced by [`metric_values`]. The
/// per-section breakdown matches the comment dividers in the function
/// body.
pub(crate) const METRIC_COUNT: usize = 111;

/// Flatten a single FuncSpace's metrics into the flat column order
/// used by CSV output. The order is the public contract exposed via
/// `output::csv::CSV_HEADER`; callers iterate the returned array in
/// lockstep with `CSV_HEADER[IDENTITY_COLUMNS..]`.
pub(crate) fn metric_values(space: &FuncSpace) -> [f64; METRIC_COUNT] {
    let m = &space.metrics;
    let cyc = &m.cyclomatic;
    let cog = &m.cognitive;
    let hal = &m.halstead;
    let l = &m.loc;
    let nm = &m.nom;
    let nrg = &m.nargs;
    let nex = &m.nexits;
    let tok = &m.tokens;
    let a = &m.abc;
    let w = &m.wmc;
    let pm = &m.npm;
    let pa = &m.npa;
    let mi_ = &m.mi;

    [
        // cognitive
        cog.cognitive_sum() as f64,
        cog.cognitive_average(),
        cog.cognitive_min() as f64,
        cog.cognitive_max() as f64,
        // cyclomatic
        cyc.cyclomatic_sum() as f64,
        cyc.cyclomatic_average(),
        cyc.cyclomatic_min() as f64,
        cyc.cyclomatic_max() as f64,
        cyc.cyclomatic_modified_sum() as f64,
        cyc.cyclomatic_modified_average(),
        cyc.cyclomatic_modified_min() as f64,
        cyc.cyclomatic_modified_max() as f64,
        // halstead
        hal.unique_operators() as f64,
        hal.total_operators() as f64,
        hal.unique_operands() as f64,
        hal.total_operands() as f64,
        hal.length() as f64,
        hal.estimated_program_length(),
        hal.purity_ratio(),
        hal.vocabulary() as f64,
        hal.volume(),
        hal.difficulty(),
        hal.level(),
        hal.effort(),
        hal.time(),
        hal.bugs(),
        // loc
        l.sloc() as f64,
        l.ploc() as f64,
        l.lloc() as f64,
        l.cloc() as f64,
        l.blank() as f64,
        l.sloc_average(),
        l.ploc_average(),
        l.lloc_average(),
        l.cloc_average(),
        l.blank_average(),
        l.sloc_min() as f64,
        l.sloc_max() as f64,
        l.cloc_min() as f64,
        l.cloc_max() as f64,
        l.ploc_min() as f64,
        l.ploc_max() as f64,
        l.lloc_min() as f64,
        l.lloc_max() as f64,
        l.blank_min() as f64,
        l.blank_max() as f64,
        // nom
        nm.functions_sum() as f64,
        nm.closures_sum() as f64,
        nm.functions_average(),
        nm.closures_average(),
        nm.total() as f64,
        nm.average(),
        nm.functions_min() as f64,
        nm.functions_max() as f64,
        nm.closures_min() as f64,
        nm.closures_max() as f64,
        // nargs
        nrg.function_args_sum() as f64,
        nrg.closure_args_sum() as f64,
        nrg.function_args_average(),
        nrg.closure_args_average(),
        nrg.total() as f64,
        nrg.average(),
        nrg.function_args_min() as f64,
        nrg.function_args_max() as f64,
        nrg.closure_args_min() as f64,
        nrg.closure_args_max() as f64,
        // nexits
        nex.nexits_sum() as f64,
        nex.nexits_average(),
        nex.nexits_min() as f64,
        nex.nexits_max() as f64,
        // tokens
        tok.tokens_sum() as f64,
        tok.tokens_average(),
        tok.tokens_min() as f64,
        tok.tokens_max() as f64,
        // abc
        a.assignments_sum() as f64,
        a.branches_sum() as f64,
        a.conditions_sum() as f64,
        a.magnitude_sum(),
        a.assignments_average(),
        a.branches_average(),
        a.conditions_average(),
        a.assignments_min() as f64,
        a.assignments_max() as f64,
        a.branches_min() as f64,
        a.branches_max() as f64,
        a.conditions_min() as f64,
        a.conditions_max() as f64,
        // wmc
        w.class_wmc_sum() as f64,
        w.interface_wmc_sum() as f64,
        w.total_wmc() as f64,
        // npm
        pm.class_npm_sum() as f64,
        pm.interface_npm_sum() as f64,
        pm.class_nm_sum() as f64,
        pm.interface_nm_sum() as f64,
        pm.class_coa(),
        pm.interface_coa(),
        pm.total_npm() as f64,
        pm.total_nm() as f64,
        pm.total_coa(),
        // npa
        pa.class_npa_sum() as f64,
        pa.interface_npa_sum() as f64,
        pa.class_na_sum() as f64,
        pa.interface_na_sum() as f64,
        pa.class_cda(),
        pa.interface_cda(),
        pa.total_npa() as f64,
        pa.total_na() as f64,
        pa.total_cda(),
        // mi
        mi_.original(),
        mi_.sei(),
        mi_.visual_studio(),
    ]
}
