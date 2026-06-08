use super::*;

#[test]
fn walk_err_wraps_into_walk_variant() {
    let err = walk_err("object missing");
    assert!(matches!(err, Error::Walk(msg) if msg == "object missing"));
}

#[test]
fn diff_err_wraps_into_diff_variant() {
    let err = diff_err("blob decode failed");
    assert!(matches!(err, Error::Diff(msg) if msg == "blob decode failed"));
}

#[test]
fn at_prefixed_epoch_is_parsed_by_the_fast_path() {
    // gix's date parser does not accept the bare `@<unix>` spelling, so a
    // correct parse here proves our fast-path ran — not the gix fallback.
    // This is what makes the negative test below load-bearing: if the
    // fast-path were deleted, this assertion would fail.
    assert_eq!(
        parse_timestamp("@1577836800").expect("epoch"),
        1_577_836_800
    );
}

#[test]
fn at_prefixed_non_numeric_timestamp_is_rejected() {
    // Pin the fast-path's *own* error message. A bare wildcard match
    // would also pass via the gix fallback (which rejects `@notanumber`
    // as InvalidTimestamp too), so it would not guard the fast-path.
    assert!(matches!(
        parse_timestamp("@notanumber"),
        Err(Error::InvalidTimestamp(msg)) if msg.contains("not a Unix timestamp")
    ));
}

#[test]
fn current_unix_seconds_is_a_recent_wall_clock() {
    // 2020-01-01T00:00:00Z. Any sane build host's clock is well past
    // this, exercising the happy `duration_since(UNIX_EPOCH)` path.
    const JAN_1_2020: i64 = 1_577_836_800;
    assert!(current_unix_seconds() > JAN_1_2020);
}
