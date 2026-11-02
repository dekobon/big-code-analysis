//! The GNU build-id note of the running executable (#1773).
//!
//! The linker writes a hash of the final object into a `PT_NOTE`
//! segment as `NT_GNU_BUILD_ID`; debuggers, `perf` and symbol servers
//! key on it. It is the one provenance field that cannot be baked in by
//! `build.rs`, because it does not exist until the link has finished,
//! so it is read back from the binary at run time.
//!
//! Support is deliberately narrow: ELF64 in the host's byte order,
//! which is every Linux target the gate runs on. Anything else — ELF32,
//! a Mach-O or PE executable, a truncated or hand-edited file, an
//! executable that has been deleted since it started — yields `None`
//! and renders as `unavailable`. Nothing is decided on this field, so
//! every failure is informational.
//!
//! All offsets come from the file and are bounds-checked against the
//! bytes actually read; arithmetic is `checked_*` throughout, so a
//! malformed header cannot panic.

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const PT_NOTE: u32 = 4;
const NT_GNU_BUILD_ID: u32 = 3;
const GNU_NOTE_NAME: &[u8] = b"GNU\0";

/// Lowercase hex of the running executable's GNU build-id, or `None`.
#[cfg(target_os = "linux")]
pub(crate) fn current_build_id() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let bytes = std::fs::read(exe).ok()?;
    build_id(&bytes).map(hex_lower)
}

/// Lowercase hex, two digits per byte.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn current_build_id() -> Option<String> {
    None
}

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
        if elf.get(..4)? != ELF_MAGIC || *elf.get(4)? != ELFCLASS64 || *elf.get(5)? != ELFDATA2LSB {
            return None;
        }
        Some(Self {
            phoff: usize::try_from(u64_at(elf, 0x20)?).ok()?,
            phentsize: usize::from(u16_at(elf, 0x36)?),
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
        let descsz = usize::try_from(u32_at(segment, at + 4)?).ok()?;
        let kind = u32_at(segment, at + 8)?;
        let namesz = usize::try_from(namesz).ok()?;
        let name_at = at.checked_add(12)?;
        let desc_at = name_at.checked_add(align4(namesz)?)?;
        let next = desc_at.checked_add(align4(descsz)?)?;
        if kind == NT_GNU_BUILD_ID
            && segment.get(name_at..name_at.checked_add(namesz)?)? == GNU_NOTE_NAME
        {
            return segment.get(desc_at..desc_at.checked_add(descsz)?);
        }
        at = next;
    }
    None
}

fn align4(n: usize) -> Option<usize> {
    n.checked_add(3).map(|v| v & !3)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at.checked_add(2)?)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at.checked_add(4)?)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    bytes
        .get(at..at.checked_add(8)?)?
        .try_into()
        .ok()
        .map(u64::from_le_bytes)
}
