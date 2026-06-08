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
fn at_prefixed_non_numeric_timestamp_is_rejected() {
    // The `@<unix>` fast-path must reject a non-numeric epoch with a
    // typed error rather than silently falling through to gix's parser.
    assert!(matches!(
        parse_timestamp("@notanumber"),
        Err(Error::InvalidTimestamp(_))
    ));
}

#[test]
fn current_unix_seconds_is_a_recent_wall_clock() {
    // 2020-01-01T00:00:00Z. Any sane build host's clock is well past
    // this, exercising the happy `duration_since(UNIX_EPOCH)` path.
    const JAN_1_2020: i64 = 1_577_836_800;
    assert!(current_unix_seconds() > JAN_1_2020);
}
