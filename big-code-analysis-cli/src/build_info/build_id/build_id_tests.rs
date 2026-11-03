use super::elf::build_id;
use super::{HEAD_BYTES, current_build_id, hex_lower, read_at, read_head};
use super::{macho, pe};

/// `read_at` is the one I/O primitive every reader shares: it is short
/// at end of file rather than an error, and `read_head` is the same
/// call capped at `HEAD_BYTES`. Both are exercised here directly so the
/// test build on a platform with no reader of its own still has a
/// caller for them.
#[test]
fn read_at_is_short_at_end_of_file_and_read_head_is_capped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("exe");
    let mut bytes: Vec<u8> = (0..=255u8).cycle().take(HEAD_BYTES + 7).collect();
    std::fs::write(&path, &bytes).expect("write");
    assert_eq!(read_at(&path, 3, 4).as_deref(), Some(&bytes[3..7]));
    assert_eq!(
        read_at(&path, HEAD_BYTES as u64, 100).as_deref(),
        Some(&bytes[HEAD_BYTES..]),
        "a read past the end is short, not an error"
    );
    // An offset past the end seeks fine and reads nothing: `Some` of no
    // bytes, which `read_exact` then rejects. The test double `staged`
    // below models exactly this.
    assert_eq!(read_at(&path, bytes.len() as u64 + 100, 4), Some(vec![]));
    // Whether a seek to `u64::MAX` fails (Linux: `EINVAL`) or succeeds and
    // reads nothing is the platform's choice; no bytes come back either way.
    assert!(read_at(&path, u64::MAX, 1).is_none_or(|b| b.is_empty()));
    assert_eq!(read_head(&path).as_deref(), Some(&bytes[..HEAD_BYTES]));
    bytes.truncate(10);
    std::fs::write(&path, &bytes).expect("write");
    assert_eq!(read_head(&path).as_deref(), Some(&bytes[..]));
    assert_eq!(read_at(&dir.path().join("absent"), 0, 1), None);
}

/// Linux permits opening and seeking a directory, but reading it fails.
/// Unlike the missing-file row, this reaches `read_to_end`'s error path.
/// Discarding that read error makes this test the only failure.
#[cfg(target_os = "linux")]
#[test]
fn read_at_rejects_an_openable_path_that_cannot_be_read() {
    use std::io::{Read as _, Seek as _, SeekFrom};

    let dir = tempfile::tempdir().expect("tempdir");
    let mut file = std::fs::File::open(dir.path()).expect("opening a directory succeeds");
    file.seek(SeekFrom::Start(0))
        .expect("seeking a directory succeeds");
    assert!(file.read(&mut [0u8; 1]).is_err());
    assert_eq!(read_at(dir.path(), 0, 1), None);
}

#[test]
fn hex_lower_renders_two_digits_per_byte() {
    assert_eq!(hex_lower(&[]), "");
    assert_eq!(hex_lower(&[0x00, 0x0f, 0xf0, 0xff]), "000ff0ff");
}

