//! HTML escaping, language palette, and CSS/JS chrome for the report.

use super::*;

/// HTML-escape a string for safe interpolation into element text or
/// double-quoted attribute values. Returns a borrowed `Cow` when the
/// input is already safe so the common case (most metric column names,
/// well-formed paths) allocates nothing.
pub(crate) fn escape_html(s: &str) -> Cow<'_, str> {
    let needs_escape = s
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''));
    if !needs_escape {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

/// Render a note that is written once as Markdown prose and shared with
/// the HTML report: HTML-escape it, then turn each *paired* backtick code
/// span into `<code>…</code>`. Before this the HTML page showed the
/// literal backticks ("sourced from the \`bca.toml\` \`[thresholds]\`
/// table."). An unpaired backtick is left as-is, since there is no span
/// to close.
pub(crate) fn note_html(note: &str) -> Cow<'_, str> {
    let escaped = escape_html(note);
    let ticks = escaped.matches('`').count();
    if ticks == 0 || !ticks.is_multiple_of(2) {
        return escaped;
    }
    let mut out = String::with_capacity(escaped.len() + 16);
    for (i, segment) in escaped.split('`').enumerate() {
        if i > 0 {
            out.push_str(if i.is_multiple_of(2) {
                "</code>"
            } else {
                "<code>"
            });
        }
        out.push_str(segment);
    }
    Cow::Owned(out)
}

