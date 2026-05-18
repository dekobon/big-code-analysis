// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::path::PathBuf;
use std::sync::Arc;

use crate::node::Node;

use crate::dump::*;
use crate::error::MetricsError;
use crate::traits::*;

/// Finds the types of nodes specified in the input slice.
///
/// "No matches" is represented by `Ok(Vec::new())` rather than an
/// error — it is a normal outcome, not a failure mode. The
/// [`Result`] return type is for forward compatibility with the
/// other entry points; today no [`MetricsError`] variant is produced
/// by `find`, but future strict-parsing modes may surface
/// [`MetricsError::ParseHasErrors`] here.
///
/// # Errors
///
/// Currently infallible; the [`Result`] wrapper aligns the signature
/// with [`crate::metrics`] and [`crate::operands_and_operators`] so
/// callers can use the `?` operator uniformly.
pub fn find<'a, T: ParserTrait>(
    parser: &'a T,
    filters: &[String],
) -> Result<Vec<Node<'a>>, MetricsError> {
    let filters = parser.get_filters(filters);
    let node = parser.get_root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let mut good = Vec::new();
    let mut children = Vec::new();

    stack.push(node);

    while let Some(node) = stack.pop() {
        if filters.any(&node) {
            good.push(node);
        }
        cursor.reset(&node);
        if cursor.goto_first_child() {
            loop {
                children.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            for child in children.drain(..).rev() {
                stack.push(child);
            }
        }
    }
    Ok(good)
}

/// Configuration options for finding different
/// types of nodes in a code.
#[derive(Debug)]
pub struct FindCfg {
    /// Path to the file containing the code
    pub path: PathBuf,
    /// Types of nodes to find
    pub filters: Arc<[String]>,
    /// The first line of code considered in the search
    ///
    /// If `None`, the search starts from the
    /// first line of code in a file
    pub line_start: Option<usize>,
    /// The end line of code considered in the search
    ///
    /// If `None`, the search ends at the
    /// last line of code in a file
    pub line_end: Option<usize>,
}

/// Type tag identifying the node-find action; carries no data.
pub struct Find {
    _guard: (),
}

impl Callback for Find {
    type Res = std::io::Result<()>;
    type Cfg = FindCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        if let Ok(good) = find(parser, &cfg.filters)
            && !good.is_empty()
        {
            println!("In file {}", cfg.path.display());
            for node in good {
                dump_node(parser.get_code(), &node, 1, cfg.line_start, cfg.line_end)?;
            }
            println!();
        }
        Ok(())
    }
}
