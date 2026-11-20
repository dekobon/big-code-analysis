//! Bounded source-documentation properties shared by raw and tagged targets.

mod header;
mod identity;
mod maps;
mod observation;
mod properties;
mod repeat;
#[cfg(test)]
mod tests;

use big_code_analysis::{Ast, LANG};
use big_code_analysis_ast::{
    AnyParser,
    cancel::ParseCancel,
    documentation::{
        Capability, CollectionError, DocumentationContext, DocumentationIndex,
        DocumentationOptions, collect,
    },
};
use header::{Input, Mode, expected_profile};
use observation::Event;

/// Maximum exact source length inspected by the additional documentation operation.
pub const MAX_SOURCE: usize = 32 * 1024;
pub(super) const MAX_QUERIES: usize = 8;
pub(super) const MAX_STRUCTURE: usize = 256;

/// A classified input that intentionally does not reach documentation collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Fewer than three header bytes.
    ShortHeader,
    /// Language tag outside the version-one table.
    UnknownLanguage,
    /// Reserved operation or version bits are set.
    ReservedBits,
    /// Profile selector outside the version-one table.
    UnknownProfile,
    /// The exact source exceeds the harness budget.
    Oversize,
    /// Included-range input has no included bytes or too many ranges.
    IncludedRangeLimit,
}

/// What the checker actually executed, distinct from an empty extraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Header or resource rejection before parsing.
    Skipped(Skip),
    /// A recognized unimplemented adapter or incompatible profile was rejected.
    Rejected(CollectionError),
    /// A helper grammar explicitly has no source declaration model.
    Helper,
    /// An implemented adapter completed all selected properties.
    Checked,
}

pub(super) type Check = Result<(), &'static str>;

struct Parsed<'s> {
    language: LANG,
    tree: &'s tree_sitter::Tree,
    source: &'s [u8],
    typed: Option<&'s AnyParser>,
}

impl<'s> Parsed<'s> {
    fn collect(
        &self,
        options: DocumentationOptions,
    ) -> Result<DocumentationIndex<'s>, CollectionError> {
        let context = DocumentationContext::default();
        let result = if let Some(parser) = self.typed {
            parser.documentation(&context, &options)
        } else {
            collect(self.language, self.tree, self.source, &context, &options)
        };
        observation::record(Event::Collection(
            result.as_ref().map(|_| ()).map_err(|error| *error),
        ));
        result
    }
}

fn require(condition: bool, message: &'static str) -> Check {
    if condition { Ok(()) } else { Err(message) }
}

/// Run the default check on the source actually held by an existing parse.
///
/// # Errors
/// Returns the violated invariant; oversize input skips only this operation.
pub fn check_ast(ast: &Ast) -> Result<Outcome, &'static str> {
    if ast.source().len() > MAX_SOURCE {
        return Ok(Outcome::Skipped(Skip::Oversize));
    }
    check_parse(
        &Parsed {
            language: ast.language(),
            tree: ast.as_tree_sitter(),
            source: ast.source(),
            typed: None,
        },
        DocumentationOptions::default(),
        false,
        false,
        |_, _| Ok(()),
    )
}

/// Decode the version-one header, parse its unchanged payload once, and check it.
///
/// # Errors
/// Returns a property violation, including an unexpected collection failure.
pub fn exercise(bytes: &[u8]) -> Result<Outcome, &'static str> {
    exercise_with(bytes, |_, _| Ok(()))
}

