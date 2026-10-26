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
// them when the class node itself is visited, where `stats` is already the
// class space. `@property` accessors are auto-generated and
// carry no `method_declaration` node, so they are not counted here.
/// Whether a container member is a method: a bodiless
/// `method_declaration`, or the `FuncOpener` row's callable that the
/// container *owns* — a `method_definition`. A free C function inside
/// `@implementation` is a callable too, and the one the container does
/// not own (#1356), so it is the callable `is_non_member_function`
/// declines. `ancestors` is the member's chain.
fn objc_is_method<'a>(member: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    use crate::roles::objc::func_opener;
    member.kind_id() == Objc::MethodDeclaration as u16
        || (func_opener::is_callable(member, code, ancestors)
            && !ObjcCode::is_non_member_function(member, code, ancestors))
}

impl Npm for ObjcCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Objc::*;

        // Which container this is, for the interface / class bucket; not
        // whether it opens a space.
        // role-bypass: allow(Container) — the interface bucket
        let is_interface = matches!(node.kind_id().into(), ClassInterface | ProtocolDeclaration);
        // role-bypass: allow(Container) — the class bucket
        if !is_interface && node.kind_id() != ClassImplementation as u16 {
            return;
        }
        let mut methods = 0;
        let under = ancestors.of_children(node);
        let members = under.get();
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
                    let in_wrapper = members.of_children(&child);
                    methods += child
                        .children()
                        .filter(|inner| objc_is_method(inner, code, in_wrapper.get()))
                        .count();
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
