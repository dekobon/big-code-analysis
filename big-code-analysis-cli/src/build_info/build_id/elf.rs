//! The GNU build-id note of an ELF executable (#1773).
//!
//! The linker writes a hash of the final object into a `PT_NOTE`
//! segment as `NT_GNU_BUILD_ID`; debuggers, `perf` and symbol servers
//! key on it. Support is ELF64 in little-endian byte order, which is
//! every Linux target the gate runs on; ELF32 and big-endian images
//! read as absent.

#[cfg(target_os = "linux")]
pub(super) fn current() -> Option<String> {
    let head = super::read_head(&std::env::current_exe().ok()?)?;
    reader::build_id(&head).map(super::hex_lower)
}

#[cfg(test)]
pub(super) use reader::build_id;

/// The parser itself. Compiled only where it has a caller — Linux, and
/// the test build on every platform, where the synthetic fixtures
/// exercise it — so a macOS or Windows release build under
/// `-D warnings` carries no dead code.
///
/// The `test` alternative also takes this module out of the self-scan:
/// `bca check` under `exclude_tests` prunes a `cfg(any(…, test))`
/// subtree as test code, so the `bca: suppress(nexits)` markers here
/// and in the sibling readers document the intent for an unpruned run
/// (`bca check --no-config --threshold nexits=5 -p <file>`) rather than
/// hold the gate.
#[cfg(any(target_os = "linux", test))]
mod reader {
    use super::super::{u16_at, u32_at, u64_at};

    const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
    const ELFCLASS64: u8 = 2;
    const ELFDATA2LSB: u8 = 1;
    const PT_NOTE: u32 = 4;
    const NT_GNU_BUILD_ID: u32 = 3;
    const GNU_NOTE_NAME: &[u8] = b"GNU\0";
    const PHENTSIZE: usize = 0x38;

    /// The raw `NT_GNU_BUILD_ID` descriptor in `elf`, or `None` when the
    /// bytes are not a little-endian ELF64 image carrying one.
    pub(crate) fn build_id(elf: &[u8]) -> Option<&[u8]> {
        let header = Header::parse(elf)?;
        (0..header.phnum)
            .filter_map(|i| header.program_header(elf, i))
            .filter(|ph| ph.kind == PT_NOTE)
            .find_map(|ph| find_gnu_build_id(elf.get(ph.offset..ph.offset.checked_add(ph.size)?)?))
    }

    struct Header {
        phoff: usize,
        phentsize: usize,
        phnum: usize,
    }

    struct ProgramHeader {
        kind: u32,
        offset: usize,
        size: usize,
    }

    impl Header {
        fn parse(elf: &[u8]) -> Option<Self> {
            // bca: suppress(nexits) — every exit is a `?` on a bounds-checked
            // read; a binary-format reader that fails closed is clearest as
            // one straight line of them, not as nested `if let`s.
            if !elf.starts_with(ELF_MAGIC)
                || *elf.get(4)? != ELFCLASS64
                || *elf.get(5)? != ELFDATA2LSB
            {
                return None;
            }
            // ELF64 fixes `e_phentsize` at 56. A smaller value would make
            // every index alias the same header, so one `PT_NOTE` could be
            // walked `phnum` times — 272 ms measured for `phentsize = 0`
            // against 31 µs — and `readelf` rejects it too.
            let phentsize = usize::from(u16_at(elf, 0x36)?);
            if phentsize < PHENTSIZE {
                return None;
            }
            Some(Self {
                phoff: usize::try_from(u64_at(elf, 0x20)?).ok()?,
                phentsize,
                phnum: usize::from(u16_at(elf, 0x38)?),
            })
        }

        fn program_header(&self, elf: &[u8], index: usize) -> Option<ProgramHeader> {
            // bca: suppress(nexits) — same shape as `parse`: `?` on checked
            // arithmetic and bounded reads.
            let at = self.phoff.checked_add(index.checked_mul(self.phentsize)?)?;
            Some(ProgramHeader {
                kind: u32_at(elf, at)?,
                offset: usize::try_from(u64_at(elf, at.checked_add(0x08)?)?).ok()?,
                size: usize::try_from(u64_at(elf, at.checked_add(0x20)?)?).ok()?,
            })
        }
    }

    /// Walk the notes in one `PT_NOTE` segment. Each is a 12-byte header
    /// (`namesz`, `descsz`, `type`) followed by the name and the
    /// descriptor, each padded to four bytes.
    fn find_gnu_build_id(segment: &[u8]) -> Option<&[u8]> {
        // bca: suppress(nexits) — the note walk fails closed through `?` on
        // every read and every `checked_add`; that is the whole safety
        // argument of the module, and it reads best uninterrupted.
        let mut at = 0usize;
        while let Some(namesz) = u32_at(segment, at) {
            // `u32_at(segment, at)` succeeded, so `at + 4 <= len`, and a
            // slice is at most `isize::MAX` bytes: neither plain addition
            // can overflow.
            let descsz = usize::try_from(u32_at(segment, at + 4)?).ok()?;
            let kind = u32_at(segment, at + 8)?;
            let namesz = usize::try_from(namesz).ok()?;
            let name_at = at.checked_add(12)?;
            let desc_at = name_at.checked_add(namesz.checked_next_multiple_of(4)?)?;
            let next = desc_at.checked_add(descsz.checked_next_multiple_of(4)?)?;
            if kind == NT_GNU_BUILD_ID
                && segment.get(name_at..name_at.checked_add(namesz)?)? == GNU_NOTE_NAME
            {
                return segment.get(desc_at..desc_at.checked_add(descsz)?);
            }
            at = next;
        }
        None
    }
}
