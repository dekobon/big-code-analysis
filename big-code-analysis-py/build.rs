//! Build script: bake the libpython rpath into binaries that embed
//! Python (i.e. `cargo test` with `pyo3/auto-initialize`), so the
//! test harness can find `libpython3.x.so` at runtime without the
//! caller having to set `LD_LIBRARY_PATH`.
//!
//! Mise / pyenv / Homebrew Python installs live outside the system
//! loader's default search path, so the rpath is the difference
//! between `cargo test --workspace` passing in a fresh shell vs.
//! only inside a mise-activated one. The wheel build path
//! (`extension-module`) does not link libpython, and
//! `add_libpython_rpath_link_args` is a no-op in that case, so the
//! call is safe unconditionally.

fn main() {
    pyo3_build_config::add_libpython_rpath_link_args();
}
