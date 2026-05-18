// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use crate::abc::Abc;
use crate::checker::Checker;
use crate::cognitive::Cognitive;
use crate::cyclomatic::Cyclomatic;
use crate::exit::Exit;
use crate::halstead::Halstead;
use crate::loc::Loc;
use crate::mi::Mi;
use crate::nargs::NArgs;
use crate::nom::Nom;
use crate::npa::Npa;
use crate::npm::Npm;
use crate::tokens::Tokens;
use crate::wmc::Wmc;

use crate::alterator::Alterator;
use crate::getter::Getter;

use crate::c_macro;
use crate::langs::*;
use crate::node::{Node, Tree};
use crate::preproc::{PreprocResults, get_macros};
use crate::traits::*;

/// Parsed source plus the tree-sitter `Tree` for a given language `T`.
///
/// Construct with [`Parser::new`] and feed the result into the metric,
/// alterator, or AST-dump entry points. The type parameter `T` is one
/// of the language code tags (`RustCode`, `PythonCode`, etc.) declared
/// by the internal `mk_code!` macro.
#[derive(Debug)]
pub struct Parser<
    T: LanguageInfo
        + Alterator
        + Checker
        + Getter
        + Abc
        + Cognitive
        + Cyclomatic
        + Exit
        + Halstead
        + Loc
        + Mi
        + NArgs
        + Nom
        + Npa
        + Npm
        + Tokens
        + Wmc,
> {
    code: Vec<u8>,
    tree: Tree,
    phantom: PhantomData<T>,
}

type FilterFn = dyn Fn(&Node) -> bool;

/// Collection of node-matching predicates used by the AST-walking
/// metric and dump routines to decide whether to visit a node.
pub struct Filter {
    filters: Vec<Box<FilterFn>>,
}

impl Filter {
    /// Returns `true` if *any* of the configured predicates matches `node`.
    #[must_use]
    pub fn any(&self, node: &Node) -> bool {
        for f in &self.filters {
            if f(node) {
                return true;
            }
        }
        false
    }

    /// Returns `true` if *every* configured predicate matches `node`.
    #[must_use]
    pub fn all(&self, node: &Node) -> bool {
        for f in &self.filters {
            if !f(node) {
                return false;
            }
        }
        true
    }
}

#[inline]
fn get_fake_code<T: LanguageInfo>(
    code: &[u8],
    path: &Path,
    pr: Option<Arc<PreprocResults>>,
) -> Option<Vec<u8>> {
    if let Some(pr) = pr {
        match T::get_lang() {
            LANG::Cpp => {
                let macros = get_macros(path, &pr.files);
                c_macro::replace(code, &macros)
            }
            _ => None,
        }
    } else {
        None
    }
}

impl<
    T: 'static
        + LanguageInfo
        + Alterator
        + Checker
        + Getter
        + Abc
        + Cognitive
        + Cyclomatic
        + Exit
        + Halstead
        + Loc
        + Mi
        + NArgs
        + Nom
        + Npa
        + Npm
        + Tokens
        + Wmc,
> ParserTrait for Parser<T>
{
    type Checker = T;
    type Getter = T;
    type Cognitive = T;
    type Cyclomatic = T;
    type Halstead = T;
    type Loc = T;
    type Nom = T;
    type Mi = T;
    type NArgs = T;
    type Exit = T;
    type Wmc = T;
    type Abc = T;
    type Npm = T;
    type Npa = T;
    type Tokens = T;

    fn new(code: Vec<u8>, path: &Path, pr: Option<Arc<PreprocResults>>) -> Self {
        let fake_code = get_fake_code::<T>(&code, path, pr);
        let code = if let Some(fake) = fake_code {
            fake
        } else {
            code
        };

        let tree = Tree::new::<T>(&code);

        Self {
            code,
            tree,
            phantom: PhantomData,
        }
    }

    #[inline]
    fn get_language(&self) -> LANG {
        T::get_lang()
    }

    #[inline]
    fn get_root(&self) -> Node<'_> {
        self.tree.get_root()
    }

    #[inline]
    fn get_code(&self) -> &[u8] {
        &self.code
    }

    fn get_filters(&self, filters: &[String]) -> Filter {
        let mut res: Vec<Box<FilterFn>> = Vec::new();
        for f in filters {
            let f = f.as_str();
            match f {
                "all" => res.push(Box::new(|_: &Node| -> bool { true })),
                "call" => res.push(Box::new(T::is_call)),
                "comment" => res.push(Box::new(T::is_comment)),
                "error" => res.push(Box::new(T::is_error)),
                "string" => res.push(Box::new(T::is_string)),
                "function" => res.push(Box::new(T::is_func)),
                _ => {
                    if let Ok(n) = f.parse::<u16>() {
                        res.push(Box::new(move |node: &Node| -> bool { node.kind_id() == n }));
                    } else {
                        let f = f.to_owned();
                        res.push(Box::new(move |node: &Node| -> bool {
                            node.kind().contains(&f)
                        }));
                    }
                }
            }
        }
        if res.is_empty() {
            res.push(Box::new(|_: &Node| -> bool { true }));
        }

        Filter { filters: res }
    }
}

impl<
    T: 'static
        + LanguageInfo
        + Alterator
        + Checker
        + Getter
        + Abc
        + Cognitive
        + Cyclomatic
        + Exit
        + Halstead
        + Loc
        + Mi
        + NArgs
        + Nom
        + Npa
        + Npm
        + Tokens
        + Wmc,
> Parser<T>
{
    /// Builds a [`Parser`] from a pre-parsed [`tree_sitter::Tree`]
    /// and the matching source bytes.
    ///
    /// Use this when the caller already drives `tree-sitter` for
    /// other purposes (e.g. an editor doing incremental reparsing)
    /// and wants the metric walker to reuse the parse instead of
    /// running its own. The standard byte-based entry point
    /// remains [`ParserTrait::new`].
    ///
    /// The supplied `tree` must have been produced from `code` with
    /// the tree-sitter language matching `T` — typically obtained
    /// via [`crate::LANG::get_tree_sitter_language`]. A mismatch is
    /// not `unsafe`, but metric values will be nonsensical because
    /// the tree's `kind_id` values will not correspond to the per-
    /// language enum the metric `compute` functions match on.
    #[must_use]
    pub fn from_tree(tree: tree_sitter::Tree, code: Vec<u8>) -> Self {
        Self {
            code,
            tree: Tree::from_ts_tree(tree),
            phantom: PhantomData,
        }
    }
}
