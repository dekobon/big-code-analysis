//! Checkstyle 4.3 XML writer for [`OffenderRecord`] batches.
//!
//! Checkstyle is the de-facto interchange format for Jenkins, SonarQube,
//! GitLab, and most "warnings plugin" CI integrations. We emit a single
//! XML document covering every offender, grouped by source path:
//!
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <checkstyle version="4.3">
//!   <file name="src/foo.rs">
//!     <error line="42" column="5" severity="warning"
//!            message="cyclomatic 17 exceeds limit 15"
//!            source="big-code-analysis.cyclomatic"/>
//!   </file>
//! </checkstyle>
//! ```
//!
//! XML escaping is hand-rolled because the surface is tiny (five
//! entities in attribute values) and adding a new dependency is not
//! worth it for that.

#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::io::{self, Write};

use crate::output::offenders::{OffenderRecord, TOOL_ID, warn_non_utf8_path};

/// Write Checkstyle 4.3 XML for `offenders` to `writer`.
///
/// Offenders are grouped by `path` (sorted lexicographically by the
/// UTF-8 representation; non-UTF-8 paths are skipped with a warning to
/// stderr) so the output is deterministic and snapshot-friendly. Within
/// a file, errors retain their input order.
///
/// The empty case still emits a well-formed `<checkstyle version="4.3"/>`
/// document so consumers can rely on a non-empty file always being
/// parseable.
///
/// # Errors
///
/// Propagates any [`io::Error`] returned by `writer` while emitting
/// the XML envelope, the per-file `<file>` blocks, or their contained
/// `<error>` elements.
pub fn write_checkstyle<W: Write>(offenders: &[OffenderRecord], mut writer: W) -> io::Result<()> {
    writer.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n")?;

    // Group while preserving per-file insertion order. BTreeMap key is
    // the UTF-8 path; this also gives us deterministic file ordering.
    let mut by_file: BTreeMap<&str, Vec<&OffenderRecord>> = BTreeMap::new();
    for record in offenders {
        let Some(path_str) = warn_non_utf8_path("Checkstyle", &record.path) else {
            continue;
        };
        by_file.entry(path_str).or_default().push(record);
    }

    // Empty input *and* all-non-UTF-8 input both end up here with an
    // empty `by_file`, so one branch covers both cases.
    if by_file.is_empty() {
        writer.write_all(b"<checkstyle version=\"4.3\"/>\n")?;
        return Ok(());
    }

    writer.write_all(b"<checkstyle version=\"4.3\">\n")?;
    for (path_str, records) in by_file {
        writeln!(writer, "  <file name=\"{}\">", XmlAttr(path_str))?;
        for record in records {
            write_error(&mut writer, record)?;
        }
        writer.write_all(b"  </file>\n")?;
    }
    writer.write_all(b"</checkstyle>\n")
}

fn write_error<W: Write>(writer: &mut W, record: &OffenderRecord) -> io::Result<()> {
    let message = record.default_message();
    write!(writer, "    <error line=\"{}\"", record.start_line.max(1))?;
    if let Some(col) = record.start_col {
        write!(writer, " column=\"{col}\"")?;
    }
    writeln!(
        writer,
        " severity=\"{}\" message=\"{}\" source=\"{}.{}\"/>",
        record.severity.as_str(),
        XmlAttr(&message),
        TOOL_ID,
        XmlAttr(&record.metric),
    )
}

/// Format adapter that XML-escapes attribute values. We escape the five
/// XML predefined entities, emit numeric character references for TAB
/// / LF / CR (which XML 1.0 §3.3.3 attribute-value normalization would
/// otherwise collapse to a single space), and replace with `?` every
/// code point that is either:
///
/// - Forbidden by XML 1.0 §2.2's `Char` production: C0 controls minus
///   TAB/LF/CR, and the BMP non-characters `U+FFFE` / `U+FFFF`.
/// - A supplementary-plane non-character (`U+nFFFE` / `U+nFFFF` for
///   plane `n` in `1..=16`). These are technically permitted by XML
///   1.0's `Char` production, but strict consumers (libxml2 in
///   non-character-reject mode, Jenkins, SonarQube) treat them as
///   fatal. Substituting them aligns the output with the strict
///   consumer class without changing well-formedness for lenient
///   parsers.
///
/// Note: `U+FDD0`–`U+FDEF` are Unicode non-characters but are
/// permitted by XML 1.0's `Char` production and accepted by libxml2's
/// strict mode, so we pass them through.
struct XmlAttr<'a>(&'a str);

impl std::fmt::Display for XmlAttr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Unify every per-char escape behind a single `f.write_str` so the
        // `?` operator fires once per iteration instead of once per arm —
        // each `?` is a counted exit on the per-function `nexits` budget.
        // The 4-byte stack buffer covers every UTF-8 scalar; `encode_utf8`
        // borrows from it for the default arm so all arms unify as `&str`.
        let mut buf = [0u8; 4];
        for ch in self.0.chars() {
            let escaped: &str = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&apos;",
                // XML 1.0 §3.3.3 mandates attribute-value normalization:
                // a conforming parser collapses literal TAB / LF / CR
                // bytes inside an attribute value to a single space on
                // read. To round-trip these characters intact (POSIX
                // paths may contain newlines, and future message
                // templates may span lines), emit numeric character
                // references — they are exempt from normalization.
                '\t' => "&#x9;",
                '\n' => "&#xA;",
                '\r' => "&#xD;",
                // XML 1.0 §2.2's `Char` production forbids the remaining
                // C0 controls (U+0000–U+001F minus TAB/LF/CR). We also
                // substitute every plane-end non-character (`U+nFFFE` /
                // `U+nFFFF` for plane `n` in `0..=16`) — the BMP pair
                // is `Char`-illegal, the 32 supplementary-plane
                // counterparts are permitted by the spec but rejected
                // by strict libxml2-based consumers (Jenkins,
                // SonarQube). The bitmask `(cp & 0xFFFF) >= 0xFFFE`
                // catches all 34 plane-end non-characters in one test
                // without touching `U+FDD0`–`U+FDEF` (which the spec
                // and libxml2 both accept).
                c if (c as u32) < 0x20 || ((c as u32) & 0xFFFF) >= 0xFFFE => "?",
                c => c.encode_utf8(&mut buf),
            };
            f.write_str(escaped)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
#[path = "checkstyle_tests.rs"]
mod tests;
