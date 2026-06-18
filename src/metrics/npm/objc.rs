//! `Npm` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Objective-C public-method count. ObjC has no syntactic method-privacy
// keyword (no `public:` / `private:` block): a method is "public" exactly
// when it is declared in the `@interface` / `@protocol`, and "private" by
// the convention of being defined in the `@implementation` without an
// interface declaration. There is no per-node visibility marker to read,
// so every method counts as public — `npm == nm`. The members are direct
// children of the class node (not a `field_declaration_list`), so we walk
// them when the class node itself is visited (where `stats` is already the
// class space, the same point `is_class_space` is marked — mirroring the
// C++ impl's marking step). `@property` accessors are auto-generated and
// carry no `method_declaration` node, so they are not counted here.
impl Npm for ObjcCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Objc::*;

        let is_interface = matches!(node.kind_id().into(), ClassInterface | ProtocolDeclaration);
        if !is_interface && node.kind_id() != ClassImplementation as u16 {
            return;
        }
        if stats.is_disabled() {
            stats.is_class_space = true;
        }
        let mut methods = 0;
        for child in node.children() {
            match child.kind_id().into() {
                // `@interface` lists `method_declaration`s as direct
                // children; a `@protocol`'s first (unqualified) members
                // are direct too. (`@implementation` method *definitions*
                // are never direct children — they are always wrapped in
                // an `implementation_definition`, handled below.)
                MethodDeclaration => methods += 1,
                // `@implementation` wraps each member in an
                // `implementation_definition`, and a `@protocol` groups
                // members after an `@required` / `@optional` marker under a
                // `qualified_protocol_interface_declaration`. Both wrappers
                // are descended one level for method nodes (free C
                // functions and `@synthesize`, also wrapped by the former,
                // are not methods, so they are skipped).
                ImplementationDefinition | QualifiedProtocolInterfaceDeclaration => {
                    for inner in child.children() {
                        if matches!(inner.kind_id().into(), MethodDeclaration | MethodDefinition) {
                            methods += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        // ObjC has no method-privacy keyword, so every method is public:
        // route to the interface or class accumulator by the space kind.
        if is_interface {
            stats.interface_nm += methods;
            stats.interface_npm += methods;
        } else {
            stats.class_nm += methods;
            stats.class_npm += methods;
        }
    }
}