/// A minimal little-endian ELF64 image: header, one program header of
/// `kind`, and a note segment built from `notes`.
fn elf64(kind: u32, notes: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 0x40];
    out[..4].copy_from_slice(b"\x7fELF");
    out[4] = 2;
    out[5] = 1;
    out[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
    out[0x36..0x38].copy_from_slice(&0x38u16.to_le_bytes());
    out[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    let seg_at = 0x40u64 + 0x38;
    let mut ph = vec![0u8; 0x38];
    ph[..4].copy_from_slice(&kind.to_le_bytes());
    ph[0x08..0x10].copy_from_slice(&seg_at.to_le_bytes());
    ph[0x20..0x28].copy_from_slice(&(notes.len() as u64).to_le_bytes());
    out.extend(ph);
    out.extend_from_slice(notes);
    out
}

fn note(name: &[u8], kind: u32, desc: &[u8]) -> Vec<u8> {
    let len = |s: &[u8]| u32::try_from(s.len()).expect("fixture notes are tiny");
    let mut out = Vec::new();
    out.extend(len(name).to_le_bytes());
    out.extend(len(desc).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(name);
    out.resize(out.len().next_multiple_of(4), 0);
    out.extend(desc);
    out.resize(out.len().next_multiple_of(4), 0);
    out
}

#[test]
fn build_id_is_read_past_an_unrelated_note_and_hex_rendered() {
    let id = [0xde, 0xad, 0xbe, 0xef, 0x01];
    let mut notes = note(b"GNU\0", 1, b"\0\0\0\0\x03\0\0\0\x02\0\0\0\0\0\0\0");
    notes.extend(note(b"GNU\0", 3, &id));
    let image = elf64(4, &notes);
    assert_eq!(build_id(&image), Some(&id[..]));
    assert_eq!(
        build_id(&image).map(hex_lower).as_deref(),
        Some("deadbeef01")
    );
}

#[test]
fn build_id_is_none_for_every_shape_the_reader_does_not_support() {
    let id = [1u8, 2, 3];
    let good = elf64(4, &note(b"GNU\0", 3, &id));
    assert_eq!(
        build_id(&good),
        Some(&id[..]),
        "the fixture itself must parse"
    );

    let mut wrong_magic = good.clone();
    wrong_magic[0] = 0;
    assert_eq!(build_id(&wrong_magic), None, "wrong magic");

    let mut elf32 = good.clone();
    elf32[4] = 1;
    assert_eq!(build_id(&elf32), None, "ELF32");

    let mut big_endian = good.clone();
    big_endian[5] = 2;
    assert_eq!(build_id(&big_endian), None, "big-endian");

    assert_eq!(build_id(&good[..0x30]), None, "truncated header");
    assert_eq!(build_id(&good[..0x50]), None, "truncated program header");
    assert_eq!(
        build_id(&elf64(1, &note(b"GNU\0", 3, &id))),
        None,
        "not a PT_NOTE segment"
    );
    assert_eq!(
        build_id(&elf64(4, &note(b"XYZ\0", 3, &id))),
        None,
        "not the GNU note"
    );
    assert_eq!(
        build_id(&elf64(4, &note(b"GNU\0", 7, &id))),
        None,
        "not NT_GNU_BUILD_ID"
    );

    // Every prefix of a valid image must read as absent, not panic and
    // not misparse. This walks each `?` in the header and program-header
    // readers; it never reaches the note reader, because the shortened
    // image keeps the program header's declared segment size and the
    // segment slice fails first.
    for cut in 0..good.len() {
        assert_eq!(build_id(&good[..cut]), None, "truncated at byte {cut}");
    }
    // The note reader's own short reads need a segment whose *declared*
    // size is honest: 4 bytes reach `descsz`'s read, 8 reach `kind`'s,
    // 12 reach the name's. These execute each read's `None` arm; they
    // cannot tell a failed read from an infallible one, because the
    // next read fails the same way and no id can come out of a note
    // that short — the outcome, not the mechanism, is what is pinned.
    for short in [&[0u8; 4][..], &[0u8; 8], &[0u8; 12]] {
        assert_eq!(
            build_id(&elf64(4, short)),
            None,
            "{}-byte note segment",
            short.len()
        );
    }

    let mut overflow = good.clone();
    overflow[0x40 + 0x20..0x40 + 0x28].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(build_id(&overflow), None, "segment size past the end");
    let mut far_offset = good.clone();
    far_offset[0x40 + 0x08..0x40 + 0x10].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(build_id(&far_offset), None, "segment offset past the end");

    let mut more_headers_than_exist = good.clone();
    more_headers_than_exist[0x38..0x3a].copy_from_slice(&7u16.to_le_bytes());
    assert_eq!(
        build_id(&more_headers_than_exist),
        Some(&id[..]),
        "a phnum past the table still finds the note in the headers present"
    );

    // `phentsize` below ELF64's 56 makes every index alias one header, so
    // a single `PT_NOTE` would be walked `phnum` times; the reader refuses
    // it (and so does `readelf`). Fails if the `< PHENTSIZE` guard is
    // removed: index 0 still reads the real header and finds the id.
    let mut zero_phentsize = good.clone();
    zero_phentsize[0x36..0x38].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(build_id(&zero_phentsize), None, "phentsize 0");
    let mut short_phentsize = good.clone();
    short_phentsize[0x36..0x38].copy_from_slice(&0x37u16.to_le_bytes());
    assert_eq!(build_id(&short_phentsize), None, "phentsize 55");

    let mut huge_phoff = good.clone();
    huge_phoff[0x20..0x28].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        build_id(&huge_phoff),
        None,
        "program-header offset past the end"
    );

    // A name length that overruns the segment: the note walk fails
    // closed. (A clamped slice would fail closed too — the name would
    // not compare equal — so this pins the outcome, not the mechanism;
    // the bounded-read spelling is the module's safety argument.)
    let mut lying_namesz = good;
    let note_at = 0x40 + 0x38;
    lying_namesz[note_at..note_at + 4].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    assert_eq!(build_id(&lying_namesz), None, "namesz past the segment");

    // A descriptor length that overruns: same, on the other field.
    let mut lying_descsz = elf64(4, &note(b"GNU\0", 3, &id));
    lying_descsz[note_at + 4..note_at + 8].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    assert_eq!(build_id(&lying_descsz), None, "descsz past the segment");
}

/// The first program-header lookup is out of bounds; advancing to the
/// second would overflow a 64-bit offset. Both must fail without panicking.
/// Replacing the checked offset sum with plain addition fails this test.
#[test]
fn elf_program_header_offsets_cannot_wrap() {
    let mut image = elf64(4, &note(b"GNU\0", 3, &[1, 2, 3]));
    image[0x20..0x28].copy_from_slice(&(u64::MAX - 0x10).to_le_bytes());
    image[0x38..0x3a].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(build_id(&image), None);
}

/// A capability probe, not a contract: the linker on this host is what
/// decides whether the test binary carries an id at all. When it does,
/// the reader must find it and render it as lowercase hex — never a
/// fixed length, since each format's id has its own; when it does not,
/// `unavailable` is the documented answer.
/// On Linux the dispatcher must reach the ELF reader: the answer for the
/// running executable equals what `elf::build_id` reads from its head.
/// Vacuously true on a host whose linker emitted no note (both `None`),
/// and the one test that fails if `current_build_id` is routed to
/// another reader — the capability probe below cannot tell.
#[cfg(target_os = "linux")]
#[test]
fn linux_dispatch_reaches_the_elf_reader() {
    let exe = std::env::current_exe().expect("current_exe");
    let expected = read_head(&exe).and_then(|head| build_id(&head).map(hex_lower));
    assert_eq!(current_build_id(), expected);
}

#[test]
fn running_executable_yields_hex_or_nothing() {
    match current_build_id() {
        Some(id) => assert!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "{id}"
        ),
        None => eprintln!("no build id in the test binary on this host"),
    }
}

