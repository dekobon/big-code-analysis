//! The crate's two GFM escaping policies, one for an ordinary table
//! cell and one for an identifier in a code span, shared by every
//! Markdown emitter: the `check` summary (`check_format`), the AST
//! report and its provenance footer (`markdown_report`, `provenance`),
//! the VCS report through the report's cell renderer, and `exemptions`.
//!
//! The two are deliberately not one function. GFM processes a code span's
//! content literally, so the backslash an ordinary cell must escape would
//! render doubled inside a span (issue #846); and an ordinary cell has no
//! delimiter to protect, so the span's backtick replacement would be noise
//! there. An emitter picks the one that matches the construct it writes.
//!
//! Neither policy touches GFM's extended autolinks: a bare `www.` or
//! `http://` run in a cell still becomes a link under a renderer that
//! implements that extension. No emitter here writes a cell where that
//! has mattered, and preserving the text's identity does not require
//! suppressing it.
//!
//! GFM spec sections: [tables](https://github.github.com/gfm/#tables-extension-),
//! [backslash escapes](https://github.github.com/gfm/#backslash-escapes),
//! [code spans](https://github.github.com/gfm/#code-spans) and
//! [extended autolinks](https://github.github.com/gfm/#autolinks-extension-).

/// Escape a GFM table cell so its text renders as itself. `|` is the
/// column separator, and a literal `\` is escaped too so that a path
/// `a\|b` renders as itself rather than as `a\` followed by a column
/// break — one backslash per metacharacter, never two, so a `\` already
/// in the cell can never pair with the one added for a following `|`.
/// Each newline and carriage return becomes one space (so CRLF becomes
/// two) — multi-line cells would break the table layout entirely.
///
/// The remaining escapes keep a path's *identity* intact in the rendered
/// output: `<` opens raw HTML (`a<img src=x onerror=…>.py` became an
/// element under a non-sanitising renderer), `&` starts an entity
/// (`a&amp;b.py` displayed as `a&b.py`), and `` ` `` / `*` / `_` / `~` /
/// `[` / `]` open code spans, emphasis, strikethrough and links
/// (`__init__.py` rendered as bold `init.py`). GFM honours a backslash
/// before any ASCII punctuation, so each renders as the literal character.
///
/// Unified from three per-emitter copies in issue #439, so every GFM
/// table in this crate shares one escaping policy.
pub(crate) fn escape_gfm_cell(s: &str) -> String {
    let needs_escape = |ch: char| {
        matches!(
            ch,
            '\\' | '|' | '<' | '&' | '`' | '*' | '_' | '~' | '[' | ']'
        )
    };
    // Size for every escape up front: `_` is in most identifiers and
    // paths, so the plain `s.len()` guess reallocated on most cells.
    let escapes = s.chars().filter(|&ch| needs_escape(ch)).count();
    let mut out = String::with_capacity(s.len() + escapes);
    for ch in s.chars() {
        if needs_escape(ch) {
            out.push('\\');
            out.push(ch);
        } else if matches!(ch, '\n' | '\r') {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

/// Escape an identifier for display inside a backtick code span in a GFM
/// table cell. Unlike [`escape_gfm_cell`], this must **not** double literal
/// backslashes: inside a code span GFM treats content literally and never
/// processes backslash escapes, so `escape_gfm_cell`'s `\` → `\\` rule
/// (correct for a bare cell) would render a doubled backslash (issue #846).
/// The only characters that still need handling inside the span are the
/// backtick (replaced with the modifier-letter U+02CB so it cannot close
/// the span), the pipe (GFM's table parser strips `\|` → `|` *before*
/// code-span parsing, so it renders correctly), and line breaks (each
/// becomes a space so a multi-line name cannot break the table layout).
///
/// The provenance footer reuses this for the seed paths it renders inside
/// an italic line, where the span is what keeps a `_` or `*` in a path out
/// of the surrounding emphasis (issue #848).
pub(crate) fn escape_name(s: &str) -> String {
    let mut span = String::with_capacity(s.len() + 2);
    span.push('`');
    for ch in s.chars() {
        match ch {
            '`' => span.push('\u{02CB}'),
            '|' => span.push_str("\\|"),
            '\n' | '\r' => span.push(' '),
            _ => span.push(ch),
        }
    }
    span.push('`');
    span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_gfm_cell_escapes_pipe_and_collapses_newlines() {
        // Path containing `|` (legal on Unix) would otherwise split
        // the GFM cell into two columns and corrupt the table layout.
        assert_eq!(escape_gfm_cell("a|b"), "a\\|b");
        // Newlines inside a cell would also break the row; each
        // line-break byte becomes one space, so a CRLF pair is two.
        assert_eq!(escape_gfm_cell("a\nb\rc"), "a b c");
        assert_eq!(escape_gfm_cell("a\r\nb"), "a  b");
    }

    #[test]
    fn escape_gfm_cell_escapes_backslash_before_pipe() {
        // A literal `\` in a path must round-trip as `\` in the
        // rendered cell. If we only escape `|`, then the input
        // `a\|b` (literal backslash + pipe) becomes `a\\|b` — which
        // GFM renders as `a\` followed by a column break, splitting
        // the cell. Escaping `\` to `\\` first makes the same input
        // become `a\\\|b`, which renders as the literal `a\|b`.
        assert_eq!(escape_gfm_cell("a\\b"), "a\\\\b");
        assert_eq!(escape_gfm_cell("a\\|b"), "a\\\\\\|b");
        // Regression for #439: a Windows-style path must render
        // literally. GFM keeps a backslash before a non-punctuation
        // character such as `m`, so `src\main.rs` would survive on its
        // own; the escape is for the `\` that precedes a punctuation
        // character or a `|`, and the policy escapes every `\` so the
        // two cases cannot drift.
        assert_eq!(escape_gfm_cell("src\\main.rs"), "src\\\\main.rs");
        // A trailing backslash must not consume the closing column border.
        assert_eq!(escape_gfm_cell("dir\\"), "dir\\\\");
    }

    /// A path's Markdown and HTML metacharacters must render as the
    /// literal path: unescaped, `<img …>` became an element under a
    /// non-sanitising GFM renderer, `&amp;` collapsed to `&`, and
    /// `__init__.py` rendered as bold `init.py`.
    #[test]
    fn escape_gfm_cell_neutralises_markup_in_paths() {
        assert_eq!(
            escape_gfm_cell("a<img src=x onerror=alert(1)>.py"),
            "a\\<img src=x onerror=alert(1)>.py"
        );
        assert_eq!(escape_gfm_cell("a&amp;b.py"), "a\\&amp;b.py");
        assert_eq!(escape_gfm_cell("__init__.py"), "\\_\\_init\\_\\_.py");
        assert_eq!(escape_gfm_cell("x*y~z`w[v].rs"), "x\\*y\\~z\\`w\\[v\\].rs");
        // Ordinary paths are untouched: no escape is spent on a `.`, `/`,
        // `-` or `#`, which GFM renders literally in a cell.
        assert_eq!(escape_gfm_cell("src/a-b.c#1.rs"), "src/a-b.c#1.rs");
    }

    #[test]
    fn escape_name_wraps_in_backticks() {
        assert_eq!(escape_name("hello"), "`hello`");
        assert_eq!(escape_name("a|b"), "`a\\|b`");
        assert_eq!(escape_name("a`b"), "`a\u{02CB}b`");
        assert_eq!(escape_name("a\nb"), "`a b`");
        assert_eq!(escape_name("a\r\nb"), "`a  b`");
        // A literal backslash must NOT be doubled inside a code span: GFM
        // treats span content literally, so `escape_gfm_cell`'s `\` → `\\`
        // rule would render a visible doubled backslash (issue #846). A
        // PHP-style fully-qualified name `Foo\Bar` must keep its single
        // backslash.
        assert_eq!(escape_name("Foo\\Bar"), "`Foo\\Bar`");
        // A backslash adjacent to a pipe stays single (one `\`) while the
        // pipe still gets its `\|` table escape — input chars a \ | b become
        // span content a \ \ | b, i.e. the literal backslash plus the pipe's
        // own `\|`.
        assert_eq!(escape_name("a\\|b"), "`a\\\\|b`");
    }
}
