use std::io::Write;
use termcolor::{Color, ColorChoice, StandardStream, StandardStreamLock};

use crate::ops::Ops;

use crate::tools::{color, intense_color};

/// Dumps all operands and operators of a code.
///
/// Returns a [`Result`] value, when an error occurs.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use big_code_analysis::{dump_ops, operands_and_operators, CppParser, ParserTrait};
///
/// # fn main() {
/// let source_code = "int a = 42;";
///
/// // The path to a dummy file used to contain the source code
/// let path = PathBuf::from("foo.c");
/// let source_as_vec = source_code.as_bytes().to_vec();
///
/// // The parser of the code, in this case a CPP parser
/// let parser = CppParser::new(source_as_vec, &path, None);
///
/// // Retrieve all operands and operators
/// let ops = operands_and_operators(&parser, &path).unwrap();
///
/// // Dump all operands and operators
/// dump_ops(&ops).unwrap();
/// # }
/// ```
///
/// [`Result`]: #variant.Result
pub fn dump_ops(ops: &Ops) -> std::io::Result<()> {
    let stdout = StandardStream::stdout(ColorChoice::Always);
    let mut stdout = stdout.lock();
    dump_space(ops, "", true, &mut stdout)?;
    color(&mut stdout, Color::White)?;

    Ok(())
}

fn dump_space(
    space: &Ops,
    prefix: &str,
    last: bool,
    stdout: &mut StandardStreamLock,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Yellow)?;
    write!(stdout, "{}: ", space.kind)?;

    intense_color(stdout, Color::Cyan)?;
    write!(stdout, "{}", space.name.as_ref().map_or("", |name| name))?;

    intense_color(stdout, Color::Red)?;
    writeln!(stdout, " (@{})", space.start_line)?;

    let prefix = format!("{prefix}{pref_child}");
    dump_space_ops(space, &prefix, space.spaces.is_empty(), stdout)?;

    if let Some((last, spaces)) = space.spaces.split_last() {
        for space in spaces {
            dump_space(space, &prefix, false, stdout)?;
        }
        dump_space(last, &prefix, true, stdout)?;
    }

    Ok(())
}

fn dump_space_ops(
    ops: &Ops,
    prefix: &str,
    last: bool,
    stdout: &mut StandardStreamLock,
) -> std::io::Result<()> {
    dump_ops_values("operators", &ops.operators, prefix, last, stdout)?;
    dump_ops_values("operands", &ops.operands, prefix, last, stdout)
}

fn dump_ops_values(
    name: &str,
    ops: &[String],
    prefix: &str,
    last: bool,
    stdout: &mut StandardStreamLock,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "{name}")?;

    let Some((last_op, rest)) = ops.split_last() else {
        return Ok(());
    };

    let prefix = format!("{prefix}{pref_child}");
    for op in rest {
        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}|- ")?;

        color(stdout, Color::White)?;
        writeln!(stdout, "{op}")?;
    }

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}`- ")?;

    color(stdout, Color::White)?;
    writeln!(stdout, "{last_op}")
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::spaces::SpaceKind;

    #[test]
    fn dump_ops_empty_operators_and_operands_does_not_panic() {
        // Regression: `ops.len() - 1` underflowed (usize) when ops was empty,
        // then `ops.last().unwrap()` panicked. A space with no Halstead
        // operators or operands is a realistic input.
        let ops = Ops {
            name: Some("unit".to_string()),
            name_was_lossy: false,
            start_line: 1,
            end_line: 1,
            kind: SpaceKind::Unit,
            spaces: vec![],
            operands: vec![],
            operators: vec![],
        };
        assert!(dump_ops(&ops).is_ok());
    }
}
