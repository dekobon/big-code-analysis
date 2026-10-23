//! `Getter` implementation for C comments.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for CcommentCode {
    // Every row is `absent`, so this answers `Unknown` for every node;
    // reading the rows rather than inheriting a default is what lets
    // the trait require the method of every language (#1693).
    keyword_roles!(ccomment);
}
