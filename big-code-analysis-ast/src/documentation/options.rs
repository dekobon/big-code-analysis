//! Explicit attachment profiles and collection outcomes.

use super::SourceError;
use crate::LANG;

/// Caller metadata used by documentation rules, never a filesystem credential.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct DocumentationContext {
    /// Explicit logical filename. Display labels are not substituted for it.
    pub logical_filename: Option<String>,
}

impl DocumentationContext {
    /// Set the logical name used by source attachment rules, without filesystem access.
    #[must_use]
    pub fn with_logical_filename(mut self, filename: impl Into<String>) -> Self {
        self.logical_filename = Some(filename.into());
        self
    }
}

/// One named source attachment convention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Profile {
    /// The language's documented default convention.
    #[default]
    Default,
    /// Compiler documentation attributes and comments.
    Rust,
    /// PEP 257/258 and Sphinx source documentation.
    Python,
    /// JDK 25 source attachment with BCA local cleanup.
    Javadoc,
    /// Kotlin documentation comments.
    Kdoc,
    /// Groovy documentation comments.
    Groovydoc,
    /// C# XML source documentation.
    CsharpXml,
    /// Declaration-before-file PHP source selection.
    BcaPhpSource,
    /// File-before-declaration phpDocumentor selection.
    PhpDocumentor,
    /// JavaScript source documentation.
    Jsdoc,
    /// TypeScript attachment with TypeDoc structural-file precedence.
    BcaTypescriptSource,
    /// Doxygen source documentation.
    Doxygen,
    /// go/doc source groups.
    GoDoc,
    /// Ruby YARD documentation.
    Yard,
    /// Ruby RDoc documentation.
    Rdoc,
    /// Lua LDoc documentation.
    Ldoc,
    /// Perl POD file documentation.
    Pod,
    /// Compiler source documentation attributes.
    Elixir,
    /// Bash shdoc comments.
    Shdoc,
    /// Static Ruff! 3.0.4 documentation.
    Ruff,
    /// BCA leading/body iRules convention.
    Irules,
    /// A helper grammar with no declaration model.
    Helper,
}

impl Profile {
    /// Stable convention identifier used in associations and errors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        // bca: suppress(cyclomatic) -- exhaustive profile identifier table.
        match self {
            Self::Default => "Default",
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::Javadoc => "Javadoc",
            Self::Kdoc => "KDoc",
            Self::Groovydoc => "Groovydoc",
            Self::CsharpXml => "CsharpXml",
            Self::BcaPhpSource => "BcaPhpSource",
            Self::PhpDocumentor => "PhpDocumentor",
            Self::Jsdoc => "JSDoc",
            Self::BcaTypescriptSource => "BcaTypescriptSource",
            Self::Doxygen => "Doxygen",
            Self::GoDoc => "GoDoc",
            Self::Yard => "YARD",
            Self::Rdoc => "RDoc",
            Self::Ldoc => "LDoc",
            Self::Pod => "POD",
            Self::Elixir => "Elixir",
            Self::Shdoc => "shdoc",
            Self::Ruff => "Ruff",
            Self::Irules => "Irules",
            Self::Helper => "Helper",
        }
    }

    pub(super) fn resolve(self, language: LANG) -> Result<Self, CollectionError> {
        // bca: suppress(cyclomatic, halstead) -- exhaustive language/profile compatibility and deferred-adapter table.
        let default = match language {
            LANG::Rust => Self::Rust,
            LANG::Python => Self::Python,
            LANG::Java => Self::Javadoc,
            LANG::Kotlin => Self::Kdoc,
            LANG::Groovy => Self::Groovydoc,
            LANG::Csharp => Self::CsharpXml,
            LANG::Php => Self::BcaPhpSource,
            LANG::Javascript | LANG::Mozjs => Self::Jsdoc,
            LANG::Typescript | LANG::Tsx => Self::BcaTypescriptSource,
            LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => Self::Doxygen,
            LANG::Go => Self::GoDoc,
            LANG::Ruby => Self::Yard,
            LANG::Lua => Self::Ldoc,
            LANG::Perl => Self::Pod,
            LANG::Elixir => Self::Elixir,
            LANG::Bash => Self::Shdoc,
            LANG::Tcl => Self::Ruff,
            LANG::Irules => Self::Irules,
            LANG::Ccomment | LANG::Preproc => Self::Helper,
        };
        let profile = if self == Self::Default { default } else { self };
        let alternate = matches!(
            (language, profile),
            (LANG::Php, Self::PhpDocumentor) | (LANG::Ruby, Self::Rdoc)
        );
        if profile != default && !alternate {
            return Err(CollectionError::UnsupportedProfile { language, profile });
        }
        if alternate || matches!(profile, Self::Python | Self::Shdoc | Self::Irules) {
            return Err(CollectionError::AdapterUnavailable { language, profile });
        }
        Ok(profile)
    }
}