/// Slugify a heading basis into an HTML `id`/fragment-safe token:
/// lowercase ASCII, every run of non-`[a-z0-9]` characters collapsed to a
/// single `-`, with leading/trailing hyphens trimmed. Built from the raw
/// language *slug* (`"cpp"`, `"csharp"`) or a section title, never from a
/// display name carrying HTML-special punctuation — so `C++` deep-links to
/// `#cpp`, not a broken `#c++` fragment (issue #622).
///
/// An empty or all-separator basis yields `"section"` so the id is never
/// the empty string (which is not a valid fragment target).
pub(crate) fn slugify(basis: &str) -> String {
    let mut slug = String::with_capacity(basis.len());
    let mut prev_dash = false;
    for ch in basis.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "section".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// One `h2` section in the table of contents plus the `h3` subsections
/// nested under it (in document order). `(display_text, id)` for the
/// section, then a child list of the same for each `h3` that followed it
/// before the next `h2`.
#[derive(Default)]
pub(crate) struct TocSection {
    pub(crate) text: String,
    pub(crate) id: String,
    pub(crate) children: Vec<(String, String)>,
}

/// Emit the table-of-contents `<nav>` linking each collected `h2` section,
/// with its `h3` subsections nested in a collapsible `<details>` so a reader
/// can jump straight to one hotspot table (issue #685). Renders nothing when
/// there are no sections (the empty-walk report). All `text` is already
/// `escape_html`-ed and every `id` is slug-safe ASCII, so each entry resolves
/// to a real anchor on the page.
pub(crate) fn write_toc(out: &mut String, toc: &[TocSection]) {
    if toc.is_empty() {
        return;
    }
    let _ = out.write_str("<nav class=\"toc\" aria-label=\"Sections\">\n<ul>\n");
    for section in toc {
        if section.children.is_empty() {
            let _ = writeln!(
                out,
                "<li><a href=\"#{}\">{}</a></li>",
                section.id, section.text
            );
        } else {
            // A `<details>` keeps the nested list collapsed by default so the
            // nav stays compact, yet expandable — robust across print, mobile,
            // and narrow viewports (issue #685).
            let _ = out.write_str("<li><details>\n<summary>");
            let _ = write!(out, "<a href=\"#{}\">{}</a>", section.id, section.text);
            let _ = out.write_str("</summary>\n<ul>\n");
            for (text, id) in &section.children {
                let _ = writeln!(out, "<li><a href=\"#{id}\">{text}</a></li>");
            }
            let _ = out.write_str("</ul>\n</details></li>\n");
        }
    }
    let _ = out.write_str("</ul>\n</nav>\n");
}

pub(crate) const INLINE_CSS: &str = "\
body{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;\
margin:1.5rem;color:#222;background:#fafafa}\
h1{font-size:1.4rem;margin:0 0 0.5rem}\
h2{font-size:1.15rem;margin:1.5rem 0 0.5rem;\
border-bottom:1px solid #ccc;padding-bottom:0.25rem}\
h3{font-size:1rem;margin:1rem 0 0.4rem;color:#444}\
section{margin-top:2rem}\
section.lang-section{padding:0.5rem 1rem;border-radius:4px;\
border-left:3px solid rgba(127,127,127,0.35)}\
section.lang-section>h2{margin-top:0.25rem}\
section.lang-rust{background:rgba(222,128,82,0.08);border-left-color:rgba(222,128,82,0.55)}\
section.lang-python{background:rgba(58,118,196,0.08);border-left-color:rgba(58,118,196,0.55)}\
section.lang-javascript{background:rgba(229,202,71,0.10);border-left-color:rgba(229,202,71,0.65)}\
section.lang-typescript{background:rgba(46,116,194,0.08);border-left-color:rgba(46,116,194,0.55)}\
section.lang-java{background:rgba(196,69,60,0.08);border-left-color:rgba(196,69,60,0.55)}\
section.lang-kotlin{background:rgba(193,71,167,0.08);border-left-color:rgba(193,71,167,0.55)}\
section.lang-go{background:rgba(0,173,181,0.08);border-left-color:rgba(0,173,181,0.55)}\
section.lang-cpp{background:rgba(120,80,180,0.08);border-left-color:rgba(120,80,180,0.55)}\
section.lang-csharp{background:rgba(83,150,80,0.08);border-left-color:rgba(83,150,80,0.55)}\
section.lang-php{background:rgba(98,113,178,0.08);border-left-color:rgba(98,113,178,0.55)}\
section.lang-bash{background:rgba(96,128,96,0.08);border-left-color:rgba(96,128,96,0.55)}\
section.lang-perl{background:rgba(180,120,60,0.08);border-left-color:rgba(180,120,60,0.55)}\
section.lang-lua{background:rgba(0,86,180,0.08);border-left-color:rgba(0,86,180,0.55)}\
section.lang-tcl{background:rgba(160,90,140,0.08);border-left-color:rgba(160,90,140,0.55)}\
section.lang-irules{background:rgba(200,70,120,0.08);border-left-color:rgba(200,70,120,0.55)}\
section.lang-ruby{background:rgba(204,52,45,0.08);border-left-color:rgba(204,52,45,0.55)}\
section.lang-elixir{background:rgba(110,73,153,0.08);border-left-color:rgba(110,73,153,0.55)}\
section.lang-other{background:rgba(127,127,127,0.06);border-left-color:rgba(127,127,127,0.45)}\
@media (prefers-color-scheme:dark){\
section.lang-rust{background:rgba(222,128,82,0.16)}\
section.lang-python{background:rgba(58,118,196,0.18)}\
section.lang-javascript{background:rgba(229,202,71,0.16)}\
section.lang-typescript{background:rgba(46,116,194,0.18)}\
section.lang-java{background:rgba(196,69,60,0.18)}\
section.lang-kotlin{background:rgba(193,71,167,0.18)}\
section.lang-go{background:rgba(0,173,181,0.18)}\
section.lang-cpp{background:rgba(120,80,180,0.20)}\
section.lang-csharp{background:rgba(83,150,80,0.18)}\
section.lang-php{background:rgba(98,113,178,0.20)}\
section.lang-bash{background:rgba(96,128,96,0.18)}\
section.lang-perl{background:rgba(180,120,60,0.18)}\
section.lang-lua{background:rgba(0,86,180,0.20)}\
section.lang-tcl{background:rgba(160,90,140,0.20)}\
section.lang-irules{background:rgba(200,70,120,0.20)}\
section.lang-ruby{background:rgba(204,52,45,0.18)}\
section.lang-elixir{background:rgba(110,73,153,0.20)}\
section.lang-other{background:rgba(200,200,200,0.10)}\
}\
.summary{font-size:0.9rem;color:#444;margin-bottom:0.5rem}\
.summary strong{color:#222}\
.summary p{margin:0.2rem 0}\
.note{font-size:0.85rem;color:#555;margin:0.4rem 0}\
nav.toc{font-size:0.9rem;margin:0.8rem 0;padding:0.5rem 0.8rem;\
background:#fff;border:1px solid #e5e5e5;border-radius:4px;\
box-shadow:0 1px 2px rgba(0,0,0,0.06)}\
nav.toc ul{margin:0.2rem 0 0.2rem 1.2rem}\
nav.toc li{font-size:0.9rem}\
nav.toc details>summary{cursor:pointer;list-style:revert}\
nav.toc details ul{margin-left:1rem}\
div.global-actionable{margin:0.8rem 0}\
div.global-actionable ul{margin:0.2rem 0 0.2rem 1.2rem}\
footer.provenance{margin-top:2rem;padding-top:0.5rem;border-top:1px solid #ccc;\
font-size:0.8rem;color:#666}\
p.sort-hint{font-size:0.85rem;color:#555;margin:0.4rem 0;font-style:italic}\
@media (prefers-color-scheme:dark){nav.toc{background:#1e1e1e;border-color:#333}}\
details.legend{font-size:0.85rem;color:#444;margin:0.8rem 0}\
details.legend summary{cursor:pointer;font-weight:600;color:#222}\
details.legend dl{margin:0.4rem 0 0 0}\
details.legend dt{font-weight:600;margin-top:0.3rem}\
details.legend dd{margin:0 0 0 1rem;color:#555}\
ul{margin:0.4rem 0 0.4rem 1.2rem;padding:0}\
li{margin:0.15rem 0;font-size:0.9rem}\
div.table-wrap{overflow-x:auto;margin-bottom:0.5rem}\
table.hotspot{border-collapse:collapse;width:100%;font-size:0.85rem;\
background:#fff;box-shadow:0 1px 2px rgba(0,0,0,0.06);margin-bottom:0.5rem}\
table.hotspot th,table.hotspot td{padding:0.4rem 0.6rem;\
border-bottom:1px solid #e5e5e5;text-align:left;white-space:nowrap}\
table.hotspot th{background:#f0f0f0;user-select:none;font-weight:600;\
padding:0}\
table.hotspot th button.sort{font:inherit;color:inherit;background:none;\
border:0;padding:0.4rem 0.6rem;cursor:pointer;text-align:inherit}\
table.hotspot th button.sort:hover{background:#e5e5e5}\
table.hotspot th button.sort:focus-visible{outline:2px solid #3d82cc;\
outline-offset:-2px}\
table.hotspot th[aria-sort=ascending] .sort-icon::after{content:\" \\2191\"}\
table.hotspot th[aria-sort=descending] .sort-icon::after{content:\" \\2193\"}\
table.hotspot th a.doc{padding:0.4rem 0.6rem 0.4rem 0;font-size:0.85em;\
color:#555}\
table.hotspot th a.doc:hover,table.hotspot th a.doc:focus-visible{color:#222}\
table.hotspot tr:nth-child(even) td{background:#fafafa}\
table.hotspot th[data-numeric=\"1\"]{text-align:right}\
table.hotspot td.numeric{text-align:right;font-variant-numeric:tabular-nums}\
table.hotspot td.risk-heat-0,table.hotspot td.risk-heat-1,\
table.hotspot td.risk-heat-2,table.hotspot td.risk-heat-3,\
table.hotspot td.risk-heat-4{font-weight:600}\
table.hotspot tr:nth-child(even) td.risk-heat-0,table.hotspot td.risk-heat-0\
{background:#f4b8b0;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-1,table.hotspot td.risk-heat-1\
{background:#f6cba3;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-2,table.hotspot td.risk-heat-2\
{background:#f1e3a3;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-3,table.hotspot td.risk-heat-3\
{background:#cfe6b0;color:#222}\
table.hotspot tr:nth-child(even) td.risk-heat-4,table.hotspot td.risk-heat-4\
{background:#bfe3c0;color:#222}\
@media (prefers-color-scheme:dark){\
body{color:#e0e0e0;background:#121212}\
a{color:#6aa3e0}\
h2{border-bottom-color:#333}\
h3{color:#bbb}\
.summary{color:#bbb}\
.summary strong{color:#e0e0e0}\
.note{color:#999}\
footer.provenance{color:#888;border-top-color:#333}\
p.sort-hint{color:#999}\
details.legend{color:#bbb}\
details.legend summary{color:#e0e0e0}\
details.legend dd{color:#999}\
table.hotspot{background:#1e1e1e}\
table.hotspot th,table.hotspot td{border-bottom-color:#333}\
table.hotspot th{background:#2a2a2a}\
table.hotspot th button.sort:hover{background:#383838}\
table.hotspot th a.doc{color:#aaa}\
table.hotspot th a.doc:hover,table.hotspot th a.doc:focus-visible{color:#e0e0e0}\
table.hotspot tr:nth-child(even) td{background:#232323}\
table.hotspot tr:nth-child(even) td.risk-heat-0,table.hotspot td.risk-heat-0\
{background:#7a1f17;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-1,table.hotspot td.risk-heat-1\
{background:#7a4a12;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-2,table.hotspot td.risk-heat-2\
{background:#6b5e12;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-3,table.hotspot td.risk-heat-3\
{background:#33591f;color:#f0f0f0}\
table.hotspot tr:nth-child(even) td.risk-heat-4,table.hotspot td.risk-heat-4\
{background:#1f5a2e;color:#f0f0f0}\
}\
";

/// `LANG::name()` -> CSS class suffix table. The renderer uses
/// every entry here; `language_palette_classes_have_css` walks
/// [`INLINE_CSS`] to confirm both the light and dark rules exist for
/// each suffix, so adding a row without the matching CSS fails the
/// suite. `"other"` is the neutral fallback for any name not listed.
///
/// Names match production output of [`big_code_analysis::LANG::name`]
/// (see `big-code-analysis-ast/src/langs.rs`), which since #540 is
/// the canonical lowercase slug for every variant (`"cpp"`,
/// `"csharp"`, `"tsx"`). `LANG::Tsx`
/// (`"tsx"`) reuses the `"typescript"` tint (it is TypeScript + JSX),
/// and the Mozilla-fork `"mozjs"` reuses the `"javascript"` tint (it
/// is JavaScript, just a different grammar), and likewise `"mozcpp"`
/// reuses the `"cpp"` tint — none needs its own CSS rule.
pub(crate) const LANGUAGE_PALETTE: &[(&str, &str)] = &[
    ("rust", "rust"),
    ("python", "python"),
    ("javascript", "javascript"),
    ("mozjs", "javascript"),
    ("typescript", "typescript"),
    ("tsx", "typescript"),
    ("java", "java"),
    ("kotlin", "kotlin"),
    ("go", "go"),
    ("cpp", "cpp"),
    ("c", "cpp"),
    ("mozcpp", "cpp"),
    ("csharp", "csharp"),
    ("php", "php"),
    ("bash", "bash"),
    ("perl", "perl"),
    ("lua", "lua"),
    ("tcl", "tcl"),
    ("irules", "irules"),
    ("ruby", "ruby"),
    ("elixir", "elixir"),
];

pub(crate) fn language_palette_slug(lang_name: &str) -> &'static str {
    LANGUAGE_PALETTE
        .iter()
        .find_map(|&(name, slug)| (name == lang_name).then_some(slug))
        .unwrap_or("other")
}

