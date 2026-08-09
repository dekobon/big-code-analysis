use askama::Template;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::common::{camel_case, get_token_names, render_error};
use crate::languages::{Lang, get_language, get_language_name};

#[derive(Debug, Template)]
#[template(path = "json.json", escape = "none")]
struct JsonTemplate {
    names: Vec<(String, bool, String)>,
}

/// Writes one JSON token-kind table per [`Lang`] into `output`.
///
/// `file_template` is the file stem with `$` standing in for the
/// lowercased language name, so `language_$` yields
/// `language_rust.json`.
///
/// # Errors
///
/// Returns the underlying [`std::io::Error`] when a destination file
/// cannot be created or written, or when a template fails to render.
pub fn generate_json(output: &Path, file_template: &str) -> std::io::Result<()> {
    for lang in Lang::into_enum_iter() {
        let language = get_language(&lang);
        let c_name = camel_case(get_language_name(&lang));

        let file_name = format!(
            "{}.json",
            file_template.replace('$', &c_name.to_lowercase())
        );
        let path = output.join(file_name);
        let mut file = File::create(path)?;

        let names = get_token_names(&language);

        let args = JsonTemplate { names };

        file.write_all(args.render().map_err(render_error)?.as_bytes())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::sanitize_string;

    // Minimal JSON string-literal unescaper, sufficient for the escape
    // sequences `sanitize_string` can emit (`\"`, `\\`, `\t`, `\n`, `\r`).
    // A round-trip through this parser proves the generated JSON decodes
    // back to the original token text; the over-escaping bug (issue #862)
    // makes it decode to a value carrying a spurious extra backslash.
    fn json_unescape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    // A trailing lone backslash has nothing to escape,
                    // so it stands for itself — same body as `\\`.
                    Some('\\') | None => out.push('\\'),
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    // Render a single ["name", "ts_name"] row exactly as `generate_json`
    // would, then extract the quoted ts_name back out of the rendered JSON.
    fn rendered_ts_name(token: &str) -> String {
        let ts_name = sanitize_string(token);
        let args = JsonTemplate {
            names: vec![("Tok".to_string(), false, ts_name)],
        };
        let rendered = args.render().expect("template renders");
        // The row is `["Tok", "<escaped>"]`; capture the second literal.
        let start = rendered.find("\"Tok\"").expect("name literal present");
        let after = &rendered[start + "\"Tok\"".len()..];
        let open = after.find('"').expect("ts_name opening quote");
        let body = &after[open + 1..];
        let close = body.find("\"]").expect("ts_name closing quote");
        body[..close].to_string()
    }

    // A token that is a single double-quote must serialize to the
    // single-backslash form `\"`, which JSON decodes back to `"`. Under the
    // double-backslash bug it serialized to `\\\"`, decoding to `\"`.
    #[test]
    fn json_quote_token_round_trips() {
        let escaped = rendered_ts_name("\"");
        assert_eq!(
            escaped, "\\\"",
            "quote token must use single-backslash form"
        );
        assert_eq!(
            json_unescape(&escaped),
            "\"",
            "must decode to the original quote"
        );
    }

    // A backslash token decodes back to a single backslash.
    #[test]
    fn json_backslash_token_round_trips() {
        let escaped = rendered_ts_name("\\");
        assert_eq!(escaped, "\\\\");
        assert_eq!(json_unescape(&escaped), "\\");
    }

    // A tab token must serialize to the two-char `\t` escape (decoding to a
    // real tab), not the literal-backslash `\\t` the bug produced.
    #[test]
    fn json_tab_token_round_trips() {
        let escaped = rendered_ts_name("\t");
        assert_eq!(escaped, "\\t");
        assert_eq!(json_unescape(&escaped), "\t");
    }

    // Mixed special characters all decode back to the original token text.
    #[test]
    fn json_mixed_specials_round_trip() {
        let token = "a\"b\\c\td\ne\rf";
        let escaped = rendered_ts_name(token);
        assert_eq!(json_unescape(&escaped), token);
    }
}