// ---------------------------------------------------------------- Mach-O

const UUID: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
];

fn load_command(cmd: u32, body: &[u8]) -> Vec<u8> {
    let mut out = cmd.to_le_bytes().to_vec();
    out.extend((8 + u32::try_from(body.len()).expect("fixture commands are tiny")).to_le_bytes());
    out.extend(body);
    out
}

/// A minimal thin little-endian Mach-O 64 image carrying `commands`.
fn macho64(magic: u32, ncmds: u32, commands: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 32];
    out[..4].copy_from_slice(&magic.to_le_bytes());
    out[16..20].copy_from_slice(&ncmds.to_le_bytes());
    out[20..24].copy_from_slice(&u32::try_from(commands.len()).expect("tiny").to_le_bytes());
    out.extend_from_slice(commands);
    out
}

/// Fails if `LC_UUID` is matched by position rather than by `cmd`
/// (the segment command comes first), or if the walk stops at the first
/// command (`at = next` dropped).
#[test]
fn macho_uuid_is_read_past_an_unrelated_load_command_and_hex_rendered() {
    let mut commands = load_command(0x19, &[0u8; 64]);
    commands.extend(load_command(0x1b, &UUID));
    let image = macho64(0xfeed_facf, 2, &commands);
    assert_eq!(macho::build_id(&image), Some(&UUID[..]));
    assert_eq!(
        macho::build_id(&image).map(hex_lower).as_deref(),
        Some("0123456789abcdeffedcba9876543210")
    );
}

