//! Integration tests for the Checkstyle 4.3 XML output format.
//!
//! Validates structural conformance with the upstream XSD
//! (`tests/fixtures/checkstyle-report-1.0.0.xsd`, vendored from
//! checkstyle/checkstyle@master) via a `quick-xml`-driven walker in
//! [`common::validators::assert_checkstyle_well_formed_and_structural`].
//! See `tests/fixtures/README.md` for why we mirror the XSD's intent
//! in test code rather than running a true XSD validator.

use big_code_analysis::{OffenderRecord, Severity, write_checkstyle};

mod common;
use common::fixtures::rec;
use common::validators::assert_checkstyle_well_formed_and_structural;

fn render(offenders: &[OffenderRecord]) -> String {
    let mut buf = Vec::new();
    write_checkstyle(offenders, &mut buf).expect("writing to Vec is infallible");
    String::from_utf8(buf).expect("output is UTF-8")
}

#[test]
fn checkstyle_empty_is_well_formed_self_closing_root() {
    let out = render(&[]);
    assert_checkstyle_well_formed_and_structural(&out);
    // Sanity-check that the empty document really is the self-closing
    // root form, not just any well-formed XML.
    assert!(
        out.contains("<checkstyle version=\"4.3\"/>"),
        "expected self-closing root in:\n{out}"
    );
}

#[test]
fn checkstyle_single_offender_has_required_error_attributes() {
    let out = render(&[rec("src/foo.rs", "cyclomatic", 17.0, 15.0)]);
    assert_checkstyle_well_formed_and_structural(&out);
}

#[test]
fn checkstyle_multi_file_grouping_preserves_structure() {
    let out = render(&[
        rec("src/zeta.rs", "cyclomatic", 20.0, 15.0),
        rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
        rec("src/alpha.rs", "halstead.volume", 1234.5, 1000.0),
    ]);
    assert_checkstyle_well_formed_and_structural(&out);
}

#[test]
fn checkstyle_severity_value_is_in_xsd_enum() {
    let warning = rec("a.rs", "cyclomatic", 17.0, 15.0);
    let mut error = rec("a.rs", "cyclomatic", 99.0, 15.0);
    error.severity = Severity::Error;
    let out = render(&[warning, error]);
    assert_checkstyle_well_formed_and_structural(&out);

    // Negative regression guard: if the writer ever emitted a non-XSD
    // severity (e.g., "warn"), the walker would panic. We exercise
    // that path by mutating the writer's output post-render.
    let mutated = out.replace("severity=\"warning\"", "severity=\"warn\"");
    let panicked =
        std::panic::catch_unwind(|| assert_checkstyle_well_formed_and_structural(&mutated));
    assert!(
        panicked.is_err(),
        "walker must reject severity=\"warn\" (not in XSD enum)"
    );
}

#[test]
fn checkstyle_column_must_be_positive_integer_when_present() {
    // Defense-in-depth: the writer doesn't currently clamp
    // `record.start_col`. If a future producer (#96 threshold engine)
    // emits start_col=Some(0), the resulting `column="0"` violates
    // the XSD's xs:positiveInteger constraint. The walker catches it.
    let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
    r.start_col = Some(0);
    let out = render(&[r]);
    let panicked = std::panic::catch_unwind(|| assert_checkstyle_well_formed_and_structural(&out));
    assert!(
        panicked.is_err(),
        "walker must reject column=\"0\" (xs:positiveInteger requires >0)"
    );

    // And the happy path with Some(1) must pass.
    let mut r2 = rec("a.rs", "cyclomatic", 17.0, 15.0);
    r2.start_col = Some(1);
    assert_checkstyle_well_formed_and_structural(&render(&[r2]));
}
