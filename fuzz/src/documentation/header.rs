use big_code_analysis::LANG;
use big_code_analysis_ast::documentation::{CollectionError, DocumentationOptions, Profile};

use super::Skip;

// This table is a corpus format, independent of LANG discriminants and iteration order.
pub(super) const LANGUAGES: [LANG; 25] = [
    LANG::Rust,
    LANG::Python,
    LANG::Java,
    LANG::Kotlin,
    LANG::Groovy,
    LANG::Csharp,
    LANG::Php,
    LANG::Javascript,
    LANG::Mozjs,
    LANG::Typescript,
    LANG::Tsx,
    LANG::C,
    LANG::Cpp,
    LANG::Mozcpp,
    LANG::Objc,
    LANG::Go,
    LANG::Ruby,
    LANG::Lua,
    LANG::Perl,
    LANG::Elixir,
    LANG::Bash,
    LANG::Tcl,
    LANG::Irules,
    LANG::Ccomment,
    LANG::Preproc,
];

const PROFILES: [Profile; 22] = [
    Profile::Default,
    Profile::Rust,
    Profile::Python,
    Profile::Javadoc,
    Profile::Kdoc,
    Profile::Groovydoc,
    Profile::CsharpXml,
    Profile::BcaPhpSource,
    Profile::PhpDocumentor,
    Profile::Jsdoc,
    Profile::BcaTypescriptSource,
    Profile::Doxygen,
    Profile::GoDoc,
    Profile::Yard,
    Profile::Rdoc,
    Profile::Ldoc,
    Profile::Pod,
    Profile::Elixir,
    Profile::Shdoc,
    Profile::Ruff,
    Profile::Irules,
    Profile::Helper,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Raw,
    IncludedRanges,
    CancelledScope,
    ActiveScope,
}

pub(super) struct Input<'a> {
    pub language: LANG,
    pub mode: Mode,
    pub repeat: bool,
    pub identity: bool,
    pub options: DocumentationOptions,
    pub source: &'a [u8],
}

impl<'a> Input<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Skip> {
        let [tag, mode, flags, source @ ..] = bytes else {
            return Err(Skip::ShortHeader);
        };
        let language = *LANGUAGES
            .get(usize::from(*tag))
            .ok_or(Skip::UnknownLanguage)?;
        if mode & 0xf0 != 0 || flags & 0x80 != 0 {
            return Err(Skip::ReservedBits);
        }
        let profile = *PROFILES
            .get(usize::from(flags & 0x1f))
            .ok_or(Skip::UnknownProfile)?;
        Ok(Self {
            language,
            mode: match (mode >> 2) & 3 {
                0 => Mode::Raw,
                1 => Mode::IncludedRanges,
                2 => Mode::CancelledScope,
                _ => Mode::ActiveScope,
            },
            repeat: mode & 1 != 0,
            identity: mode & 2 != 0,
            options: DocumentationOptions {
                profile,
                retain_unbound: flags & 0x20 != 0,
                retain_directives: flags & 0x40 != 0,
            },
            source,
        })
    }
}

pub(super) fn expected_profile(
    language: LANG,
    requested: Profile,
) -> Result<Profile, CollectionError> {
    let default = match language {
        LANG::Rust => Profile::Rust,
        LANG::Python => Profile::Python,
        LANG::Java => Profile::Javadoc,
        LANG::Kotlin => Profile::Kdoc,
        LANG::Groovy => Profile::Groovydoc,
        LANG::Csharp => Profile::CsharpXml,
        LANG::Php => Profile::BcaPhpSource,
        LANG::Javascript | LANG::Mozjs => Profile::Jsdoc,
        LANG::Typescript | LANG::Tsx => Profile::BcaTypescriptSource,
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => Profile::Doxygen,
        LANG::Go => Profile::GoDoc,
        LANG::Ruby => Profile::Yard,
        LANG::Lua => Profile::Ldoc,
        LANG::Perl => Profile::Pod,
        LANG::Elixir => Profile::Elixir,
        LANG::Bash => Profile::Shdoc,
        LANG::Tcl => Profile::Ruff,
        LANG::Irules => Profile::Irules,
        LANG::Ccomment | LANG::Preproc => Profile::Helper,
    };
    let profile = if requested == Profile::Default {
        default
    } else {
        requested
    };
    let alternate = matches!(
        (language, profile),
        (LANG::Php, Profile::PhpDocumentor) | (LANG::Ruby, Profile::Rdoc)
    );
    if profile != default && !alternate {
        Err(CollectionError::UnsupportedProfile { language, profile })
    } else if alternate
        || matches!(
            language,
            LANG::Python | LANG::Elixir | LANG::Bash | LANG::Irules
        )
    {
        Err(CollectionError::AdapterUnavailable { language, profile })
    } else {
        Ok(profile)
    }
}
