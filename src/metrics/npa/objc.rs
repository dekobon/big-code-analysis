//! `Npa` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Counts the data attributes in an ObjC `instance_variables` `{ … }`
// block, returning `(total, public)`. Each `instance_variable` child is
// either a `visibility_specification` marker that flips the current
// visibility for the fields that follow (ObjC ivars default to
// `@protected`, so only fields under an explicit `@public` marker are
// public), or a field declaration whose `struct_declarator`s are the
// attributes — a multi-declarator `int a, b;` contributes two.
fn objc_count_instance_variables(block: &Node) -> (usize, usize) {
    let (mut total, mut public) = (0usize, 0usize);
    let mut is_public = false;
    for ivar in block.children() {
        if ivar.kind_id() != Objc::InstanceVariable as u16 {
            continue;
        }
        if let Some(vis) = ivar.first_child(|id| id == Objc::VisibilitySpecification as u16) {
            is_public = vis.first_child(|id| id == Objc::ATpublic as u16).is_some();
            continue;
        }
        let mut fields = 0;
        for decl in ivar.children() {
            if decl.kind_id() == Objc::StructDeclaration as u16 {
                fields += decl
                    .children()
                    .filter(|d| d.kind_id() == Objc::StructDeclarator as u16)
                    .count();
            }
        }
        total += fields;
        if is_public {
            public += fields;
        }
    }
    (total, public)
}

// Objective-C public-attribute count. The canonical ObjC public
// attribute is the `@property` declaration (always part of the public
// `@interface` / `@protocol` contract), so each counts toward both `na`
// and `npa`. Instance variables (`@implementation` / `@interface` `{ … }`
// blocks) are visibility-aware via `objc_count_instance_variables`.
// Members are direct children of the class node (a `@protocol` nests its
// post-`@required` / `@optional` members under a
// `qualified_protocol_interface_declaration`), so we walk them when the
// class node itself is visited — the same point `is_class_space` is
// marked (mirroring the C++ impl's marking step).
impl Npa for ObjcCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Objc::*;

        let is_interface = matches!(node.kind_id().into(), ClassInterface | ProtocolDeclaration);
        if !is_interface && node.kind_id() != ClassImplementation as u16 {
            return;
        }
        if stats.is_disabled() {
            stats.is_class_space = true;
        }
        let (mut attributes, mut public) = (0usize, 0usize);
        for child in node.children() {
            match child.kind_id().into() {
                PropertyDeclaration => {
                    attributes += 1;
                    public += 1;
                }
                QualifiedProtocolInterfaceDeclaration => {
                    for inner in child.children() {
                        if inner.kind_id() == Objc::PropertyDeclaration as u16 {
                            attributes += 1;
                            public += 1;
                        }
                    }
                }
                InstanceVariables => {
                    let (na, npa) = objc_count_instance_variables(&child);
                    attributes += na;
                    public += npa;
                }
                _ => {}
            }
        }
        // Route to the interface or class accumulator by the space kind.
        if is_interface {
            stats.interface_na += attributes;
            stats.interface_npa += public;
        } else {
            stats.class_na += attributes;
            stats.class_npa += public;
        }
    }
}
