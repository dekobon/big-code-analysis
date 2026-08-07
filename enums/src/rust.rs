use askama::Template;
use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use crate::common::*;
use crate::languages::*;

const MACROS_DEFINITION_DIR: &str = "data";

#[derive(Debug, Template)]
#[template(path = "rust.rs", escape = "none")]
struct RustTemplate {
    c_name: String,
    names: Vec<(String, bool, String)>,
    // Rust name of the tree-sitter ERROR sentinel, used as the `From<u16>`
    // fallback. It is usually `Error`, but when a grammar already defines an
    // `error` keyword (e.g. Tcl) the sentinel is renamed to `Error2`; the
    // fallback must follow that rename, never the keyword (see issue #954).
    error_sentinel: String,
}

pub fn generate_rust(output: &Path, file_template: &str) -> std::io::Result<()> {
    for lang in Lang::into_enum_iter() {
        let c_name = camel_case(get_language_name(&lang).to_string());
        let file_name = format!("{}.rs", file_template.replace('$', &c_name.to_lowercase()));
        let path = output.join(file_name);
        let mut file = File::create(path)?;

        let rendered = build_rust_template(&lang).render().map_err(render_error)?;
        file.write_all(rendered.as_bytes())?;
    }

    Ok(())
}

fn build_rust_template(lang: &Lang) -> RustTemplate {
    let c_name = camel_case(get_language_name(lang).to_string());
    let names = get_token_names(&get_language(lang), false);

    // `get_token_names` always appends the tree-sitter ERROR sentinel last
    // (pinned by its `get_token_names_appends_error_sentinel_last` test), so the
    // final entry's Rust name is the `From<u16>` fallback. It is renamed to
    // `Error2` when the grammar already owns an `error` keyword (Tcl), and the
    // fallback must follow that rename, not the keyword (#954).
    let error_sentinel = names
        .last()
        .map_or_else(|| "Error".to_string(), |(name, _, _)| name.clone());

    RustTemplate {
        c_name,
        names,
        error_sentinel,
    }
}

#[derive(Debug, Template)]
#[template(path = "c_macros.rs", escape = "none")]
struct CMacrosTemplate {
    u_name: String,
    l_name: String,
    names: Vec<String>,
}

pub fn generate_macros(output: &Path) -> std::io::Result<()> {
    create_macros_file(output, "c_macros", "PREDEFINED_MACROS")?;
    create_macros_file(output, "c_specials", "SPECIALS")
}

fn create_macros_file(output: &Path, filename: &str, u_name: &str) -> std::io::Result<()> {
    let mut macro_file = File::open(Path::new(&format!(
        "{}/{}/{}.txt",
        env!("CARGO_MANIFEST_DIR"),
        MACROS_DEFINITION_DIR,
        filename
    )))?;
    let mut data = Vec::new();
    macro_file.read_to_end(&mut data)?;

    let mut names = Vec::new();
    for tok in data.split(|c| *c == b'\n') {
        let tok = std::str::from_utf8(tok)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?
            .trim();
        if !tok.is_empty() {
            names.push(tok.to_owned());
        }
    }
    // Sort + dedup so the emitted template's `binary_search`-
    // based lookup is correct regardless of input-file ordering
    // and a hand-edit that introduces a duplicate entry doesn't
    // leak two adjacent rows into the slice. The generated
    // `*_is_sorted` test defends against a future refactor that
    // drops the sort.
    names.sort();
    names.dedup();
    let l_name = u_name.to_lowercase();

    let path = output.join(format!("{}.rs", filename));

    let mut file = File::create(&path)?;

    let args = CMacrosTemplate {
        u_name: u_name.to_owned(),
        l_name,
        names,
    };

    file.write_all(args.render().map_err(render_error)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Renders the Rust enum module for one language through the same
    // `build_rust_template` path `generate_rust` uses, so the test
    // exercises the real `error_sentinel` derivation, not a copy.
    fn render_rust(lang: &Lang) -> String {
        build_rust_template(lang)
            .render()
            .expect("RustTemplate renders")
    }

    // #954: the Tcl grammar has an `error` keyword that camel-cases to
    // `Error`, so the tree-sitter ERROR sentinel is renamed to `Error2`.
    // The `From<u16>` fallback must resolve to that renamed sentinel
    // (display "ERROR"), never the keyword variant `Error` (display
    // "error"). A hardcoded `unwrap_or(Self::Error)` template body
    // regressed this for Tcl alone (the lone grammar with such a clash).
    #[test]
    fn tcl_from_u16_fallback_targets_error_sentinel_not_keyword() {
        let rendered = render_rust(&Lang::Tcl);
        assert!(
            rendered.contains("unwrap_or(Self::Error2)"),
            "Tcl From<u16> fallback must point at the ERROR sentinel (Error2), got:\n{rendered}"
        );
        assert!(
            !rendered.contains("unwrap_or(Self::Error)"),
            "Tcl From<u16> fallback must not point at the `error` keyword variant"
        );
    }

    // For a grammar with no `error` keyword the sentinel keeps the plain
    // `Error` name, so the fallback renders `Self::Error` byte-for-byte —
    // i.e. the #954 fix does not churn any other language module.
    #[test]
    fn rust_from_u16_fallback_targets_plain_error_when_unrenamed() {
        let rendered = render_rust(&Lang::Rust);
        assert!(
            rendered.contains("unwrap_or(Self::Error)"),
            "Rust From<u16> fallback should remain Self::Error, got:\n{rendered}"
        );
    }
}
