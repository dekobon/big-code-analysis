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
//
// A member is the `FuncOpener` row's callable: the method-like
// declarations count one each; an indexer or property counts its
// accessors, and the accessor-less form — the one the row marks
// callable — one. The bodied form is not the row's callable (its
// accessors are), so it is read by kind here: how many methods a
// member is worth is this consumer's own. `ancestors` is the body's
// chain standing in for the member's; the row reads none.
fn csharp_count_member<'a>(member: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> usize {
    use crate::roles::csharp::func_opener;
    use Csharp::*;
    // role-bypass: allow(FuncOpener) — how many methods a property or
    // indexer is worth (its accessor count), not whether it is one
    if matches!(
        member.kind_id().into(),
        PropertyDeclaration | IndexerDeclaration
    ) {
        return csharp_accessor_count(member).max(1);
    }
    usize::from(func_opener::is_callable(member, code, ancestors))
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
fn csharp_member_public_method_count<'a>(
    member: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> usize {
    use Csharp::*;
    let total = csharp_count_member(member, code, ancestors);
    let narrowed = member
        .children()
        .filter(|c| matches!(c.kind_id().into(), AccessorList))
        .flat_map(|list| list.children())
        // role-bypass: allow(FuncOpener) — which accessors narrow the
        // member's visibility, not whether an accessor is a callable
        .filter(|c| matches!(c.kind_id().into(), AccessorDeclaration))
        .filter(super::npa::csharp_node_has_private_or_protected_modifier)
        .count();
    total.saturating_sub(narrowed)
}

impl Npm for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Csharp::*;

        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };

        // role-bypass: allow(Container) — which container's body this is,
        // for the class / interface bucket; not whether it opens a space
        match parent_kind {
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for member in node.children() {
                    stats.class_nm += csharp_count_member(&member, code, ancestors);
                    // A public member exposes all its accessors as public
                    // methods *except* those that narrow visibility with
                    // their own `private` / `protected` modifier.
                    if super::npa::csharp_is_explicit_public(&member) {
                        stats.class_npm +=
                            csharp_member_public_method_count(&member, code, ancestors);
                    }
                }
            }
            // Interface members default to public (matching Java's rule);
            // skip the visibility scan entirely.
            InterfaceDeclaration => {
                for member in node.children() {
                    stats.interface_nm += csharp_count_member(&member, code, ancestors);
                }
                stats.interface_npm = stats.interface_nm;
            }
            _ => {}
        }
    }
}