/// Immutable collection choices; pending assessments are always retained.
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct DocumentationOptions {
    /// One named attachment convention.
    pub profile: Profile,
    /// Preserve superseded and unattached candidate details.
    pub retain_unbound: bool,
    /// Preserve recognized non-prose directive occurrences.
    pub retain_directives: bool,
}

impl DocumentationOptions {
    /// Select a named source attachment convention.
    #[must_use]
    pub fn with_profile(mut self, profile: Profile) -> Self {
        self.profile = profile;
        self
    }

    /// Retain superseded and unattached source candidates.
    #[must_use]
    pub fn with_unbound_evidence(mut self, retain: bool) -> Self {
        self.retain_unbound = retain;
        self
    }

    /// Retain recognized non-prose directives.
    #[must_use]
    pub fn with_directive_evidence(mut self, retain: bool) -> Self {
        self.retain_directives = retain;
        self
    }
}

/// Failure before a complete immutable index can be returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollectionError {
    /// Active collection cancellation or retained typed parse abort.
    Cancelled,
    /// The grammar feature is disabled.
    LanguageDisabled(LANG),
    /// The named profile does not apply to this language.
    #[non_exhaustive]
    UnsupportedProfile {
        /// Requested source language.
        language: LANG,
        /// Requested profile.
        profile: Profile,
    },
    /// The recognized adapter/profile has not been implemented.
    #[non_exhaustive]
    AdapterUnavailable {
        /// Requested source language.
        language: LANG,
        /// Recognized profile.
        profile: Profile,
    },
    /// The supplied tree uses a different pinned grammar.
    GrammarMismatch,
    /// A root or visited node has invalid source-buffer bounds.
    SourceBounds,
    /// A source operation failed; cancellation has its own top-level variant.
    Source(SourceError),
}

impl CollectionError {
    /// Construct a profile/language incompatibility without depending on the
    /// fields of this extensible error variant.
    #[must_use]
    #[doc(hidden)]
    pub const fn unsupported_profile(language: LANG, profile: Profile) -> Self {
        Self::UnsupportedProfile { language, profile }
    }

    /// Construct an adapter availability result without depending on the
    /// fields of this extensible error variant.
    #[must_use]
    #[doc(hidden)]
    pub const fn adapter_unavailable(language: LANG, profile: Profile) -> Self {
        Self::AdapterUnavailable { language, profile }
    }
}

impl From<SourceError> for CollectionError {
    fn from(error: SourceError) -> Self {
        match error {
            SourceError::Cancelled => Self::Cancelled,
            error => Self::Source(error),
        }
    }
}

impl std::fmt::Display for CollectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("documentation collection cancelled"),
            Self::LanguageDisabled(language) => {
                write!(f, "documentation language disabled: {language}")
            }
            Self::UnsupportedProfile { language, profile } => {
                write!(
                    f,
                    "documentation profile {} is unsupported for {language}",
                    profile.name()
                )
            }
            Self::AdapterUnavailable { language, profile } => {
                write!(
                    f,
                    "documentation adapter {} is unavailable for {language}",
                    profile.name()
                )
            }
            Self::GrammarMismatch => f.write_str("documentation grammar mismatch"),
            Self::SourceBounds => f.write_str("documentation source bounds are invalid"),
            Self::Source(error) => write!(f, "documentation source: {error}"),
        }
    }
}

impl std::error::Error for CollectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            _ => None,
        }
    }
}
