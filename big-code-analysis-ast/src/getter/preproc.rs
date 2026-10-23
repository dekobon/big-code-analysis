//! `Getter` implementation for the C preprocessor.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for PreprocCode {
    // Every row is `absent`, so this answers `Unknown` for every node;
    // reading the rows rather than inheriting a default is what lets
    // the trait require the method of every language (#1693).
    keyword_roles!(preproc);
}
