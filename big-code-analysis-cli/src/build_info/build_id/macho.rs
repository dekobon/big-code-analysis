//! The `LC_UUID` load command of a Mach-O executable (#1777).
//!
//! `ld64` and `lld` stamp a 16-byte UUID into every linked image; it is
//! what `dwarfdump --uuid` prints and what Xcode symbolication keys on.
//! Support is a thin little-endian 64-bit image — the one shape the
//! `aarch64-apple-darwin` release carries. A 32-bit image, a
//! byte-swapped one and a universal (FAT) container all read as absent.

#[cfg(target_os = "macos")]
pub(super) fn current() -> Option<String> {
    let head = super::read_head(&std::env::current_exe().ok()?)?;
    reader::build_id(&head).map(super::hex_lower)
}

#[cfg(test)]
pub(super) use reader::build_id;

/// The parser itself, compiled only where it has a caller (see `elf`).
#[cfg(any(target_os = "macos", test))]
mod reader {
    use super::super::u32_at;

    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const HEADER_LEN: usize = 32;
    const LC_UUID: u32 = 0x1b;
    const LC_UUID_LEN: usize = 24;

    /// The 16 UUID bytes of the `LC_UUID` command in `macho`, or `None`
    /// when the bytes are not a thin little-endian 64-bit image carrying
    /// one.
    pub(crate) fn build_id(macho: &[u8]) -> Option<&[u8]> {
        // bca: suppress(nexits) — every exit is a `?` on a bounds-checked
        // read or a checked add; the load-command walk fails closed and
        // reads best as one straight line of them.
        if u32_at(macho, 0)? != MH_MAGIC_64 {
            return None;
        }
        let ncmds = u32_at(macho, 16)?;
        let sizeofcmds = usize::try_from(u32_at(macho, 20)?).ok()?;
        let end = HEADER_LEN.checked_add(sizeofcmds)?;
        let mut at = HEADER_LEN;
        for _ in 0..ncmds {
            let cmd = u32_at(macho, at)?;
            // `u32_at(macho, at)` succeeded, so `at + 4 <= len`, and once
            // this read succeeds too `at + 8 <= len`: neither plain
            // addition can overflow.
            let cmdsize = usize::try_from(u32_at(macho, at + 4)?).ok()?;
            let next = at.checked_add(cmdsize)?;
            if next > end {
                return None;
            }
            // A `cmdsize` under 8 makes this range inverted, so the read
            // fails and the walk cannot re-read the same command for every
            // one of `ncmds`: each turn advances by at least the 8-byte
            // header or stops.
            let body = macho.get(at + 8..next)?;
            if cmd == LC_UUID {
                return (cmdsize == LC_UUID_LEN).then_some(body);
            }
            at = next;
        }
        None
    }
}
