//! The doc-comment binding the C-family `Checker` impls share (#1613):
//! which doc blocks Doxygen attaches to the entry after them. A macro,
//! because each language has its own `Checker` impl to generate into.

// Generate Doxygen's attachment rule for a C-family `Checker` block
// (#1613): a doc block among a container's entries documents every entry
// Doxygen makes of what follows, read past directives that declare
// nothing; every doc block of the stretch is kept, as Doxygen
// concatenates them. A member-after `/**<` and a structural block
// (`@file`, `@class X`) stand alone. The rule reads kind names, which the
// four grammars share; see `lang_helpers::doc_comment`.
macro_rules! impl_doxygen_doc_binding {
    () => {
        const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
            skips: $crate::lang_helpers::doxygen::doxygen_skips,
            forward: $crate::lang_helpers::doxygen::doxygen_binds_forward::<Self>,
            gathers: Some(|node, code| {
                $crate::lang_helpers::doxygen::doxygen_standalone(
                    $crate::lang_helpers::doc_comment::node_bytes(node, code),
                )
            }),
            ..DocBinding::new(KeptDocs::All)
        });

        fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
            $crate::lang_helpers::doxygen::doxygen_target(node, ancestors)
        }
    };
}
