//! Error type of the file-backed [`Ast::from_path`][crate::Ast::from_path].

use crate::MetricsError;

/// Error returned by [`Ast::from_path`][crate::Ast::from_path].
///
/// `from_path` reads, language-detects, and parses a file in one call, so it
/// can fail in more ways than the in-memory [`Ast::parse`][crate::Ast::parse]
/// (which only reports [`MetricsError`]). Unlike [`analyze`][crate::analyze],
/// `from_path` does not silently skip files: every reason it cannot produce a
/// tree surfaces as a distinct variant so the caller — who asked for *this*
/// file's tree — learns why.
///
/// The enum is `#[non_exhaustive]`; match with a trailing `_` arm to stay
/// forward-compatible.
#[non_exhaustive]
#[derive(Debug)]
pub enum FromPathError {
    /// The file could not be read (a genuine I/O fault: missing file,
    /// permission denied, hardware error). Carries the underlying
    /// [`std::io::Error`].
    Io(std::io::Error),
    /// The path is not valid UTF-8. The path doubles as the resulting
    /// [`FuncSpace`][crate::FuncSpace] name (an identifier used as a map key
    /// and in JSON output), so a lossy conversion is rejected rather than
    /// silently corrupting correlation — mirroring `analyze`'s strict
    /// default.
    NonUtf8Path,
    /// The file is empty, too small, binary, or encoded in an unsupported
    /// encoding (UTF-16, invalid UTF-8) — the same files
    /// [`analyze`][crate::analyze] skips. `from_path` reuses the library's
    /// text reader for byte-exact metric parity with `analyze`, so these
    /// inputs cannot yield a tree.
    Unreadable,
    /// No language is registered for the path (unknown extension and no
    /// recognizable shebang / mode line).
    UnknownLanguage,
    /// The detected language's per-language Cargo feature is not enabled in
    /// this build (carries the [`MetricsError::LanguageDisabled`] raised by
    /// [`Ast::parse`][crate::Ast::parse]).
    Parse(MetricsError),
}

impl std::fmt::Display for FromPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read file: {e}"),
            Self::NonUtf8Path => f.write_str("path is not valid UTF-8"),
            Self::Unreadable => {
                f.write_str("file is empty, binary, or not valid UTF-8 source text")
            }
            Self::UnknownLanguage => f.write_str("no language is registered for this path"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FromPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<MetricsError> for FromPathError {
    fn from(e: MetricsError) -> Self {
        Self::Parse(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LANG;
    use std::error::Error as _;
    use std::io::{Error as IoError, ErrorKind};

    // `FromPathError`'s `Display` / `source` impls and its `From`
    // conversion are part of the stable error surface but are only
    // reached on `from_path` failure paths that no other test drives.
    // Pin the message shape (substring, not exact wording) and the
    // `source` chaining contract, which is what `?`-propagating callers
    // and `anyhow`-style reporters rely on.

    #[test]
    fn from_path_error_display_covers_every_variant() {
        let io = FromPathError::Io(IoError::new(ErrorKind::PermissionDenied, "denied"));
        assert!(io.to_string().contains("could not read file"));
        assert!(io.to_string().contains("denied"));

        assert!(
            FromPathError::NonUtf8Path
                .to_string()
                .contains("not valid UTF-8")
        );
        assert!(
            FromPathError::Unreadable
                .to_string()
                .contains("empty, binary")
        );
        assert!(
            FromPathError::UnknownLanguage
                .to_string()
                .contains("no language is registered")
        );

        // `Parse` delegates to the wrapped `MetricsError`'s `Display`.
        let parse = FromPathError::Parse(MetricsError::LanguageDisabled(LANG::Rust));
        assert_eq!(
            parse.to_string(),
            MetricsError::LanguageDisabled(LANG::Rust).to_string()
        );
    }

    #[test]
    fn from_path_error_source_chains_only_for_wrapping_variants() {
        // `Io` and `Parse` wrap an underlying error and must expose it;
        // the leaf variants must report no source.
        let io = FromPathError::Io(IoError::new(ErrorKind::NotFound, "missing"));
        assert!(io.source().is_some(), "Io must chain to the io::Error");

        let parse = FromPathError::Parse(MetricsError::EmptyRoot);
        assert!(parse.source().is_some(), "Parse must chain to MetricsError");

        for leaf in [
            FromPathError::NonUtf8Path,
            FromPathError::Unreadable,
            FromPathError::UnknownLanguage,
        ] {
            assert!(leaf.source().is_none(), "{leaf:?} must report no source");
        }
    }

    #[test]
    fn metrics_error_converts_into_parse_variant() {
        let converted: FromPathError = MetricsError::LanguageDisabled(LANG::Cpp).into();
        assert!(
            matches!(
                converted,
                FromPathError::Parse(MetricsError::LanguageDisabled(LANG::Cpp))
            ),
            "From<MetricsError> must wrap into Parse"
        );
    }
}
