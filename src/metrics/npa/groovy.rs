//! `Npa` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Groovy uses the dekobon grammar, which models class/interface/trait/
// annotation-type/record bodies as a single `class_body` node and
// flattens modifiers as direct children of the declaration (the
// `_modifier` rule is hidden — no `Modifiers` wrapper). That rules out
// the Java macro, so an explicit impl is required.
//
// `def field` at class scope parses as a `FieldDeclaration` with `Def`
// in the modifier slot and no `Public`, so it's correctly excluded from
// `class_npa` unless explicitly annotated `public` — consistent with
// Groovy's access semantics (we conservatively follow Java).
impl Npa for GroovyCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Groovy::*;

        if opens_container_space::<Self>(node) && stats.is_disabled() {
            stats.is_class_space = true;
        }

        match node.kind_id().into() {
            ClassBody | EnumBody => {
                let is_interface_like = groovy_body_is_interface_like(node, ancestors);

                for declaration in node
                    .children()
                    .filter(|n| matches!(n.kind_id().into(), FieldDeclaration))
                {
                    let attributes = declaration
                        .children()
                        .filter(|n| matches!(n.kind_id().into(), VariableDeclarator))
                        .count();
                    if is_interface_like {
                        stats.interface_na += attributes;
                        stats.interface_npa += attributes;
                    } else {
                        stats.class_na += attributes;
                        if groovy_has_explicit_public(&declaration) {
                            stats.class_npa += attributes;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
