use super::*;
use big_code_analysis_ast::documentation::*;

#[test]
fn rust_unit1_full_rungs_validate_source_text_maps_and_identity() {
    let probes = &REGISTRY[46..];
    assert_eq!(probes.len(), 11);
    for probe in probes {
        for n in probe.sizes {
            let parsed = Parsed::new(probe, n).unwrap();
            let index = parsed.collect(probe).unwrap();
            let prepared = Prepared::new(probe, n, &parsed, &index)
                .unwrap_or_else(|error| panic!("{}/{n}: {error}", probe.id));
            let counts = prepared.counts();
            assert_eq!(counts.documents, 1);
            assert_eq!(counts.edges, 1);
            assert_eq!(counts.qualifications, 0);
        }
    }
}

#[test]
fn rust_origin_validator_rejects_wrong_token_with_unchanged_counts_and_text() {
    let probe = REGISTRY
        .iter()
        .find(|p| p.key == "rust-escapes-split")
        .unwrap();
    let parsed = Parsed::new(probe, 2).unwrap();
    let index = parsed.collect(probe).unwrap();
    let text = index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap();
    let expected = [
        (0..1, rust_validation::ExpectedOrigin::Decoded(9..13)),
        (1..2, rust_validation::ExpectedOrigin::Decoded(13..17)),
    ];
    rust_validation::mappings(text.mappings(), &expected, parsed.source(), true).unwrap();
    let mut corrupted = text.mappings().to_vec();
    corrupted[1].origin = corrupted[0].origin.clone();
    assert_eq!(corrupted.len(), 2);
    assert_eq!(text.text(), "AA");
    assert_eq!(
        rust_validation::mappings(&corrupted, &expected, parsed.source(), true)
            .unwrap_err()
            .0,
        "Rust responsible piece range/bytes"
    );
}

#[test]
fn rust_graph_validator_rejects_equal_source_with_foreign_canonical_links() {
    let probe = REGISTRY.iter().find(|p| p.key == "rust-mixed").unwrap();
    let parsed = Parsed::new(probe, 2).unwrap();
    let other = Parsed::new(probe, 2).unwrap();
    let index = other.collect(probe).unwrap();
    assert_eq!(parsed.source(), other.source());
    assert_eq!(index.documents().len(), 1);
    assert_eq!(index.associations().len(), 1);
    assert_eq!(index.facts().fragments().len(), 5);
    assert_eq!(
        rust_validation::validate(probe, 2, &parsed, &index)
            .unwrap_err()
            .0,
        "Rust binding/scope identity"
    );
}

#[test]
fn rust_split_map_consumes_each_borrowed_piece_in_order() {
    let probe = REGISTRY.last().unwrap();
    assert_eq!(probe.id, "documentation/rust-unit1/split-late-map");
    let parsed = Parsed::new(probe, 8).unwrap();
    let index = parsed.collect(probe).unwrap();
    let text = index
        .complete_text(index.documents().next().unwrap())
        .unwrap()
        .unwrap();
    let Origin::Decoded(expected) = &text.mappings()[7].origin else {
        panic!("decoded")
    };
    let mut consumed = Vec::new();
    operation::consume_map_pieces(text, |piece| consumed.push(piece)).unwrap();
    assert_eq!(consumed.len(), 4);
    for (at, (piece, original)) in consumed.iter().zip(expected.pieces()).enumerate() {
        assert!(std::ptr::eq(*piece, original));
        assert_eq!(piece.range(), 37 + at..38 + at);
        assert_eq!(piece.bytes(), &b"\\x41"[at..=at]);
    }
}