fn exercise_with(
    bytes: &[u8],
    inspect: impl FnOnce(Option<&DocumentationIndex<'_>>, &tree_sitter::Tree) -> Check,
) -> Result<Outcome, &'static str> {
    let input = match Input::decode(bytes) {
        Ok(input) => input,
        Err(skip) => return Ok(Outcome::Skipped(skip)),
    };
    if input.source.len() > MAX_SOURCE {
        return Ok(Outcome::Skipped(Skip::Oversize));
    }
    if input.mode == Mode::IncludedRanges {
        let Some(ranges) = included_ranges(input.source) else {
            return Ok(Outcome::Skipped(Skip::IncludedRangeLimit));
        };
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(
                &input
                    .language
                    .tree_sitter_language()
                    .map_err(|_| "language unavailable")?,
            )
            .map_err(|_| "grammar setup")?;
        parser
            .set_included_ranges(&ranges)
            .map_err(|_| "range setup")?;
        let tree = parser
            .parse(input.source, None)
            .ok_or("included parse aborted")?;
        observation::record(Event::IncludedParse(tree.included_ranges().len()));
        return check_parse(
            &Parsed {
                language: input.language,
                tree: &tree,
                source: input.source,
                typed: None,
            },
            input.options,
            input.repeat,
            input.identity,
            inspect,
        );
    }
    let parser = AnyParser::parse(input.language, input.source.to_vec(), None, None)
        .map_err(|_| "primary parse failed")?;
    observation::record(Event::Parse);
    require(parser.code() == input.source, "tag payload changed")?;
    let parsed = Parsed {
        language: input.language,
        tree: parser.ts_tree(),
        source: parser.code(),
        typed: Some(&parser),
    };
    if input.mode == Mode::CancelledScope {
        let flag = ParseCancel::new();
        flag.cancel();
        require(
            matches!(
                flag.run(|| parsed.collect(input.options)),
                Err(CollectionError::Cancelled)
            ),
            "active cancellation not propagated",
        )?;
    }
    let check = || {
        check_parse(
            &parsed,
            input.options,
            input.repeat,
            input.identity,
            inspect,
        )
    };
    if input.mode == Mode::ActiveScope {
        let flag = ParseCancel::new();
        flag.run(|| {
            let outcome = check()?;
            #[cfg(test)]
            observation::active_witness(&flag, &parsed, input.options)?;
            Ok(outcome)
        })
    } else {
        check()
    }
}

fn check_parse(
    parsed: &Parsed<'_>,
    options: DocumentationOptions,
    repeat: bool,
    identity: bool,
    inspect: impl FnOnce(Option<&DocumentationIndex<'_>>, &tree_sitter::Tree) -> Check,
) -> Result<Outcome, &'static str> {
    let Parsed {
        language,
        tree,
        source,
        ..
    } = *parsed;
    let result = parsed.collect(options);
    let profile = match expected_profile(language, options.profile) {
        Ok(profile) => profile,
        Err(expected) => {
            require(
                result.err() == Some(expected),
                "wrong adapter/profile rejection",
            )?;
            inspect(None, tree)?;
            return Ok(Outcome::Rejected(expected));
        }
    };
    let index = result.map_err(|_| "unexpected collection failure")?;
    require(
        index.source() == source && index.profile() == profile,
        "index source/profile mismatch",
    )?;
    properties::check(&index, tree, source)?;
    if identity {
        properties::negative_nodes(&index, tree)?;
    }
    if repeat {
        let second = parsed
            .collect(options)
            .map_err(|_| "repeat collection failed")?;
        properties::repeat(&index, &second)?;
    }
    inspect(Some(&index), tree)?;
    if matches!(language, LANG::Ccomment | LANG::Preproc) {
        require(
            index.capability() == Capability::NoDeclarationModel,
            "helper capability",
        )?;
        require(
            index.documents().len() == 0 && index.associations().len() == 0,
            "helper emitted documentation",
        )?;
        Ok(Outcome::Helper)
    } else {
        require(
            index.capability() == Capability::Partial,
            "adapter capability",
        )?;
        Ok(Outcome::Checked)
    }
}

// Every non-NUL byte is a separate included range, deliberately allowing adjacent
// ranges to divide a scalar. NUL bytes create gaps without changing coordinates.
fn included_ranges(source: &[u8]) -> Option<Vec<tree_sitter::Range>> {
    let mut ranges = Vec::new();
    let mut point = tree_sitter::Point::new(0, 0);
    for (at, byte) in source.iter().copied().enumerate() {
        let start_point = point;
        if byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
        if byte != 0 {
            ranges.push(tree_sitter::Range {
                start_byte: at,
                start_point,
                end_byte: at + 1,
                end_point: point,
            });
            if ranges.len() > MAX_STRUCTURE {
                return None;
            }
        }
    }
    (!ranges.is_empty()).then_some(ranges)
}
