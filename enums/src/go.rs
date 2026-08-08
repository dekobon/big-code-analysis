use askama::Template;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::common::{camel_case, get_token_names, render_error};
use crate::languages::{Lang, get_language, get_language_name};

#[derive(Debug, Template)]
#[template(path = "go.go", escape = "none")]
struct GoTemplate {
    c_name: String,
    names: Vec<(String, bool, String, String)>,
}

/// Writes one Go token-kind package per [`Lang`] into `output`.
///
/// `file_template` is the file stem with `$` standing in for the
/// lowercased language name, so `language_$` yields `language_rust.go`.
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] when a destination file
/// cannot be created or written, or when a template fails to render.
pub fn generate_go(output: &Path, file_template: &str) -> std::io::Result<()> {
    for lang in Lang::into_enum_iter() {
        let language = get_language(&lang);
        let c_name = camel_case(get_language_name(&lang));

        let file_name = format!("{}.go", file_template.replace('$', &c_name.to_lowercase()));
        let path = output.join(file_name);
        let mut file = File::create(path)?;

        let names = get_token_names(&language, false);
        // `unwrap_or(0)` is the identity, not a swallowed error: with no
        // names the `map` below yields nothing, so the padding width is
        // never read. The empty case is unreachable — `get_token_names`
        // walks `0..node_kind_count()` and every real grammar has at
        // least the ERROR sentinel — so this is correct-by-construction
        // hardening, not a fixed crash. It is converted rather than left
        // alone because an `unwrap()` states no invariant at all (#1227).
        let max_len = names.iter().map(|x| x.0.len()).max().unwrap_or(0);
        let names: Vec<_> = names
            .into_iter()
            .map(|(n, d, t)| {
                let padded = format!("{n: <max_len$}");
                (n, d, t, padded)
            })
            .collect();

        let args = GoTemplate { c_name, names };

        file.write_all(args.render().map_err(render_error)?.as_bytes())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Issue #863: the generated `FromString` signature must reference Go's
    // built-in `string` type, not the undefined identifier `String`, or the
    // emitted package fails to compile with `undefined: String`.
    #[test]
    fn from_string_uses_lowercase_go_string_type() {
        let template = GoTemplate {
            c_name: "Rust".to_string(),
            names: vec![(
                "Identifier".to_string(),
                false,
                "identifier".to_string(),
                "Identifier".to_string(),
            )],
        };
        let rendered = template.render().expect("GoTemplate renders");
        assert!(
            rendered.contains("func FromString(str string)"),
            "FromString must use Go's built-in `string` type, got:\n{rendered}"
        );
        assert!(
            !rendered.contains("str String"),
            "FromString must not reference the undefined `String` type, got:\n{rendered}"
        );
    }
}
