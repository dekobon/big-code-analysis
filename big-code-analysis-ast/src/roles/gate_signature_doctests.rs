//! The gate signatures `role_table!` holds a row to (#1874). A gate is
//! handed the node, then the source bytes only where its keyword is
//! `text_gated_by`, then the chain unless it is marked `no_chain`; the
//! other gate forms — a payer's `gated_by`, `callable_when`,
//! `pays_itself_when` and `opens_by` — have no text spelling and are
//! never handed the bytes. So a gate that reads the bytes under a
//! keyword that does not declare them is a call with one argument too
//! few, not a claim a test must catch.
//!
//! Each `compile_fail` doctest below is one row of the control, copied
//! verbatim — one owner of it, where the row has two — with its fn's
//! signature given a bytes parameter and nothing else changed. Kept off
//! the macro's own doc for the reason `parent_list_doctests.rs` gives.
//!
//! The control: one row per byte-less gate form, then the text forms,
//! each fn in the signature its keyword declares.
//!
//! ```
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node, SpaceKind};
//! fn owner(_: &Node<'_>, _: Ancestors<'_, '_>) -> bool { true }
//! fn owner_no_chain(_: &Node<'_>) -> bool { true }
//! fn payer(_: &Node<'_>, _: Ancestors<'_, '_>) -> bool { true }
//! fn callable(_: &Node<'_>, _: Ancestors<'_, '_>) -> bool { true }
//! fn pays(_: &Node<'_>, _: Ancestors<'_, '_>) -> bool { true }
//! fn opens(_: &Node<'_>, _: Ancestors<'_, '_>) -> SpaceKind { SpaceKind::Class }
//! fn opens_no_chain(_: &Node<'_>) -> SpaceKind { SpaceKind::Class }
//! fn text(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! fn text_no_chain(_: &Node<'_>, _: &[u8]) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Exit { owns [ReturnStatement(gated_by owner)] }
//!     Catch { owns [CatchClause(gated_by owner_no_chain no_chain)] }
//!     Comparison { owns [BinaryExpression] pays [EQEQ gated_by payer] }
//!     FuncOpener { owns [LambdaExpression(opens Function callable_when callable)] }
//!     ElseIf { owns [IfStatement(pays_itself_when pays)] pays [] }
//!     Container {
//!         owns [ClassDeclaration(opens_by opens),
//!               InterfaceDeclaration(opens_by opens_no_chain no_chain)]
//!     }
//!     Branch { owns [IfStatement(text_gated_by text)] }
//!     Loop { owns [WhileStatement(text_gated_by text_no_chain no_chain)] }
//!     ShortCircuit { owns [BinaryExpression] pays [AMPAMP text_gated_by text] }
//! }
//! # fn main() {}
//! ```
//!
//! An owner's `gated_by` gate:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn owner(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Exit { owns [ReturnStatement(gated_by owner)] }
//! }
//! # fn main() {}
//! ```
//!
//! The same, marked `no_chain`:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Java, Node};
//! fn owner_no_chain(_: &Node<'_>, _: &[u8]) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Catch { owns [CatchClause(gated_by owner_no_chain no_chain)] }
//! }
//! # fn main() {}
//! ```
//!
//! A payer's `gated_by` gate:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn payer(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Comparison { owns [BinaryExpression] pays [EQEQ gated_by payer] }
//! }
//! # fn main() {}
//! ```
//!
//! A `callable_when` fn:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn callable(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     FuncOpener { owns [LambdaExpression(opens Function callable_when callable)] }
//! }
//! # fn main() {}
//! ```
//!
//! A `pays_itself_when` fn:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn pays(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     ElseIf { owns [IfStatement(pays_itself_when pays)] pays [] }
//! }
//! # fn main() {}
//! ```
//!
//! An `opens_by` fn:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node, SpaceKind};
//! fn opens(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> SpaceKind { SpaceKind::Class }
//! role_table! {
//!     lang: Java;
//!     Container { owns [ClassDeclaration(opens_by opens)] }
//! }
//! # fn main() {}
//! ```
//!
//! The same, marked `no_chain`:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Java, Node, SpaceKind};
//! fn opens_no_chain(_: &Node<'_>, _: &[u8]) -> SpaceKind { SpaceKind::Class }
//! role_table! {
//!     lang: Java;
//!     Container { owns [InterfaceDeclaration(opens_by opens_no_chain no_chain)] }
//! }
//! # fn main() {}
//! ```
