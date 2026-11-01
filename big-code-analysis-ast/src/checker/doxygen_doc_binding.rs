//! The doc-comment binding the C-family `Checker` impls share (#1613):
//! which doc blocks Doxygen attaches to the entry after them. A macro,
//! because each language has its own `Checker` impl to generate into.

// Generate Doxygen's attachment rule for a C-family `Checker` block
// (#1613): a doc block among a container's entries documents every entry
// Doxygen makes of what follows, read past directives that declare
// nothing; every doc block of the stretch is kept, as Doxygen
// concatenates them. A member-after `/**<` and a structural block
// (`@file`, `@class X`) stand alone. The targets are the dialect's
// `DocTarget` row (#1699), which `impl_doxygen_row!` below binds — the
// one place a dialect names its row, since a trait impl cannot be
// emitted from inside the `impl Checker` block — plus the one clause no
// row can spell: a parse error holding an entry, the `ERROR` kind being
// the enum's sentinel. `doxygen_target` composes the row with that
// clause; `doxygen_entry` reads the row back as "entry without
// conditionals" with no bytes.
macro_rules! impl_doxygen_doc_binding {
    () => {
        const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
            skips: $crate::lang_helpers::doxygen::doxygen_skips::<Self>,
            forward: $crate::lang_helpers::doxygen::doxygen_binds_forward::<Self>,
            gathers: Some(|node, code| {
                $crate::lang_helpers::doxygen::doxygen_standalone(
                    $crate::lang_helpers::doc_comment::node_bytes(node, code),
                )
            }),
            ..DocBinding::new(KeptDocs::All)
        });

        fn is_doc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
            $crate::lang_helpers::doxygen::doxygen_target::<Self>(node, code, ancestors)
        }
    };
}

// The row each dialect's Doxygen helpers read targets through; one impl
// per dialect beside its `impl Checker`, which the macro above composes.
macro_rules! impl_doxygen_row {
    ($code:ident, $roles:ident) => {
        impl $crate::lang_helpers::doxygen::DoxygenRow for $code {
            fn doc_target_row<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
                // `doxygen_entry` hands this row no bytes; the pin sits
                // beside the forward it protects.
                const _: () = assert!(
                    !$crate::roles::$roles::doc_target::TEXT_GATED,
                    "a C-family `DocTarget` row reads bytes; `doxygen_entry` cannot forward with `&[]`"
                );
                $crate::roles::$roles::doc_target::owns(node, code, ancestors)
            }
        }
    };
}
