use super::*;

/// A distinct-but-valid object id per `byte` so timeline entries are
/// identifiable without a real repository.
fn oid(byte: u8) -> gix::ObjectId {
    gix::ObjectId::from_bytes_or_panic(&[byte; 20])
}

#[test]
fn tip_at_or_before_picks_the_most_recent_in_range() {
    // Newest-first timeline (as produced by the first-parent walk).
    let timeline = vec![(300, oid(3)), (200, oid(2)), (100, oid(1))];
    // Exactly on a boundary selects that commit.
    assert_eq!(tip_at_or_before(&timeline, 200), Some(oid(2)));
    // Between boundaries selects the latest at-or-before.
    assert_eq!(tip_at_or_before(&timeline, 250), Some(oid(2)));
    // At/after the tip selects the tip.
    assert_eq!(tip_at_or_before(&timeline, 999), Some(oid(3)));
}

#[test]
fn tip_at_or_before_returns_none_before_first_commit() {
    let timeline = vec![(300, oid(3)), (100, oid(1))];
    // Before the repository's first commit: no tip yet.
    assert_eq!(tip_at_or_before(&timeline, 50), None);
    assert_eq!(tip_at_or_before(&[], 100), None);
}

#[test]
fn tip_at_or_before_is_robust_to_out_of_order_times() {
    // Clock skew / history rewriting can leave times non-monotonic. The
    // scan must still find the greatest time <= the cutoff (oid(9) @ 250),
    // not the first entry it walks past.
    let timeline = vec![(150, oid(1)), (250, oid(9)), (90, oid(2))];
    assert_eq!(tip_at_or_before(&timeline, 300), Some(oid(9)));
    assert_eq!(tip_at_or_before(&timeline, 200), Some(oid(1)));
}

#[test]
fn snapshot_options_anchors_reference_and_as_of() {
    let base = Options {
        reference: "HEAD".to_owned(),
        compute_bus_factor: true,
        long_window_secs: 999,
        ..Options::default()
    };
    let opts = snapshot_options(&base, oid(7), 4_242);
    assert_eq!(opts.reference, oid(7).to_hex().to_string());
    assert_eq!(opts.as_of, Some(4_242));
    // Per-point snapshots never carry the bus-factor aggregate...
    assert!(!opts.compute_bus_factor);
    // ...but every other knob is inherited from the base options.
    assert_eq!(opts.long_window_secs, 999);
}