/// Each row names the production guard it exercises; removing that guard
/// turns the row's `None` into `Some`. The `cmdsize 0` and `cmdsize 4`
/// rows are the exception: no guard answers them — the inverted body
/// range does — so they pin the outcome (the walk terminates, with
/// `ncmds = u32::MAX` on offer, and yields `None`) rather than a line.
/// A reader that clamped the range and advanced by the 8-byte header
/// would pass both rows; the image's own end bounds it.
#[test]
fn macho_build_id_is_none_for_every_shape_the_reader_does_not_support() {
    let good = macho64(0xfeed_facf, 1, &load_command(0x1b, &UUID));
    assert_eq!(
        macho::build_id(&good),
        Some(&UUID[..]),
        "the fixture itself must parse"
    );

    assert_eq!(
        macho::build_id(&macho64(0xfeed_face, 1, &load_command(0x1b, &UUID))),
        None,
        "MH_MAGIC (32-bit)"
    );
    assert_eq!(
        macho::build_id(&macho64(0xcffa_edfe, 1, &load_command(0x1b, &UUID))),
        None,
        "MH_CIGAM_64 (byte-swapped)"
    );
    assert_eq!(
        macho::build_id(&macho64(0xbeba_feca, 1, &load_command(0x1b, &UUID))),
        None,
        "FAT_MAGIC (universal)"
    );
    assert_eq!(
        macho::build_id(&macho64(0, 1, &load_command(0x1b, &UUID))),
        None,
        "wrong magic"
    );

    for cut in 0..good.len() {
        assert_eq!(
            macho::build_id(&good[..cut]),
            None,
            "truncated at byte {cut}"
        );
    }

    // `ncmds` past the table: the walk runs off the end of the image
    // (nothing follows the one command, so the bounded read, not the
    // `sizeofcmds` bound, is what stops it — `past_sizeofcmds` below is
    // the row for that bound) and fails closed.
    let mut more_commands_than_exist = good.clone();
    more_commands_than_exist[16..20].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        macho::build_id(&more_commands_than_exist),
        Some(&UUID[..]),
        "the id is found before the table runs out"
    );
    assert_eq!(
        macho::build_id(&macho64(0xfeed_facf, 3, &load_command(0x19, &[0u8; 8]))),
        None,
        "ncmds past the table, no id"
    );

    // `cmdsize` of 0 must terminate: the body read's inverted range is
    // what stops the walk, and `u32::MAX` commands would otherwise spin.
    let mut zero_cmdsize = macho64(0xfeed_facf, u32::MAX, &load_command(0x19, &[0u8; 8]));
    zero_cmdsize[36..40].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(macho::build_id(&zero_cmdsize), None, "cmdsize 0");
    let mut short_cmdsize = good.clone();
    short_cmdsize[36..40].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(macho::build_id(&short_cmdsize), None, "cmdsize 4");

    // `LC_UUID` with a size other than 24 is malformed, not a shorter id.
    assert_eq!(
        macho::build_id(&macho64(0xfeed_facf, 1, &load_command(0x1b, &[0u8; 24]))),
        None,
        "LC_UUID with cmdsize 32"
    );
    let truncated_uuid = macho64(0xfeed_facf, 1, &load_command(0x1b, &UUID[..8]));
    assert_eq!(
        macho::build_id(&truncated_uuid),
        None,
        "LC_UUID with cmdsize 16"
    );

    // A valid `LC_UUID` that lies past the declared `sizeofcmds` is not
    // the image's id: the walk stops at the declared end even though the
    // bytes are present. Fails if the `next > end` bound is removed —
    // the second turn then reads the UUID and returns it.
    let mut past_sizeofcmds = macho64(0xfeed_facf, 2, &load_command(0x19, &[0u8; 8]));
    past_sizeofcmds.extend(load_command(0x1b, &UUID));
    assert_eq!(
        macho::build_id(&past_sizeofcmds),
        None,
        "LC_UUID past sizeofcmds"
    );

    assert_eq!(
        macho::build_id(&macho64(0xfeed_facf, 0, &[])),
        None,
        "no load commands"
    );
}

// -------------------------------------------------------------------- PE

const GUID_AND_AGE: [u8; 20] = [
    0xa0, 0xa1, 0xa2, 0xa3, 0xb0, 0xb1, 0xc0, 0xc1, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7,
    0x07, 0x00, 0x00, 0x00,
];

/// One `IMAGE_DEBUG_DIRECTORY` entry of `kind` whose record of `size`
/// bytes sits at file offset `at`.
fn debug_entry(kind: u32, size: u32, at: u32) -> Vec<u8> {
    let mut out = vec![0u8; 28];
    out[12..16].copy_from_slice(&kind.to_le_bytes());
    out[16..20].copy_from_slice(&size.to_le_bytes());
    out[24..28].copy_from_slice(&at.to_le_bytes());
    out
}

fn rsds(signature: [u8; 4]) -> Vec<u8> {
    let mut out = signature.to_vec();
    out.extend(GUID_AND_AGE);
    out
}

struct PeLayout {
    plus: bool,
    /// `SizeOfOptionalHeader` to declare; `None` is the real size.
    optional_len: Option<u16>,
    /// Where the debug directory sits in the file. Its RVA is this plus
    /// `va_shift`: the section's `VirtualAddress` is its `PointerToRawData`
    /// plus the shift, and an unrelated section precedes it in the table
    /// when the shift is non-zero, so the RVA walk must both pick the
    /// right section and translate within it.
    directory_at: u32,
    va_shift: u32,
    directory: Vec<u8>,
    /// `(file offset, record)` pairs appended after the directory.
    records: Vec<(u32, Vec<u8>)>,
}

