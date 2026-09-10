//! Drift guards on the crate-level rustdoc in `src/lib.rs`.

use crate::LANG;

// Drift guard for the crate-level `## Supported Languages` rustdoc
// list in `src/lib.rs` (#769): every LANG variant's canonical slug
// — the single source of truth from `name()` — must appear in that
// list as a backtick-delimited token. Without this guard, adding a
// language (or renaming a slug) silently desyncs the docs.rs
// landing page, which is exactly how Objective-C went missing for a
// full release after #724 shipped it.
//
// The slug set is derived from `LANG::into_enum_iter()`, which is
// compiled unconditionally (the enum surface is feature-independent;
// only the grammar crates are gated), so this test is robust under
// `--no-default-features` and any per-language feature subset — no
// `all-languages` gate needed. Distinct slugs are deduplicated, so
// shared-slug families do not require one bullet per variant.
#[test]
fn supported_languages_rustdoc_lists_every_slug() {
    // Bound the search to the `## Supported Languages` section so an
    // incidental backtick match elsewhere in the module docs (e.g.
    // a slug named in the metrics section) cannot mask a real
    // omission from the list itself.
    const SECTION_HEADER: &str = "## Supported Languages";
    const NEXT_HEADER: &str = "## Supported Metrics";

    let lib_rs = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("src/lib.rs is readable from CARGO_MANIFEST_DIR");

    let section_start = lib_rs
        .find(SECTION_HEADER)
        .expect("rustdoc must contain a `## Supported Languages` section");
    let section_end = lib_rs[section_start..]
        .find(NEXT_HEADER)
        .map(|offset| section_start + offset)
        .expect("`## Supported Languages` must be followed by `## Supported Metrics`");
    let section = &lib_rs[section_start..section_end];

    for lang in LANG::into_enum_iter() {
        let slug_token = format!("`{}`", lang.name());
        assert!(
            section.contains(&slug_token),
            "LANG::{lang:?} slug {slug_token} is missing from the \
             `## Supported Languages` rustdoc list in src/lib.rs — \
             add an entry there (see #769)",
        );
    }
}
