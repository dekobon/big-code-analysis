/// outer
#[test]
// between attributes
#[allow(dead_code)]
fn removed() { work()?; }
// plain prefix
#[test]
fn plain() { work()?; }
//! inner control
#[test]
fn inner() { work()?; }
/// kept
fn kept() { work()?; }