/// A PE image: DOS stub, `PE\0\0`, COFF header, optional header (PE32 or
/// PE32+), one section whose raw data covers `directory_at ..` the end
/// of the file, then the debug directory and the records at the offsets
/// they name. Everything before the section data stays inside 64 KiB;
/// the directory and records sit wherever `directory_at` says.
fn pe(layout: &PeLayout) -> Vec<u8> {
    let optional_len: u16 = layout
        .optional_len
        .unwrap_or(if layout.plus { 240 } else { 224 });
    let mut out = vec![0u8; 0x40];
    out[..2].copy_from_slice(b"MZ");
    out[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    out.resize(0x80, 0);
    out.extend_from_slice(b"PE\0\0");
    let section_count: u16 = if layout.va_shift == 0 { 1 } else { 2 };
    let mut coff = vec![0u8; 20];
    coff[2..4].copy_from_slice(&section_count.to_le_bytes());
    coff[16..18].copy_from_slice(&optional_len.to_le_bytes());
    out.extend(coff);
    let mut optional = vec![0u8; if layout.plus { 240 } else { 224 }];
    let (magic, count_at, table_at) = if layout.plus {
        (0x20bu16, 108, 112)
    } else {
        (0x10b, 92, 96)
    };
    optional[..2].copy_from_slice(&magic.to_le_bytes());
    optional[count_at..count_at + 4].copy_from_slice(&16u32.to_le_bytes());
    let debug = table_at + 6 * 8;
    optional[debug..debug + 4]
        .copy_from_slice(&(layout.directory_at + layout.va_shift).to_le_bytes());
    optional[debug + 4..debug + 8].copy_from_slice(
        &u32::try_from(layout.directory.len())
            .expect("tiny")
            .to_le_bytes(),
    );
    // The declared length decides where the section table starts. A
    // short declaration writes the section header into the optional
    // header's own bytes and then lays the rest of the real optional
    // header (the data directories) behind it, so entry 6 keeps the
    // file offset the reader computes for it and is reachable from the
    // slice — only the declared bound says it is not the header's.
    let declared = usize::from(optional_len);
    out.extend(&optional[..declared]);
    let tail = optional[declared..].to_vec();
    let file_end = layout
        .records
        .iter()
        .map(|(at, record)| at + u32::try_from(record.len()).expect("tiny"))
        .chain([layout.directory_at + u32::try_from(layout.directory.len()).expect("tiny")])
        .max()
        .expect("at least the directory");
    if layout.va_shift != 0 {
        // An unrelated section whose virtual range ends below the
        // directory's RVA: a reader that takes the first section, or
        // ignores `VirtualAddress`, lands here.
        let mut text = vec![0u8; 40];
        text[..5].copy_from_slice(b".text");
        text[12..16].copy_from_slice(&0x1000u32.to_le_bytes());
        text[16..20].copy_from_slice(&0x200u32.to_le_bytes());
        text[20..24].copy_from_slice(&0x200u32.to_le_bytes());
        out.extend(text);
    }
    // With a shift the section's raw data starts 0x80 before the
    // directory, so the directory sits at a non-zero offset within it and
    // `rva - VirtualAddress` is 0x80, not 0 — the translation is observable.
    let raw_at = layout.directory_at - if layout.va_shift == 0 { 0 } else { 0x80 };
    let mut section = vec![0u8; 40];
    section[..5].copy_from_slice(b".data");
    section[12..16].copy_from_slice(&(raw_at + layout.va_shift).to_le_bytes());
    section[16..20].copy_from_slice(&(file_end - raw_at).to_le_bytes());
    section[20..24].copy_from_slice(&raw_at.to_le_bytes());
    out.extend(section);
    if !tail.is_empty() {
        let overlap = 40.min(tail.len());
        out.extend(&tail[overlap..]);
    }
    assert!(
        out.len() <= layout.directory_at as usize,
        "headers must precede the section data"
    );
    out.resize(file_end as usize, 0);
    let dir = layout.directory_at as usize;
    out[dir..dir + layout.directory.len()].copy_from_slice(&layout.directory);
    for (at, record) in &layout.records {
        let at = *at as usize;
        out[at..at + record.len()].copy_from_slice(record);
    }
    out
}

/// `(head, read_at)` for `image`: the head is the first `HEAD_BYTES`,
/// and the closure serves the rest from the same bytes, short at EOF
/// exactly as the production `read_at` is — including an offset past
/// the end, which that one answers with `Some` of nothing (the seek
/// succeeds and the read returns zero bytes), never `None`. Only an
/// offset `usize` cannot hold is `None`, which stands in for the
/// `u64` seek position the production closure would reject.
fn staged(image: &[u8]) -> (&[u8], impl Fn(u64, usize) -> Option<Vec<u8>> + '_) {
    let head = &image[..image.len().min(HEAD_BYTES)];
    let read_at = move |at: u64, len: usize| {
        let at = usize::try_from(at).ok()?;
        let bytes = image.get(at..).unwrap_or_default();
        Some(bytes[..bytes.len().min(len)].to_vec())
    };
    (head, read_at)
}

fn pe_id(image: &[u8]) -> Option<Vec<u8>> {
    let (head, read_at) = staged(image);
    pe::build_id(head, read_at)
}

/// The debug directory and the record both sit past 64 KiB, so this
/// fails if either stage reads from `head` instead of through `read_at`
/// — the fixture is the shape a real `link.exe` image has.
#[test]
fn pe32_plus_codeview_past_the_head_is_read_through_the_second_stage_and_hex_rendered() {
    let directory_at = u32::try_from(HEAD_BYTES + 0x100).expect("fits");
    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at,
        directory: debug_entry(2, 24, directory_at + 0x40),
        records: vec![(directory_at + 0x40, rsds(*b"RSDS"))],
    });
    assert!(image.len() > HEAD_BYTES);
    assert_eq!(pe_id(&image).as_deref(), Some(&GUID_AND_AGE[..]));
    assert_eq!(
        pe_id(&image).map(|id| hex_lower(&id)).as_deref(),
        Some("a0a1a2a3b0b1c0c1d0d1d2d3d4d5d6d707000000")
    );
}

