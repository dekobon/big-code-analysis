//! Inherent impl block for [`super::Source`].
//!
//! Split out of `spaces.rs` to keep that module focused on the public
//! API type definitions. The block is moved verbatim; method
//! resolution is by type, so `crate::spaces::Source`'s methods resolve
//! unchanged.

use super::*;

impl<'a> Source<'a> {
    /// Build a `Source` for `lang` and `code` with no name and no
    /// preprocessor inputs. Chain `with_*` setters to attach a
    /// display name or preprocessor results.
    ///
    /// `Source` is `#[non_exhaustive]`, so external callers cannot
    /// use struct-literal syntax — this constructor plus the
    /// builder setters are the supported construction path.
    #[inline]
    #[must_use]
    pub fn new(lang: LANG, code: &'a [u8]) -> Self {
        Self {
            lang,
            code: Cow::Borrowed(code),
            name: None,
            preproc_path: None,
            preproc: None,
        }
    }

    /// Build a `Source` that owns `code`, so [`Ast::parse`] moves the
    /// buffer into the parser instead of copying it. Prefer this over
    /// [`Source::new`] when you already hold an owned `Vec<u8>` (e.g. a
    /// just-read file), which saves one full-buffer copy per parse.
    #[inline]
    #[must_use]
    pub fn from_bytes(lang: LANG, code: Vec<u8>) -> Self {
        Self {
            lang,
            code: Cow::Owned(code),
            name: None,
            preproc_path: None,
            preproc: None,
        }
    }

    /// Builder-style setter for `Source::name`.
    #[inline]
    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// Builder-style setter for `Source::preproc_path`.
    #[inline]
    #[must_use]
    pub fn with_preproc_path(mut self, preproc_path: Option<&'a Path>) -> Self {
        self.preproc_path = preproc_path;
        self
    }

    /// Builder-style setter for `Source::preproc`.
    #[inline]
    #[must_use]
    pub fn with_preproc(mut self, preproc: Option<Arc<PreprocResults>>) -> Self {
        self.preproc = preproc;
        self
    }
}
