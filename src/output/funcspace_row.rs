//! Shared row-shape helpers for CSV and HTML output formats.
//!
//! Both formats walk the same FuncSpace tree and produce the same
//! flat per-space metric matrix; only the per-cell rendering differs.
//! Keeping the metric tuple and identity-column count in one place
//! prevents the two formats from drifting silently when a metric is
//! added or renamed.

use crate::FuncSpace;

/// Number of identity columns that come before the metric columns:
/// `path`, `space_name`, `space_kind`, `start_line`, `end_line`.
pub(crate) const IDENTITY_COLUMNS: usize = 5;

/// Number of metric columns produced by [`metric_values`]. The
/// per-section breakdown matches the comment dividers in the function
/// body.
pub(crate) const METRIC_COUNT: usize = 111;

/// Flatten a single FuncSpace's metrics into the flat column order
/// used by CSV / HTML output. The order is the public contract
/// exposed via `output::csv::CSV_HEADER`; callers iterate the returned
/// array in lockstep with `CSV_HEADER[IDENTITY_COLUMNS..]`.
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
        cog.cognitive_sum(),
        cog.cognitive_average(),
        cog.cognitive_min(),
        cog.cognitive_max(),
        // cyclomatic
        cyc.cyclomatic_sum(),
        cyc.cyclomatic_average(),
        cyc.cyclomatic_min(),
        cyc.cyclomatic_max(),
        cyc.cyclomatic_modified_sum(),
        cyc.cyclomatic_modified_average(),
        cyc.cyclomatic_modified_min(),
        cyc.cyclomatic_modified_max(),
        // halstead
        hal.u_operators(),
        hal.operators(),
        hal.u_operands(),
        hal.operands(),
        hal.length(),
        hal.estimated_program_length(),
        hal.purity_ratio(),
        hal.vocabulary(),
        hal.volume(),
        hal.difficulty(),
        hal.level(),
        hal.effort(),
        hal.time(),
        hal.bugs(),
        // loc
        l.sloc(),
        l.ploc(),
        l.lloc(),
        l.cloc(),
        l.blank(),
        l.sloc_average(),
        l.ploc_average(),
        l.lloc_average(),
        l.cloc_average(),
        l.blank_average(),
        l.sloc_min(),
        l.sloc_max(),
        l.cloc_min(),
        l.cloc_max(),
        l.ploc_min(),
        l.ploc_max(),
        l.lloc_min(),
        l.lloc_max(),
        l.blank_min(),
        l.blank_max(),
        // nom
        nm.functions_sum(),
        nm.closures_sum(),
        nm.functions_average(),
        nm.closures_average(),
        nm.total(),
        nm.average(),
        nm.functions_min(),
        nm.functions_max(),
        nm.closures_min(),
        nm.closures_max(),
        // nargs
        nrg.fn_args_sum(),
        nrg.closure_args_sum(),
        nrg.fn_args_average(),
        nrg.closure_args_average(),
        nrg.nargs_total(),
        nrg.nargs_average(),
        nrg.fn_args_min(),
        nrg.fn_args_max(),
        nrg.closure_args_min(),
        nrg.closure_args_max(),
        // nexits
        nex.exit_sum(),
        nex.exit_average(),
        nex.exit_min(),
        nex.exit_max(),
        // tokens
        tok.tokens_sum(),
        tok.tokens_average(),
        tok.tokens_min(),
        tok.tokens_max(),
        // abc
        a.assignments_sum(),
        a.branches_sum(),
        a.conditions_sum(),
        a.magnitude_sum(),
        a.assignments_average(),
        a.branches_average(),
        a.conditions_average(),
        a.assignments_min(),
        a.assignments_max(),
        a.branches_min(),
        a.branches_max(),
        a.conditions_min(),
        a.conditions_max(),
        // wmc
        w.class_wmc_sum(),
        w.interface_wmc_sum(),
        w.total_wmc(),
        // npm
        pm.class_npm_sum(),
        pm.interface_npm_sum(),
        pm.class_nm_sum(),
        pm.interface_nm_sum(),
        pm.class_coa(),
        pm.interface_coa(),
        pm.total_npm(),
        pm.total_nm(),
        pm.total_coa(),
        // npa
        pa.class_npa_sum(),
        pa.interface_npa_sum(),
        pa.class_na_sum(),
        pa.interface_na_sum(),
        pa.class_cda(),
        pa.interface_cda(),
        pa.total_npa(),
        pa.total_na(),
        pa.total_cda(),
        // mi
        mi_.mi_original(),
        mi_.mi_sei(),
        mi_.mi_visual_studio(),
    ]
}
