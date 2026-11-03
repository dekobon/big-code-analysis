//! The `CodeView` record of a PE executable (#1777).
//!
//! `link.exe` and `lld-link` write an `IMAGE_DEBUG_DIRECTORY` whose
//! `CODEVIEW` entry points at an `RSDS` record — a 16-byte GUID and an
//! age — that pairs the image with its `.pdb`. Both PE32 and PE32+ are
//! read. The debug directory and the record routinely sit past the
//! first 64 KiB, so this reader is two-stage: the headers come from
//! the head, and the directory and record are each fetched with one
//! bounded read at the file offset the headers name. A PE whose
//! linker emitted no `CodeView` entry reads as absent.

#[cfg(windows)]
pub(super) fn current() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let head = super::read_head(&exe)?;
    reader::build_id(&head, |offset, len| super::read_at(&exe, offset, len))
        .map(|id| super::hex_lower(&id))
}

#[cfg(test)]
pub(super) use reader::build_id;

/// The parser itself, compiled only where it has a caller (see `elf`).
#[cfg(any(windows, test))]
mod reader {
    use super::super::{u16_at, u32_at};

    const PE_SIGNATURE: &[u8; 4] = b"PE\0\0";
    const PE32: u16 = 0x10b;
    const PE32_PLUS: u16 = 0x20b;
    const IMAGE_DIRECTORY_ENTRY_DEBUG: usize = 6;
    const SECTION_LEN: usize = 40;
    const DEBUG_ENTRY_LEN: usize = 28;
    /// More debug entries than any linker writes; bounds the second-stage read.
    const MAX_DEBUG_ENTRIES: usize = 64;
    const IMAGE_DEBUG_TYPE_CODEVIEW: u32 = 2;
    const RSDS: &[u8; 4] = b"RSDS";
    /// `RSDS` + GUID + age.
    const CODEVIEW_LEN: usize = 24;

    /// The 20 bytes after `RSDS` — the GUID as stored, then the age — of
    /// the first `CodeView` entry in `head`'s debug directory, or `None`.
    /// `read_at(offset, len)` fetches bytes of the image past `head`;
    /// it returns what it can, and a short read fails the parse.
    pub(crate) fn build_id(
        head: &[u8],
        read_at: impl Fn(u64, usize) -> Option<Vec<u8>>,
    ) -> Option<Vec<u8>> {
        let headers = Headers::parse(head)?;
        let (rva, size) = headers.debug_directory(head)?;
        let at = headers.file_offset(head, rva, size)?;
        let directory = read_exact(&read_at, at, size.min(DEBUG_ENTRY_LEN * MAX_DEBUG_ENTRIES))?;
        directory
            .as_chunks::<DEBUG_ENTRY_LEN>()
            .0
            .iter()
            .filter(|entry| u32_at(*entry, 12) == Some(IMAGE_DEBUG_TYPE_CODEVIEW))
            .find_map(|entry| codeview_id(&read_at, entry))
    }

    /// The identity of one `CODEVIEW` entry, or `None` for a short one
    /// or a record that is not `RSDS`; the caller moves on to the next.
    fn codeview_id(
        read_at: &impl Fn(u64, usize) -> Option<Vec<u8>>,
        entry: &[u8],
    ) -> Option<Vec<u8>> {
        let size = u32_at(entry, 16)?;
        if usize::try_from(size).ok()? < CODEVIEW_LEN {
            return None;
        }
        let record = read_exact(read_at, u64::from(u32_at(entry, 24)?), CODEVIEW_LEN)?;
        record.strip_prefix(RSDS).map(<[u8]>::to_vec)
    }

    fn read_exact(
        read_at: &impl Fn(u64, usize) -> Option<Vec<u8>>,
        at: u64,
        len: usize,
    ) -> Option<Vec<u8>> {
        let bytes = read_at(at, len)?;
        (bytes.len() == len).then_some(bytes)
    }

    struct Headers {
        /// Where the section table starts: the end of the declared
        /// optional header.
        sections: usize,
        section_count: usize,
        directories_at: usize,
        directory_count: usize,
    }

    impl Headers {
        fn parse(head: &[u8]) -> Option<Self> {
            // bca: suppress(nexits) — every exit is a `?` on a bounds-checked
            // read; a binary-format reader that fails closed is clearest as
            // one straight line of them (see `elf`).
            if !head.starts_with(b"MZ") {
                return None;
            }
            let pe = usize::try_from(u32_at(head, 0x3c)?).ok()?;
            if !head.get(pe..)?.starts_with(PE_SIGNATURE) {
                return None;
            }
            let coff = pe.checked_add(4)?;
            let section_count = usize::from(u16_at(head, coff.checked_add(2)?)?);
            let optional_len = usize::from(u16_at(head, coff.checked_add(16)?)?);
            let optional = coff.checked_add(20)?;
            // The data-directory count and table sit at different offsets in
            // the two optional-header layouts; everything before them that
            // this reader needs is the magic.
            let (count_at, table_at) = match u16_at(head, optional)? {
                PE32 => (92, 96),
                PE32_PLUS => (108, 112),
                _ => return None,
            };
            Some(Self {
                sections: optional.checked_add(optional_len)?,
                section_count,
                directories_at: optional.checked_add(table_at)?,
                directory_count: usize::try_from(u32_at(head, optional.checked_add(count_at)?)?)
                    .ok()?,
            })
        }

        /// The debug directory's `(rva, size)`, read only when entry 6
        /// lies inside the declared optional header — before the section
        /// table, not merely inside the bytes read.
        fn debug_directory(&self, head: &[u8]) -> Option<(usize, usize)> {
            // bca: suppress(nexits) — two declared-region bounds and `?` on
            // each bounded read; the straight line is the safety argument.
            if self.directory_count <= IMAGE_DIRECTORY_ENTRY_DEBUG {
                return None;
            }
            let entry = self
                .directories_at
                .checked_add(IMAGE_DIRECTORY_ENTRY_DEBUG * 8)?;
            if entry.checked_add(8)? > self.sections {
                return None;
            }
            let rva = usize::try_from(u32_at(head, entry)?).ok()?;
            let size = usize::try_from(u32_at(head, entry.checked_add(4)?)?).ok()?;
            Some((rva, size))
        }

        /// The file offset of `[rva, rva + size)`, which must lie within
        /// one section's raw data — `SizeOfRawData`, not `VirtualSize`,
        /// since the bytes are read from the file, not a mapped image.
        fn file_offset(&self, head: &[u8], rva: usize, size: usize) -> Option<u64> {
            // bca: suppress(nexits) — `?` on each bounded field read. The
            // closure is its own space to the gate, so it carries a marker
            // too; one alone leaves the other at nexits 14 / 15.
            let end = rva.checked_add(size)?;
            (0..self.section_count).find_map(|index| {
                // bca: suppress(nexits) — see the enclosing fn.
                let at = self.sections.checked_add(index.checked_mul(SECTION_LEN)?)?;
                let virtual_address = usize::try_from(u32_at(head, at.checked_add(12)?)?).ok()?;
                let raw_size = usize::try_from(u32_at(head, at.checked_add(16)?)?).ok()?;
                let raw_at = usize::try_from(u32_at(head, at.checked_add(20)?)?).ok()?;
                let raw_end = virtual_address.checked_add(raw_size)?;
                if virtual_address > rva || end > raw_end {
                    return None;
                }
                u64::try_from(raw_at.checked_add(rva - virtual_address)?).ok()
            })
        }
    }
}
