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
// member is worth is this consumer's own. `ancestors` is the member's
// chain.
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
// narrows it with its own `private` / `protected` / `internal` modifier
// (`public int X { get; private set; }` exposes one public getter, not
// two). So of the member's `total` — its `csharp_count_member`, which
// the caller has already read — only the accessors *without* an
// explicit narrowing modifier count toward npm.
//
// The `.max(1)` total covers the accessor-less expression-bodied form
// (`public int Z => 0;`) and the single-accessor auto-property
// (`public int W { get; }`): both define one implicit/sole getter that
// inherits the member's public visibility, so the public count equals
// the total and no narrowing can apply.
fn csharp_member_public_method_count(member: &Node, total: usize) -> usize {
    use Csharp::*;
    let narrowed = member
        .children()
        .filter(|c| matches!(c.kind_id().into(), AccessorList))
        .flat_map(|list| list.children())
        // role-bypass: allow(FuncOpener) — which accessors narrow the
        // member's visibility, not whether an accessor is a callable
        .filter(|c| matches!(c.kind_id().into(), AccessorDeclaration))
        .filter(super::npa::csharp_has_non_public_access_modifier)
        .count();
    total.saturating_sub(narrowed)
}

// Whether a member carries the `partial` modifier.
fn csharp_is_partial(member: &Node) -> bool {
    member.children().any(|child| {
        // role-bypass: allow(Modifier) — which modifier, not whether the node is one
        matches!(child.kind_id().into(), Csharp::Modifier)
            && child.first_child(|id| id == Csharp::Partial).is_some()
    })
}

// Counts the public methods an interface member contributes. A C# 8+
// interface member is public unless it narrows its access, and three
// unmodified members are not public either: the static constructor,
// which only the runtime calls (an interface has no other constructor);
// an explicit implementation of a base interface's member
// (`void IA.M() { }`), reachable only through that interface; and a
// `partial` method, which is private unless it says `public` (C#
// standard §15.6.9, kept for interfaces by Roslyn's
// `ModifierUtils.AdjustModifiersForAnInterfaceMember`, which makes a
// partial property public as any other member is). `total` is the
// member's `csharp_count_member`; a child worth no method — a field, a
// comment, a brace — has no public one, so its modifiers are not read.
fn csharp_interface_member_public_method_count(member: &Node, total: usize) -> usize {
    use Csharp::*;
    if total == 0 {
        return 0;
    }
    // role-bypass: allow(FuncOpener) — which callable is hidden from the
    // public surface, not whether the member is one
    let hidden = member.kind_id() == ConstructorDeclaration
        || member
            .first_child(|id| id == ExplicitInterfaceSpecifier)
            .is_some()
        || (member.kind_id() == MethodDeclaration
            && csharp_is_partial(member)
            && !super::npa::csharp_is_explicit_public(member));
    if hidden || !super::npa::csharp_interface_member_is_public(member) {
        return 0;
    }
    csharp_member_public_method_count(member, total)
}

impl Npm for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::csharp::func_opener;
        use Csharp::*;

        // A primary constructor is the `parameter_list` child of the
        // type declaration itself, not of its `declaration_list`
        // (#1828): counted here, where the walk is still on the class
        // space — the list's own function space opens one node later.
        // Whether a list under this node is one is the `FuncOpener`
        // row's question, asked of the list under the node's own chain,
        // so the row's parent gate has one copy. Its accessibility is
        // the type's (C# 12 §15.8.1, "the primary constructor is
        // public"), with no member modifier of its own to read, so
        // `csharp_is_explicit_public` does not apply.
        // role-bypass: allow(FuncOpener) — a traversal step to the list
        // the row then judges, not whether this node opens a space
        if let Some(list) = node.first_child(|id| id == ParameterList)
            && func_opener::is_callable(&list, code, ancestors.of_children(node).get())
        {
            stats.class_nm += 1;
            stats.class_npm += 1;
        }
        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };

        let under = ancestors.of_children(node);
        let members = under.get();
        // role-bypass: allow(Container) — which container's body this is,
        // for the class / interface bucket; not whether it opens a space
        match parent_kind {
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for member in node.children() {
                    let total = csharp_count_member(&member, code, members);
                    stats.class_nm += total;
                    // A public member exposes all its accessors as public
                    // methods *except* those that narrow visibility with
                    // their own `private` / `protected` / `internal`
                    // modifier. A child worth no method has no public
                    // one, so its modifiers are not read.
                    if total > 0 && super::npa::csharp_is_explicit_public(&member) {
                        stats.class_npm += csharp_member_public_method_count(&member, total);
                    }
                }
            }
            InterfaceDeclaration => {
                for member in node.children() {
                    let total = csharp_count_member(&member, code, members);
                    stats.interface_nm += total;
                    stats.interface_npm +=
                        csharp_interface_member_public_method_count(&member, total);
                }
            }
            _ => {}
        }
    }
}