/// Fails if the PE32 directory offsets (92/96) are read at the PE32+
/// positions (108/112): the count then reads 0 and the id is absent.
#[test]
fn pe32_codeview_inside_the_head_is_read() {
    let image = pe(&PeLayout {
        plus: false,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory: debug_entry(2, 24, 0x440),
        records: vec![(0x440, rsds(*b"RSDS"))],
    });
    assert!(image.len() < HEAD_BYTES);
    assert_eq!(pe_id(&image).as_deref(), Some(&GUID_AND_AGE[..]));
}

/// Fails if the entry walk returns on the first `CODEVIEW` entry rather
/// than continuing past a non-`RSDS` one, or if an entry of another
/// type is mistaken for `CODEVIEW`: the leading type-16 entry points at
/// a well-formed `RSDS` record whose GUID differs, so a reader without
/// the type filter answers with the decoy rather than `None`.
#[test]
fn pe_walks_past_unrelated_and_non_rsds_entries_to_a_later_codeview() {
    let mut decoy = rsds(*b"RSDS");
    decoy[4] ^= 0xff;
    let mut directory = debug_entry(16, 24, 0x480);
    directory.extend(debug_entry(2, 24, 0x4c0));
    directory.extend(debug_entry(2, 24, 0x500));
    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory,
        records: vec![
            (0x480, decoy),
            (0x4c0, rsds(*b"NB10")),
            (0x500, rsds(*b"RSDS")),
        ],
    });
    assert_eq!(pe_id(&image).as_deref(), Some(&GUID_AND_AGE[..]));
}

/// An I/O failure differs from a successful short read. A failed directory
/// read stops the parse; a failed record read still permits a later entry.
/// Making failed reads panic or stopping at the first directory entry fails.
#[test]
fn pe_read_failures_stop_the_directory_but_skip_unreadable_records() {
    use std::cell::RefCell;

    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory: [debug_entry(2, 24, 0x480), debug_entry(2, 24, 0x4c0)].concat(),
        records: vec![(0x480, rsds(*b"RSDS")), (0x4c0, rsds(*b"RSDS"))],
    });
    let (head, read) = staged(&image);
    for (failed_offsets, expected_calls, expected_id) in [
        (vec![0x400], vec![(0x400, 56)], None),
        (
            vec![0x480, 0x4c0],
            vec![(0x400, 56), (0x480, 24), (0x4c0, 24)],
            None,
        ),
        (
            vec![0x480],
            vec![(0x400, 56), (0x480, 24), (0x4c0, 24)],
            Some(&GUID_AND_AGE[..]),
        ),
    ] {
        let calls = RefCell::new(Vec::new());
        let actual = pe::build_id(head, |offset, len| {
            calls.borrow_mut().push((offset, len));
            if failed_offsets.contains(&offset) {
                None
            } else {
                read(offset, len)
            }
        });
        assert_eq!(
            actual.as_deref(),
            expected_id,
            "failed reads {failed_offsets:x?}"
        );
        assert_eq!(calls.into_inner(), expected_calls);
    }
}

/// A short record is skipped before its bytes are requested. The following
/// entry must still be read, rather than ending the search with the first.
/// Limiting the directory walk to one entry fails this test.
#[test]
fn pe_skips_undersized_records_without_reading_them() {
    use std::cell::RefCell;

    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory: [debug_entry(2, 20, 0x480), debug_entry(2, 24, 0x4c0)].concat(),
        records: vec![(0x480, rsds(*b"RSDS")), (0x4c0, rsds(*b"RSDS"))],
    });
    let (head, read) = staged(&image);
    let calls = RefCell::new(Vec::new());
    let actual = pe::build_id(head, |offset, len| {
        calls.borrow_mut().push((offset, len));
        read(offset, len)
    });
    assert_eq!(actual.as_deref(), Some(&GUID_AND_AGE[..]));
    assert_eq!(calls.into_inner(), [(0x400, 56), (0x4c0, 24)]);
}

