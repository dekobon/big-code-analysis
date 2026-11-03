//! The build id of the running executable (#1773, #1777).
//!
//! The linker writes an identifier into the final image that debuggers
//! and symbol servers key on: the GNU build-id note on ELF, the
//! `LC_UUID` load command on Mach-O, and the `CodeView` record that pairs
//! a PE with its `.pdb`. It is the one provenance field `build.rs`
//! cannot bake in, because it does not exist until the link has
//! finished, so it is read back from the binary at run time.
//!
//! Support is deliberately narrow — one reader per platform, for the
//! executable shape that platform's toolchain produces — and anything
//! else (ELF32, a universal Mach-O, a PE without `CodeView`, a truncated
//! or hand-edited file, an executable deleted since it started) yields
//! `None` and renders as `unavailable`. Nothing is decided on this
//! field, so every failure is informational.
//!
//! All offsets come from the file and are bounds-checked against the
//! bytes actually read; arithmetic is `checked_*` throughout, so a
//! malformed header cannot panic.

#[cfg(any(target_os = "linux", test))]
mod elf;
#[cfg(any(target_os = "macos", test))]
mod macho;
#[cfg(any(windows, test))]
mod pe;

#[cfg(test)]
#[path = "build_id_tests.rs"]
mod tests;

/// Lowercase hex of the running executable's build id, or `None`.
#[cfg(target_os = "linux")]
pub(crate) fn current_build_id() -> Option<String> {
    elf::current()
}

/// Lowercase hex of the running executable's build id, or `None`.
#[cfg(target_os = "macos")]
pub(crate) fn current_build_id() -> Option<String> {
    macho::current()
}

/// Lowercase hex of the running executable's build id, or `None`.
#[cfg(windows)]
pub(crate) fn current_build_id() -> Option<String> {
    pe::current()
}

/// Always `None`: no reader exists for this platform's executable
/// format, so the field renders as `unavailable`.
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn current_build_id() -> Option<String> {
    None
}

/// How much of the executable to read looking for the id. Program
/// headers, load commands and the PE headers all sit in the first page
/// or so of a linked image; 64 KiB covers that with a wide margin and
/// keeps this off the path of reading a 140 MB binary into memory to
/// answer `--version`. Headers past the bound read as absent, the
/// documented answer for every shape the readers do not handle; the
/// PE reader fetches the directory and record its headers name with
/// bounded second reads through [`read_at`], since those routinely sit
/// past it.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const HEAD_BYTES: usize = 64 * 1024;

/// The first [`HEAD_BYTES`] of `path`.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn read_head(path: &std::path::Path) -> Option<Vec<u8>> {
    read_at(path, 0, HEAD_BYTES)
}

/// Up to `len` bytes of `path` from `offset`, short at end of file.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn read_at(path: &std::path::Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut out = Vec::with_capacity(len);
    file.take(u64::try_from(len).ok()?)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

/// Lowercase hex, two digits per byte. `digest.rs` carries its own copy
/// because `build.rs` includes that file by `#[path]` and can import
/// nothing from the crate.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}

/// `N` little-endian bytes at `at`, or `None` past the end or on an
/// overflowing offset. One reader behind every fixed-width field the
/// three parsers decode.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn le_at<const N: usize>(bytes: &[u8], at: usize) -> Option<&[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.as_array()
}

#[cfg(any(target_os = "linux", windows, test))]
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    le_at(bytes, at).copied().map(u16::from_le_bytes)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    le_at(bytes, at).copied().map(u32::from_le_bytes)
}

#[cfg(any(target_os = "linux", test))]
fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    le_at(bytes, at).copied().map(u64::from_le_bytes)
}
