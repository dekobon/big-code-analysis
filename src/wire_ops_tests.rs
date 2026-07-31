//! Projection-parity tests for the borrowed mirror in
//! [`super::ops_view`].
//!
//! Wired in from `src/wire.rs` as `#[path = "wire_ops_tests.rs"] mod
//! ops_tests;`. These live in their own file for the reason `#1066`
//! records: `exclude_tests` prunes a test *node*, not the doc comments
//! and helper functions around it, so a prose-heavy test module inside
//! a production file spends the file's `loc.sloc` budget. The
//! `./**/*_tests.rs` rule in `.bcaignore` keeps this one out of the
//! self-scan entirely.

use super::ops;

/// Per-language `ops` fixtures with nested spaces and non-ASCII
/// operand text, used by the two projection-parity tests below.
///
/// Non-ASCII matters twice over: the vocabulary is sorted on raw
/// bytes and rendered with a lossy UTF-8 conversion, and the four
/// formats escape multi-byte text differently. A fixture that only
/// held ASCII would agree on both paths for the wrong reason.
fn ops_fixtures() -> Vec<(crate::LANG, &'static str)> {
    vec![
        #[cfg(feature = "rust")]
        (
            crate::LANG::Rust,
            "fn caffè(zeta: u32) -> u32 { let αlpha = |b: u32| b * 2; αlpha(zeta) }\n",
        ),
        #[cfg(feature = "python")]
        (
            crate::LANG::Python,
            "def café(zeta):\n    def ωmega(b):\n        return b * 2\n    return ωmega(zeta)\n",
        ),
        #[cfg(feature = "cpp")]
        (
            crate::LANG::Cpp,
            "int zeta(int q) { double μ = q + 1; return q - μ; }\n",
        ),
        #[cfg(feature = "javascript")]
        (
            crate::LANG::Javascript,
            "function caffè(z) { const ωf = (b) => b * 2; return ωf(z); }\n",
        ),
        #[cfg(feature = "java")]
        (
            crate::LANG::Java,
            "class Zêta { int q(int m) { long ωa = m + 1; return ωa > 2 ? m : 0; } }\n",
        ),
    ]
}

fn parse_ops(lang: crate::LANG, source: &str) -> ops::Ops {
    crate::Ast::parse(
        crate::Source::new(lang, source.as_bytes()).with_name(Some("fixture".to_owned())),
    )
    .expect("language feature enabled")
    .ops()
    .expect("ops walk must yield a top-level Ops")
}

/// The borrowed [`super::ops_view::OpsView`] and the owned [`Ops`]
/// projection must
/// emit the same document, byte for byte, in every output format.
///
/// The mirror exists only to skip the owned clone (#1110), so the two
/// field lists are a duplication this module otherwise forbids.
/// This is what keeps them from drifting: a field renamed, reordered,
/// retyped, or given a different `skip_serializing_if` on one side
/// fails here.
#[test]
// Gated on the language that guarantees a non-empty fixture list, so
// the emptiness assertion below cannot fire on a minimal build.
#[cfg(feature = "rust")]
fn borrowed_and_owned_ops_projections_serialize_alike() {
    let fixtures = ops_fixtures();
    assert!(
        !fixtures.is_empty(),
        "at least one language feature must be enabled for this test to mean anything"
    );
    for (lang, source) in fixtures {
        let ops = parse_ops(lang, source);
        let owned = ops.to_wire();
        assert!(
            !ops.spaces.is_empty() && ops.operands.iter().any(|o| !o.is_ascii()),
            "{lang:?} fixture must nest a space and carry non-ASCII operand text: {ops:?}"
        );

        assert_eq!(
            serde_json::to_string(&ops).expect("borrowed JSON"),
            serde_json::to_string(&owned).expect("owned JSON"),
            "{lang:?} JSON must not depend on the projection"
        );
        assert_eq!(
            serde_yaml::to_string(&ops).expect("borrowed YAML"),
            serde_yaml::to_string(&owned).expect("owned YAML"),
            "{lang:?} YAML must not depend on the projection"
        );
        assert_eq!(
            toml::to_string(&ops).expect("borrowed TOML"),
            toml::to_string(&owned).expect("owned TOML"),
            "{lang:?} TOML must not depend on the projection"
        );
        let (mut borrowed_cbor, mut owned_cbor) = (Vec::new(), Vec::new());
        ciborium::into_writer(&ops, &mut borrowed_cbor).expect("borrowed CBOR");
        ciborium::into_writer(&owned, &mut owned_cbor).expect("owned CBOR");
        assert_eq!(
            borrowed_cbor, owned_cbor,
            "{lang:?} CBOR must not depend on the projection"
        );
    }
}

