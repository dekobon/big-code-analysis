//! `Npm` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Count direct method-like declarations and property / indexer
// accessors (each get/set/init is a method per C# IL semantics).
// Expression-bodied properties (`int W => _w;`) have no AccessorList
// but do define a getter — `.max(1)` keeps them at 1 method.
fn csharp_count_member(member: &Node) -> usize {
    use Csharp::*;
    match member.kind_id().into() {
        MethodDeclaration
        | ConstructorDeclaration
        | DestructorDeclaration
        | OperatorDeclaration
        | ConversionOperatorDeclaration => 1,
        PropertyDeclaration | IndexerDeclaration => csharp_accessor_count(member).max(1),
        _ => 0,
    }
}

// Counts the public methods contributed by a *public* property / indexer
// member. A C# accessor inherits the member's visibility unless it
// narrows it with its own `private` / `protected` modifier
// (`public int X { get; private set; }` exposes one public getter, not
// two). So of the member's `csharp_count_member` total, only the
// accessors *without* an explicit narrowing modifier count toward npm.
//
// The `.max(1)` total covers the accessor-less expression-bodied form
// (`public int Z => 0;`) and the single-accessor auto-property
// (`public int W { get; }`): both define one implicit/sole getter that
// inherits the member's public visibility, so the public count equals
// the total and no narrowing can apply.
fn csharp_member_public_method_count(member: &Node) -> usize {
    use Csharp::*;
    let total = csharp_count_member(member);
    let narrowed = member
        .children()
        .filter(|c| matches!(c.kind_id().into(), AccessorList))
        .flat_map(|list| list.children())
        .filter(|c| matches!(c.kind_id().into(), AccessorDeclaration))
        .filter(super::npa::csharp_node_has_private_or_protected_modifier)
        .count();
    total.saturating_sub(narrowed)
}

impl Npm for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Csharp::*;

        if Self::is_func_space(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };

        match parent_kind {
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for member in node.children() {
                    stats.class_nm += csharp_count_member(&member);
                    // A public member exposes all its accessors as public
                    // methods *except* those that narrow visibility with
                    // their own `private` / `protected` modifier.
                    if super::npa::csharp_is_explicit_public(&member) {
                        stats.class_npm += csharp_member_public_method_count(&member);
                    }
                }
            }
            // Interface members default to public (matching Java's rule);
            // skip the visibility scan entirely.
            InterfaceDeclaration => {
                for member in node.children() {
                    stats.interface_nm += csharp_count_member(&member);
                }
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}