#[test]
fn rust_work_counters_separate_depth_width_and_piece_axes() {
    for n in [32, 64, 128] {
        for probe in &REGISTRY[46..54] {
            let parsed = Parsed::new(probe, n).unwrap();
            let before = observations();
            let index = parsed.collect(probe).unwrap();
            let after = observations();
            let (attributes, units, copies, projections, pieces, visits) = match probe.key {
                // expected: block copies three runs, attribute copies temporary/projected z,
                // two joins per unit and one final inner-comment copy. Only z is decoded/projected.
                "rust-mixed" => (n, n, 7 * n + 1, n, 0, 4 * n + 1),
                // expected: decode LF + n*(a, CR, CR, LF) + b; temporary 4n+2 appends,
                // final 2n+1 appends. Projection crosses 4n+2 maps plus 2n lookaheads.
                // Only the n retained LF tokens copy responsible pieces.
                "rust-cooked" => (1, 4 * n + 2, 6 * n + 3, 6 * n + 2, n, 4 * n + 2),
                // expected: raw unit has five ordinary bytes and one physical CRLF token;
                // both passes append 2n+1 runs, projection adds 2n lookaheads.
                "rust-raw" => (1, 6 * n + 1, 4 * n + 2, 4 * n + 1, n, 2 * n + 1),
                "rust-attribute-width" | "rust-attribute-depth" | "rust-attribute-depth-width" => {
                    let w = if probe.key == "rust-attribute-depth" {
                        1
                    } else {
                        n
                    };
                    // expected: one decode and projection per a; two copies and w-1 joins.
                    (w, w, 3 * w - 1, w, 0, w)
                }
                "rust-escapes-contiguous" | "rust-escapes-split" => {
                    let split = probe.key == "rust-escapes-split";
                    // expected: n token appends plus one projected line; split cursor visits
                    // nine prefix pieces and four responsible bytes plus lookahead per token.
                    (
                        1,
                        n,
                        n + 1,
                        n,
                        if split { 4 * n } else { n },
                        if split { 9 + 5 * n } else { n },
                    )
                }
                _ => unreachable!(),
            };
            assert_eq!(
                after.carrier_children - before.carrier_children,
                3 * attributes,
                "{}",
                probe.id
            );
            assert_eq!(
                after.attribute_children - before.attribute_children,
                3 * attributes,
                "{}",
                probe.id
            );
            assert_eq!(
                after.decoded_units - before.decoded_units,
                units,
                "{}",
                probe.id
            );
            assert_eq!(
                after.copy_chunks - before.copy_chunks,
                copies,
                "{}",
                probe.id
            );
            assert_eq!(
                after.projection_visits - before.projection_visits,
                projections,
                "{}",
                probe.id
            );
            assert_eq!(
                after.piece_copies - before.piece_copies,
                pieces,
                "{}",
                probe.id
            );
            assert_eq!(
                after.fragment_piece_visits - before.fragment_piece_visits,
                visits,
                "{}",
                probe.id
            );
            assert_eq!(after.renders - before.renders, 1, "{}", probe.id);
            assert_eq!(index.documents().len(), 1);
        }
    }
}

#[test]
fn rust_registry_has_exact_operations_classes_and_ladders() {
    use fixtures::Operation::*;
    let expected = [
        ("mixed-fragments", Collect, [256, 512, 1024]),
        ("cooked-large", Collect, [4096, 8192, 16384]),
        ("raw-large", Collect, [4096, 8192, 16384]),
        ("attribute-width", Collect, [256, 512, 1024]),
        ("attribute-depth", Collect, [256, 512, 1024]),
        ("attribute-depth-width", Collect, [256, 512, 1024]),
        ("escapes-contiguous", Collect, [1024, 2048, 4096]),
        ("escapes-split", Collect, [1024, 2048, 4096]),
        ("mixed-retained", Retained, [256, 512, 1024]),
        ("mixed-owner", Owner, [256, 512, 1024]),
        ("split-late-map", MapPieces, [1024, 2048, 4096]),
    ];
    assert_eq!(REGISTRY.len(), 57);
    for (probe, (id, operation, sizes)) in REGISTRY[46..].iter().zip(expected) {
        assert_eq!(probe.id, format!("documentation/rust-unit1/{id}"));
        assert_eq!(probe.operation, operation);
        assert_eq!(probe.sizes, sizes);
        assert_eq!(probe.bound, 1.50);
        assert_eq!(
            probe.class,
            match operation {
                Collect => StatisticClass::Exponent,
                MapPieces => StatisticClass::LogMapRatio,
                _ => StatisticClass::FixedOutputRatio,
            }
        );
    }
    for (class, expected) in [
        (StatisticClass::Exponent, 47),
        (StatisticClass::FixedOutputRatio, 8),
        (StatisticClass::LogMapRatio, 2),
    ] {
        assert_eq!(
            REGISTRY.iter().filter(|p| p.class == class).count(),
            expected
        );
    }
}

#[test]
fn infrastructure_registry_preserves_every_original_cell_and_policy() {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut manifest = String::new();
    for p in &REGISTRY[..46] {
        for n in p.sizes {
            let source = p.source(n).unwrap();
            let options = p.options();
            writeln!(
                manifest,
                "{}\t{}\t{:?}\t{:?}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{}\t{:?}",
                p.id,
                n,
                p.language,
                p.class,
                p.operation,
                p.bound,
                options.retain_unbound,
                options.retain_directives,
                p.dimensions,
                p.timing,
                p.key,
                p.policy,
                options.profile,
                p.evidence,
                source
            )
            .unwrap();
        }
    }
    // Frozen by executing fixtures.rs from infrastructure I=3924d45adf9716ec5d64381849b671b298b8b5fe.
    // Includes all 138 source buffers and each original operation/options/policy field in order.
    assert_eq!(
        Sha256::digest(manifest.as_bytes())
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").unwrap();
                hex
            }),
        "38a4c530ebb06529770d4ef42aba41eee97dfe4251db1fddebe22d6331b15a8d"
    );
}
