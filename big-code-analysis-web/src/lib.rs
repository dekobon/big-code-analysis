//! REST API surface for `big-code-analysis`. Run via the `bca-web` binary.

// The deeply nested `json!` literals in server.rs tests exceed the default
// recursion limit (128) during `json_internal!` macro expansion.
#![recursion_limit = "256"]

/// HTTP endpoints and request handlers.
pub mod web;
pub use web::*;