/// The two projections agree on the fields the parsed fixtures cannot
/// reach: an absent name, and a lossy one.
///
/// `name_was_lossy` is `false` on every walk the current seams produce,
/// and it is the only field carrying a `skip_serializing_if`, so
/// dropping that attribute from one projection and not the other is
/// invisible above — the field is skipped either way. Setting it makes
/// the divergence observable.
#[test]
#[cfg(feature = "rust")]
fn borrowed_and_owned_ops_projections_agree_on_the_optional_fields() {
    let mut ops = parse_ops(crate::LANG::Rust, "fn f() { let a = 1 + 2; }\n");
    ops.name = None;
    ops.name_was_lossy = true;

    let borrowed = serde_json::to_string(&ops).expect("borrowed JSON");
    assert!(
        borrowed.contains("\"name\":null") && borrowed.contains("\"name_was_lossy\":true"),
        "both fields must be emitted for this test to compare anything: {borrowed}"
    );
    assert_eq!(
        borrowed,
        serde_json::to_string(&ops.to_wire()).expect("owned JSON"),
    );
}

/// Serializing an [`crate::Ops`] must not build an owned projection.
///
/// Both paths emit identical bytes — that is what the test above
/// asserts — so nothing about the *output* distinguishes them and a
/// revert to `serialize_via_wire!(ops::Ops => Ops)` would pass every
/// other test in this module. The projection counter is the only
/// observable, and the `to_wire` leg is what proves it is a counter
/// and not a constant zero.
///
/// The deep leg is the one #1110 was really about: the refused tree
/// was cloned in full and then dropped unserialized, which was the
/// entire cost of the refusal.
#[test]
#[cfg(feature = "rust")]
fn serializing_ops_builds_no_owned_projection() {
    // Scoped to this test: the rest of the module builds no projections
    // and compiles with no language feature enabled, where these three
    // would be unused imports.
    use super::tests::nested_functions;
    use super::{MAX_SPACE_SERIALIZE_DEPTH, owned_ops_projections_on_this_thread};

    let ops = parse_ops(crate::LANG::Rust, "fn f() { fn g() { let a = 1 + 2; } }\n");

    let before = owned_ops_projections_on_this_thread();
    serde_json::to_string(&ops).expect("shallow tree serializes");
    assert_eq!(
        owned_ops_projections_on_this_thread(),
        before,
        "serializing must go through the borrowed projection"
    );

    let _ = ops.to_wire();
    assert_eq!(
        owned_ops_projections_on_this_thread(),
        before + 1,
        "`to_wire` is the caller-facing owned projection and must still build one"
    );

    // One level past `MAX_SPACE_SERIALIZE_DEPTH`, so serialization is
    // refused rather than completed.
    let deep = parse_ops(
        crate::LANG::Rust,
        &nested_functions(MAX_SPACE_SERIALIZE_DEPTH + 1),
    );
    let before = owned_ops_projections_on_this_thread();
    serde_json::to_string(&deep).expect_err("nesting past the limit must be refused");
    assert_eq!(
        owned_ops_projections_on_this_thread(),
        before,
        "a refused serialization must not have cloned the tree it refused"
    );
}
