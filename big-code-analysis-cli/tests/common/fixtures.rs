//! Source fixtures shared across the CLI integration suite, written
//! once per machine rather than once per test (#1126).
//!
//! Three modules each carried their own byte-identical copy of
//! `TRIVIAL_RUST`, and two carried `BRANCHY_RUST`, then wrote them into
//! a fresh `TempDir` in every test. The bytes never vary — what varies
//! per test is the manifest, the flags, and the working directory — so
//! they live here instead.
//!
//! **The directory is deliberately *not* a `LazyLock<TempDir>`.** A
//! `TempDir` in a `static` is never dropped, so that shape leaks one
//! directory per test process — and under `cargo nextest`, which runs a
//! process per test, it also shares nothing: each process would build
//! its own. A deterministic path under [`std::env::temp_dir`] shares
//! across processes *and* across runs, and leaves one directory behind
//! instead of one per test. The name carries a hash of the bytes, so
//! editing a fixture below yields a new directory rather than a stale
//! hit, and each file is published by rename so two test processes
//! racing to create it cannot expose a half-written fixture.
//!
//! **The directory is read-only to callers.** It sits under the system
//! temp dir, so it has no `.git` and no `bca.toml` ancestor and
//! [`cli_shared`] is exactly as hermetic as `common::cli_in` on a
//! per-test dir (#491). What it does *not* have is emptiness: it holds
//! both fixtures. Use it only for a command seeded with an explicit
//! `--paths`, never for one that would walk its working directory, and
//! never write into it — a test that needs to create a manifest, a
//! baseline, or a second source file still builds its own `TempDir`.
//!
//! `manifest.rs` deliberately keeps a branchy fixture of its own: it is
//! a *four*-branch function, not this five-branch one, and the values
//! that module asserts depend on the difference.

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use assert_cmd::Command;

/// Rust function with cyclomatic complexity > 1: each branch contributes
/// to the count. Five branches → cyclomatic == 5. Used by tests that
/// need a guaranteed violation when `cyclomatic` is given a tight limit.
#[allow(dead_code)]
pub const BRANCHY_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else if n < 10 {
        "small"
    } else if n < 100 {
        "medium"
    } else {
        "large"
    }
}
"#;

/// Rust function with cyclomatic == 1 (no branches). Threshold-clean for
/// any reasonable cyclomatic limit.
#[allow(dead_code)]
pub const TRIVIAL_RUST: &str = "
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
";

/// The shared directory, plus its fixture paths as `String` so call
/// sites can drop them straight into `Command::args`.
struct Shared {
    dir: PathBuf,
    branchy: String,
    trivial: String,
}

/// Publish `body` at `dir/name`, tolerating a concurrent publisher.
///
/// The write goes to a pid-suffixed sibling and is renamed into place,
/// which is atomic on every platform this suite runs on. Two processes
/// racing therefore either see no file or the whole file — never a
/// prefix — and the loser's rename simply replaces identical bytes.
fn publish(dir: &Path, name: &str, body: &str) -> String {
    let final_path = dir.join(name);
    let staging = dir.join(format!("{name}.{}.tmp", std::process::id()));
    fs::write(&staging, body).expect("write staged fixture");
    fs::rename(&staging, &final_path).expect("publish fixture");
    final_path.to_str().expect("utf8 fixture path").to_owned()
}

static SHARED: LazyLock<Shared> = LazyLock::new(|| {
    let mut hasher = DefaultHasher::new();
    BRANCHY_RUST.hash(&mut hasher);
    TRIVIAL_RUST.hash(&mut hasher);
    let dir = std::env::temp_dir().join(format!("bca-cli-fixtures-{:016x}", hasher.finish()));
    fs::create_dir_all(&dir).expect("create shared fixture dir");

    let branchy = publish(&dir, "branchy.rs", BRANCHY_RUST);
    let trivial = publish(&dir, "trivial.rs", TRIVIAL_RUST);
    Shared {
        dir,
        branchy,
        trivial,
    }
});

/// Absolute path to the shared five-branch fixture, named `branchy.rs`
/// so offender assertions can match on the basename.
#[allow(dead_code)]
pub fn branchy_rs() -> &'static str {
    &SHARED.branchy
}

/// Absolute path to the shared branchless fixture, named `trivial.rs`.
#[allow(dead_code)]
pub fn trivial_rs() -> &'static str {
    &SHARED.trivial
}

/// The shared directory itself, for a test that needs to name the
/// hermetic working directory separately from the command.
#[allow(dead_code)]
pub fn shared_dir() -> &'static Path {
    &SHARED.dir
}

/// A `bca` command anchored at the shared directory. See the module doc
/// for when this is *not* the right builder.
#[allow(dead_code)]
pub fn cli_shared() -> Command {
    super::cli_in(shared_dir())
}
