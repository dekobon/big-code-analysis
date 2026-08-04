//! `Npa` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npa for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Csharp::*;

        if opens_container_space::<Self>(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        // Class / struct / record / interface bodies all share
        // `DeclarationList`; the parent kind disambiguates.
        if !matches!(node.kind_id().into(), DeclarationList) {
            return;
        }
        let Some(parent_kind) = ancestors.parent(node).map(|p| p.kind_id().into()) else {
            return;
        };
        match parent_kind {
            // For `RecordDeclaration`, only explicit body fields are
            // counted. The implicit `parameter_list` of a positional
            // record (`record Person(string Name, int Age);`) is not
            // walked here — its parameters become auto-generated public
            // properties at the IL level, but modelling them would
            // require synthesizing nodes that don't appear in the AST.
            ClassDeclaration | StructDeclaration | RecordDeclaration => {
                for declaration in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                {
                    let attributes = csharp_count_field_declarators(&declaration);
                    stats.class_na += attributes;
                    if csharp_is_explicit_public(&declaration) {
                        stats.class_npa += attributes;
                    }
                }
            }
            // C# 8+ interfaces can declare fields with explicit modifiers
            // (rare); members declared without an explicit modifier default
            // to public, mirroring Java's interface convention.
            InterfaceDeclaration => {
                for declaration in node
                    .children()
                    .filter(|c| matches!(c.kind_id().into(), FieldDeclaration))
                {
                    let attributes = csharp_count_field_declarators(&declaration);
                    stats.interface_na += attributes;
                    // The modifier applies to every declarator of the field,
                    // so the public/private split is per-declaration: count
                    // all declarators as public unless the field is explicitly
                    // private/protected.
                    if csharp_interface_member_is_public(&declaration) {
                        stats.interface_npa += attributes;
                    }
                }
            }
            _ => {}
        }
    }
}

// Count `VariableDeclarator`s nested under any aliased `VariableDeclaration`
// inside a C# `FieldDeclaration`. Both kinds emit two aliased `kind_id`s
// each; the macros centralize the alias union (lesson #2).
fn csharp_count_field_declarators(field_decl: &Node) -> usize {
    field_decl
        .children()
        .filter(|c| matches!(c.kind_id().into(), csharp_var_decl_kinds!()))
        .flat_map(|c| c.children())
        .filter(|c| matches!(c.kind_id().into(), csharp_var_declarator_kinds!()))
        .count()
}
