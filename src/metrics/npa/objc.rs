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
fn objc_count_instance_variables<'a>(
    block: &Node,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
) -> (usize, usize) {
    let (mut total, mut public) = (0usize, 0usize);
    let mut is_public = false;
    for ivar in block.children() {
        if !crate::roles::objc::member::owns(&ivar, code, ancestors) {
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
// class node itself is visited, where `stats` is already the class space.
impl Npa for ObjcCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::objc::member;
        use Objc::*;

        // Which container this is, for the interface / class bucket; not
        // whether it opens a space.
        // role-bypass: allow(Container) — the interface bucket
        let is_interface = matches!(node.kind_id().into(), ClassInterface | ProtocolDeclaration);
        // role-bypass: allow(Container) — the class bucket
        if !is_interface && node.kind_id() != ClassImplementation as u16 {
            return;
        }
        // A property is the `Member` row's, direct in the container or
        // one level under a `@required` / `@optional` group; an instance
        // variable is the row's too, under the `{ … }` block, where the
        // consumer counts its declarators and reads its access section.
        // The container's chain stands in for each; the row reads none.
        let (mut attributes, mut public) = (0usize, 0usize);
        for child in node.children() {
            match child.kind_id().into() {
                _ if member::owns(&child, code, ancestors) => {
                    attributes += 1;
                    public += 1;
                }
                QualifiedProtocolInterfaceDeclaration => {
                    for inner in child.children() {
                        if member::owns(&inner, code, ancestors) {
                            attributes += 1;
                            public += 1;
                        }
                    }
                }
                InstanceVariables => {
                    let (na, npa) = objc_count_instance_variables(&child, code, ancestors);
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