/// Exercise both stages through the actual file reader, including offsets
/// beyond `HEAD_BYTES`. A closure serving slices alone cannot check seeking.
/// Seeking to zero instead of the requested offset fails this test.
#[test]
fn pe_reads_a_file_backed_codeview_beyond_the_head() {
    let directory_at = u32::try_from(HEAD_BYTES + 0x100).expect("fits");
    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at,
        directory: debug_entry(2, 24, directory_at + 0x40),
        records: vec![(directory_at + 0x40, rsds(*b"RSDS"))],
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("fixture.exe");
    std::fs::write(&path, image).expect("write PE fixture");
    let head = read_head(&path).expect("read head");
    assert_eq!(head.len(), HEAD_BYTES);
    let actual = pe::build_id(&head, |offset, len| read_at(&path, offset, len));
    assert_eq!(actual.as_deref(), Some(&GUID_AND_AGE[..]));
}

/// Keep the directory and record available to the second-stage reader while
/// truncating header fields. Rejection must precede any request for data.
/// Making fixed-width reads assume a complete header fails this test.
#[test]
fn pe_truncated_directory_and_section_fields_never_trigger_io() {
    use std::cell::Cell;

    for plus in [false, true] {
        let image = pe(&PeLayout {
            plus,
            optional_len: None,
            va_shift: 0,
            directory_at: 0x400,
            directory: debug_entry(2, 24, 0x440),
            records: vec![(0x440, rsds(*b"RSDS"))],
        });
        let (_, read) = staged(&image);
        let optional = 0x80 + 4 + 20;
        let directory_entry = optional + if plus { 112 } else { 96 } + 6 * 8;
        let section = optional + if plus { 240 } else { 224 };
        for cut in [
            directory_entry + 3,
            directory_entry + 7,
            section + 15,
            section + 19,
            section + 23,
        ] {
            let calls = Cell::new(0);
            let actual = pe::build_id(&image[..cut], |offset, len| {
                calls.set(calls.get() + 1);
                read(offset, len)
            });
            assert_eq!(actual, None, "PE32+={plus}, header cut at {cut}");
            assert_eq!(calls.get(), 0, "header rejected before second-stage I/O");
        }
    }
}

/// The directory's RVA is not its file offset: the section is mapped
/// 0x2000 above where it sits in the file, its raw data starts 0x80
/// before the directory, and an unrelated section precedes it in the
/// table. Fails if `file_offset` returns `PointerToRawData` without
/// adding `rva - VirtualAddress` (every other fixture has that
/// difference zero), or takes the first section rather than the one
/// covering the RVA.
#[test]
fn pe_debug_directory_rva_is_translated_through_its_section() {
    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0x2000,
        directory_at: 0x480,
        directory: debug_entry(2, 24, 0x4c0),
        records: vec![(0x4c0, rsds(*b"RSDS"))],
    });
    assert_eq!(pe_id(&image).as_deref(), Some(&GUID_AND_AGE[..]));
}

/// The debug-directory read is capped at `MAX_DEBUG_ENTRIES`: it is the
/// only bound between a declared `size` of 4 GiB and a `Vec` of that
/// capacity. Sixty-five entries, the `CodeView` one last, read as absent.
/// Fails if the `.min(…)` on the directory read is dropped: the 65th
/// entry is then reached and its record returned.
#[test]
fn pe_debug_directory_entries_past_the_cap_are_not_read() {
    let mut directory = Vec::new();
    for _ in 0..64 {
        directory.extend(debug_entry(16, 24, 0x900));
    }
    directory.extend(debug_entry(2, 24, 0x900));
    let image = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory,
        records: vec![(0x900, rsds(*b"RSDS"))],
    });
    assert_eq!(pe_id(&image), None);
}