pub(crate) const INLINE_JS: &str = "\
(function(){\
function num(s){return s===''?NaN:parseFloat(s.replace(/,/g,''));}\
document.querySelectorAll('table.hotspot').forEach(function(table){\
var headers=table.querySelectorAll('thead th');\
headers.forEach(function(th,idx){\
var b=th.querySelector('button.sort');\
if(!b)return;\
b.addEventListener('click',function(){sort(table,idx,th);});\
});\
});\
function sort(tbl,idx,th){\
var tbody=tbl.tBodies[0];\
if(!tbody)return;\
var numeric=th.dataset.numeric==='1';\
var dir=th.getAttribute('aria-sort')==='ascending'?'descending':'ascending';\
tbl.querySelectorAll('thead th').forEach(function(h){h.removeAttribute('aria-sort');});\
th.setAttribute('aria-sort',dir);\
var sign=dir==='ascending'?1:-1;\
var keyed=Array.prototype.map.call(tbody.rows,function(r){\
var t=r.cells[idx].textContent;\
return {r:r,k:numeric?num(t):t};\
});\
keyed.sort(function(a,b){\
if(numeric){\
var ae=isNaN(a.k);\
var be=isNaN(b.k);\
if(ae&&be)return 0;\
if(ae)return 1;\
if(be)return -1;\
if(a.k<b.k)return -1*sign;\
if(a.k>b.k)return 1*sign;\
return 0;\
}\
return a.k.localeCompare(b.k)*sign;\
});\
keyed.forEach(function(e){tbody.appendChild(e.r);});\
}\
})();\
";

