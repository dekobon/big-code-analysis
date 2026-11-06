//! The parent-list shapes `role_table!` rejects (#1794), each a
//! `compile_fail` doctest that differs from the control below in one
//! respect only. Kept off the macro's own doc so they do not count
//! against `roles/mod.rs`: this module exists only under `cfg(doctest)`,
//! where rustdoc collects a private module's doc comments too.
//!
//! The control: every attribute the rejections combine, one per owner.
//!
//! ```
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Exit {
//!         owns [ReturnStatement(gated_by always), ThrowStatement(under [Block]),
//!               YieldStatement(not_under [ClassBody])]
//!     }
//!     Comparison {
//!         owns [BinaryExpression(holds_payer), InstanceofExpression(text_gated_by always)]
//!         pays [EQEQ under [BinaryExpression]]
//!     }
//! }
//! # fn main() {}
//! ```
//!
//! A parent list is the owner's gate, so it sits beside no other gate
//! and no `holds_payer`, in either order:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Exit { owns [ReturnStatement(under [Block] gated_by always)] }
//! }
//! # fn main() {}
//! ```
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Exit { owns [ReturnStatement(gated_by always under [Block])] }
//! }
//! # fn main() {}
//! ```
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::{Ancestors, Java, Node};
//! fn always(_: &Node<'_>, _: &[u8], _: Ancestors<'_, '_>) -> bool { true }
//! role_table! {
//!     lang: Java;
//!     Comparison { owns [InstanceofExpression(under [Block] text_gated_by always)] pays [EQEQ] }
//! }
//! # fn main() {}
//! ```
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::Java;
//! role_table! {
//!     lang: Java;
//!     Comparison { owns [BinaryExpression(holds_payer under [Block])] pays [EQEQ] }
//! }
//! # fn main() {}
//! ```
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::Java;
//! role_table! {
//!     lang: Java;
//!     Comparison { owns [BinaryExpression(under [Block] holds_payer)] pays [EQEQ] }
//! }
//! # fn main() {}
//! ```
//!
//! An owner takes one list, so not one of each direction:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::Java;
//! role_table! {
//!     lang: Java;
//!     Exit { owns [ThrowStatement(under [Block] not_under [ClassBody])] }
//! }
//! # fn main() {}
//! ```
//!
//! And a list names at least one kind, an owner's or a payer's — an
//! empty one would claim the kind and never admit it:
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::Java;
//! role_table! {
//!     lang: Java;
//!     Exit { owns [YieldStatement(not_under [])] }
//! }
//! # fn main() {}
//! ```
//!
//! ```compile_fail
//! # #[macro_use] extern crate big_code_analysis_ast;
//! # use big_code_analysis_ast::Java;
//! role_table! {
//!     lang: Java;
//!     Comparison { owns [BinaryExpression] pays [EQEQ under []] }
//! }
//! # fn main() {}
//! ```