/// Each row names the guard it exercises; removing that guard turns the
/// row's `None` into `Some`, because every fixture here keeps the bytes
/// behind the lie in place — the declared-region rows exist precisely
/// so that a reader bounded only by the slice cannot pass them.
#[test]
fn pe_build_id_is_none_for_every_shape_the_reader_does_not_support() {
    let layout = |directory: Vec<u8>, records: Vec<(u32, Vec<u8>)>| PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x400,
        directory,
        records,
    };
    let good = pe(&layout(
        debug_entry(2, 24, 0x440),
        vec![(0x440, rsds(*b"RSDS"))],
    ));
    assert_eq!(
        pe_id(&good).as_deref(),
        Some(&GUID_AND_AGE[..]),
        "the fixture itself must parse"
    );
    let optional = 0x80 + 4 + 20;

    let mut wrong_dos = good.clone();
    wrong_dos[0] = 0;
    assert_eq!(pe_id(&wrong_dos), None, "no MZ");
    let mut wrong_pe = good.clone();
    wrong_pe[0x80] = 0;
    assert_eq!(pe_id(&wrong_pe), None, "no PE signature");
    let mut rom = good.clone();
    rom[optional..optional + 2].copy_from_slice(&0x107u16.to_le_bytes());
    assert_eq!(pe_id(&rom), None, "ROM optional header");

    for cut in 0..good.len() {
        assert_eq!(pe_id(&good[..cut]), None, "truncated at byte {cut}");
    }

    // Declared-region bounds: the bytes are all still there, only the
    // headers' description of them shrinks.
    let mut six_directories = good.clone();
    six_directories[optional + 108..optional + 112].copy_from_slice(&6u32.to_le_bytes());
    assert_eq!(pe_id(&six_directories), None, "NumberOfRvaAndSizes = 6");
    // The optional header declares 120 bytes, so the data directories
    // sit behind the section header — still present, still readable from
    // the slice, and not the optional header's to offer. Fails if the
    // declared-length bound is removed: the reader then takes entry 6
    // from those bytes and finds the directory.
    let short_optional = pe(&PeLayout {
        plus: true,
        optional_len: Some(120),
        va_shift: 0,
        directory_at: 0x400,
        directory: debug_entry(2, 24, 0x440),
        records: vec![(0x440, rsds(*b"RSDS"))],
    });
    assert_eq!(
        pe_id(&short_optional),
        None,
        "directory[6] past SizeOfOptionalHeader"
    );

    // RVA mapping through the section table.
    let section = optional + 240;
    let mut no_section = good.clone();
    no_section[0x84 + 2..0x84 + 4].copy_from_slice(&0u16.to_le_bytes());
    assert_eq!(pe_id(&no_section), None, "no section covers the RVA");
    let mut rva_past_raw = good.clone();
    rva_past_raw[section + 16..section + 20].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        pe_id(&rva_past_raw),
        None,
        "directory past the section's SizeOfRawData"
    );
    let mut rva_below_section = good.clone();
    rva_below_section[section + 12..section + 16].copy_from_slice(&0x800u32.to_le_bytes());
    assert_eq!(
        pe_id(&rva_below_section),
        None,
        "directory RVA below the section's VirtualAddress"
    );

    // The directory and the record themselves.
    assert_eq!(
        pe_id(&pe(&layout(
            debug_entry(2, 24, 0x440)[..27].to_vec(),
            vec![(0x440, rsds(*b"RSDS"))]
        ))),
        None,
        "debug directory of 27 bytes holds no complete entry"
    );
    assert_eq!(
        pe_id(&pe(&layout(
            debug_entry(2, 20, 0x440),
            vec![(0x440, rsds(*b"RSDS"))]
        ))),
        None,
        "CodeView SizeOfData 20"
    );
    assert_eq!(
        pe_id(&pe(&layout(
            debug_entry(2, 24, 0x440),
            vec![(0x440, rsds(*b"NB10"))]
        ))),
        None,
        "non-RSDS CodeView only"
    );
    assert_eq!(
        pe_id(&pe(&layout(
            debug_entry(16, 24, 0x440),
            vec![(0x440, rsds(*b"RSDS"))]
        ))),
        None,
        "no CodeView entry"
    );
    let mut record_past_eof = good.clone();
    record_past_eof.truncate(0x440 + 20);
    assert_eq!(pe_id(&record_past_eof), None, "record short at end of file");
    let mut record_far_away = good;
    record_far_away[0x400 + 24..0x400 + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(pe_id(&record_far_away), None, "record offset past the end");
    // The directory stage is exact too, not only the record stage: a
    // directory declared as two entries with the file ending inside the
    // second must not yield the first entry's id, which sits before the
    // directory and is fully present. Fails if that read is unchecked.
    let mut directory_past_eof = pe(&PeLayout {
        plus: true,
        optional_len: None,
        va_shift: 0,
        directory_at: 0x440,
        directory: [debug_entry(2, 24, 0x400), debug_entry(16, 24, 0x400)].concat(),
        records: vec![(0x400, rsds(*b"RSDS"))],
    });
    directory_past_eof.truncate(0x440 + 40);
    assert_eq!(
        pe_id(&directory_past_eof),
        None,
        "debug directory short at end of file"
    );
}