/// One discoverability hint near a page's first table: the columns are
/// sortable, which is otherwise invisible until a reader happens to
/// activate a header (issue #622). Lives beside [`INLINE_JS`], whose
/// behaviour it describes, and is shared by the aggregated report and the
/// standalone change-history page.
pub(crate) fn write_sort_hint(out: &mut String) {
    let _ = out.write_str(
        "<p class=\"sort-hint\">Click a column header's label \u{2014} or Tab to it and press Enter \u{2014} to sort any table.</p>\n",
    );
}

/// Write the shared HTML preamble (doctype, head with the inline CSS, and
/// the opening `<h1>`). `title` fills the `<title>` element and `heading`
/// the `<h1>`; both are escaped. Shared by the AST report and the VCS
/// report (`crate::vcs_report`) so the two pages carry identical styling
/// and the sortable-table CSS.
pub(crate) fn write_html_head(out: &mut String, title: &str, heading: &str) {
    let _ = out.write_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    let _ = out.write_str("<meta charset=\"utf-8\">\n");
    // Without a viewport meta, mobile browsers render the page at desktop
    // width zoomed out to unreadable; the report is published to GitHub Pages
    // where mobile and split-screen reading are normal (issue #686).
    let _ =
        out.write_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(
        out,
        "<title>{} \u{2014} big-code-analysis</title>",
        escape_html(title)
    );
    let _ = writeln!(out, "<style>{INLINE_CSS}</style>");
    let _ = out.write_str("</head>\n<body>\n");
    let _ = writeln!(out, "<h1>{}</h1>", escape_html(heading));
}

pub(crate) fn write_html_tail(out: &mut String) {
    let _ = writeln!(out, "<script>{INLINE_JS}</script>");
    let _ = out.write_str("</body>\n</html>\n");
}
