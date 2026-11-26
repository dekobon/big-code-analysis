//! Private fallible counterparts for public infallible source-recognition hooks.

use std::marker::PhantomData;

use super::budget::WorkBudget;
use super::streaming::{
    markers::CandidateView,
    positional::{OpenerReason, RunContext},
};
use super::{SourceContext, UnresolvedReason};
use crate::{Ancestors, Checker, Node};

crate::observation::counter!(target_queries, cfg(any(test, feature = "test-support")));
crate::observation::counter!(
    target_computations,
    cfg(any(test, feature = "test-support"))
);

pub(crate) trait SourceRules {
    type Checker: Checker;
    fn separately_assessed_trivia(_node: &Node<'_>) -> bool {
        false
    }
    fn start_row<B: WorkBudget>(node: &Node<'_>, budget: &B) -> Result<usize, B::Error>;
    fn marker<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error>;

    fn target<'s, B: WorkBudget>(
        node: &Node<'s>,
        code: &[u8],
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<bool, B::Error>;
    fn forward<'s, B: WorkBudget>(
        node: &Node<'s>,
        context: &SourceContext<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<Option<bool>, B::Error>;
    fn skips<'s, B: WorkBudget>(
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<bool, B::Error>;
    fn gathers<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error>;
    fn opener<'s, B: WorkBudget>(
        run: &RunContext<'_, 's>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<Result<Option<OpenerReason>, UnresolvedReason>, B::Error>;
    fn directive<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B)
    -> Result<bool, B::Error>;
    fn ends<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error>;
}

pub(crate) struct PublicRules<T>(PhantomData<T>);
pub(crate) struct NativeRules<T>(PhantomData<T>);

impl<T: Checker + crate::traits::LanguageInfo> SourceRules for NativeRules<T> {
    type Checker = T;
    fn separately_assessed_trivia(node: &Node<'_>) -> bool {
        // Attribute interiors have their own occurrence/content assessment; only intervening gaps break binding.
        T::lang() == crate::LANG::Rust && node.kind() == "attribute_item"
    }
    fn start_row<B: WorkBudget>(node: &Node<'_>, budget: &B) -> Result<usize, B::Error> {
        if T::lang() == crate::LANG::Ruby {
            crate::checker::ruby::start_row_with_budget(node, budget)
        } else {
            Ok(T::doc_target_start_row(node))
        }
    }
    fn marker<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error> {
        if T::lang() == crate::LANG::Rust && T::is_comment(node) {
            crate::roles::rust::doc_marker_with_budget(node, budget)
        } else {
            Ok(T::is_source_doc_comment(node, code))
        }
    }

    fn target<'s, B: WorkBudget>(
        node: &Node<'s>,
        code: &[u8],
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<bool, B::Error> {
        target_queries::record();
        ancestors.try_memoised(crate::node::MemoQuestion::SourceDocTarget, node, || {
            target_computations::record();
            match T::lang() {
                crate::LANG::Csharp if super::collector::conditional_kind(node.kind()) => Ok(true),
                crate::LANG::C => crate::lang_helpers::doxygen::doxygen_target_with_budget::<
                    crate::CCode,
                    _,
                >(node, code, ancestors, budget),
                crate::LANG::Cpp => crate::lang_helpers::doxygen::doxygen_target_with_budget::<
                    crate::CppCode,
                    _,
                >(node, code, ancestors, budget),
                crate::LANG::Mozcpp => crate::lang_helpers::doxygen::doxygen_target_with_budget::<
                    crate::MozcppCode,
                    _,
                >(node, code, ancestors, budget),
                crate::LANG::Objc => crate::lang_helpers::doxygen::doxygen_target_with_budget::<
                    crate::ObjcCode,
                    _,
                >(node, code, ancestors, budget),
                crate::LANG::Go => {
                    crate::roles::go::doc_target_with_budget(node, ancestors, budget)
                }
                crate::LANG::Ruby => crate::checker::ruby::ruby_doc_target_with_budget(
                    node, code, ancestors, true, budget,
                ),
                crate::LANG::Lua => crate::checker::lua::target_with_budget(node, budget),
                crate::LANG::Php => {
                    crate::roles::php::target_with_budget(node, code, ancestors, budget)
                }
                crate::LANG::Javascript => {
                    crate::roles::javascript::doc_target_with_budget(node, code, ancestors, budget)
                }
                crate::LANG::Mozjs => {
                    crate::roles::mozjs::doc_target_with_budget(node, code, ancestors, budget)
                }
                crate::LANG::Typescript | crate::LANG::Tsx
                    if ancestors.parent(node).is_some_and(|parent| {
                        matches!(parent.kind(), "export_statement" | "ambient_declaration")
                    }) =>
                {
                    Ok(false)
                }
                crate::LANG::Typescript => {
                    crate::roles::typescript::doc_target_with_budget(node, code, ancestors, budget)
                }
                crate::LANG::Tsx => {
                    crate::roles::tsx::doc_target_with_budget(node, code, ancestors, budget)
                }
                _ => PublicRules::<T>::target(node, code, ancestors, budget),
            }
        })
    }
    fn forward<'s, B: WorkBudget>(
        node: &Node<'s>,
        context: &SourceContext<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<Option<bool>, B::Error> {
        let bytes = crate::lang_helpers::doc_comment::node_bytes(node, context.bytes());
        match T::lang() {
            crate::LANG::C => Ok(Some(
                crate::lang_helpers::doxygen::doxygen_forward_with_budget::<crate::CCode, _>(
                    node,
                    context.bytes(),
                    ancestors,
                    budget,
                )?,
            )),
            crate::LANG::Cpp => Ok(Some(
                crate::lang_helpers::doxygen::doxygen_forward_with_budget::<crate::CppCode, _>(
                    node,
                    context.bytes(),
                    ancestors,
                    budget,
                )?,
            )),
            crate::LANG::Mozcpp => {
                Ok(Some(
                    crate::lang_helpers::doxygen::doxygen_forward_with_budget::<
                        crate::MozcppCode,
                        _,
                    >(node, context.bytes(), ancestors, budget)?,
                ))
            }
            crate::LANG::Objc => Ok(Some(
                crate::lang_helpers::doxygen::doxygen_forward_with_budget::<crate::ObjcCode, _>(
                    node,
                    context.bytes(),
                    ancestors,
                    budget,
                )?,
            )),
            crate::LANG::Php => {
                crate::checker::php::source_forward_with_budget(node, context, ancestors, budget)
            }
            crate::LANG::Javascript | crate::LANG::Mozjs => Ok(Some(
                !crate::lang_helpers::doc_text::jsdoc_virtual_with_budget(bytes, budget)?,
            )),
            crate::LANG::Typescript | crate::LANG::Tsx => Ok(Some(
                !crate::lang_helpers::doc_text::tsdoc_module_with_budget(bytes, budget)?,
            )),
            _ => PublicRules::<T>::forward(node, context, ancestors, budget),
        }
    }
    fn skips<'s, B: WorkBudget>(
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<bool, B::Error> {
        match T::lang() {
            crate::LANG::C => crate::lang_helpers::doxygen::doxygen_skips_with_budget::<
                crate::CCode,
                _,
            >(node, ancestors, budget),
            crate::LANG::Cpp => crate::lang_helpers::doxygen::doxygen_skips_with_budget::<
                crate::CppCode,
                _,
            >(node, ancestors, budget),
            crate::LANG::Mozcpp => crate::lang_helpers::doxygen::doxygen_skips_with_budget::<
                crate::MozcppCode,
                _,
            >(node, ancestors, budget),
            crate::LANG::Objc => crate::lang_helpers::doxygen::doxygen_skips_with_budget::<
                crate::ObjcCode,
                _,
            >(node, ancestors, budget),
            _ => PublicRules::<T>::skips(node, ancestors, budget),
        }
    }
    fn gathers<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error> {
        if matches!(
            T::lang(),
            crate::LANG::C | crate::LANG::Cpp | crate::LANG::Mozcpp | crate::LANG::Objc
        ) {
            crate::lang_helpers::doxygen::doxygen_standalone_with_budget(
                crate::lang_helpers::doc_comment::node_bytes(node, code),
                budget,
            )
        } else {
            PublicRules::<T>::gathers(node, code, budget)
        }
    }
    fn opener<'s, B: WorkBudget>(
        run: &RunContext<'_, 's>,
        ancestors: Ancestors<'s, '_>,
        budget: &B,
    ) -> Result<Result<Option<OpenerReason>, UnresolvedReason>, B::Error> {
        match T::lang() {
            crate::LANG::Ruby => Ok(Ok(crate::checker::ruby::source_opener_with_budget(
                run, ancestors, budget,
            )?)),
            crate::LANG::Lua => {
                crate::checker::lua::source_opener_with_budget(run, ancestors, budget)
            }
            _ => PublicRules::<T>::opener(run, ancestors, budget),
        }
    }
    fn directive<B: WorkBudget>(
        node: &Node<'_>,
        code: &[u8],
        budget: &B,
    ) -> Result<bool, B::Error> {
        let bytes = crate::lang_helpers::doc_comment::node_bytes(node, code);
        match T::lang() {
            crate::LANG::Go => {
                crate::lang_helpers::doc_text::go_directive_with_budget(bytes, budget)
            }
            crate::LANG::Ruby => {
                crate::lang_helpers::doc_text::ruby_magic_comment_with_budget(bytes, budget)
            }
            _ => PublicRules::<T>::directive(node, code, budget),
        }
    }
    fn ends<B: WorkBudget>(node: &Node<'_>, code: &[u8], budget: &B) -> Result<bool, B::Error> {
        if T::lang() == crate::LANG::Lua && T::is_comment(node) {
            crate::lang_helpers::doc_text::lua_long_comment_with_budget(
                crate::lang_helpers::doc_comment::node_bytes(node, code),
                budget,
            )
        } else {
            PublicRules::<T>::ends(node, code, budget)
        }
    }
}

impl<T: Checker> SourceRules for PublicRules<T> {
    type Checker = T;
    fn start_row<B: WorkBudget>(node: &Node<'_>, _: &B) -> Result<usize, B::Error> {
        Ok(T::doc_target_start_row(node))
    }
    fn marker<B: WorkBudget>(node: &Node<'_>, code: &[u8], _: &B) -> Result<bool, B::Error> {
        Ok(T::is_source_doc_comment(node, code))
    }

    fn target<'s, B: WorkBudget>(
        node: &Node<'s>,
        code: &[u8],
        ancestors: Ancestors<'s, '_>,
        _: &B,
    ) -> Result<bool, B::Error> {
        Ok(T::is_source_doc_target(node, code, ancestors))
    }
    fn forward<'s, B: WorkBudget>(
        node: &Node<'s>,
        context: &SourceContext<'s>,
        ancestors: Ancestors<'s, '_>,
        _: &B,
    ) -> Result<Option<bool>, B::Error> {
        Ok(T::source_doc_binds_forward(node, context, ancestors))
    }
    fn skips<'s, B: WorkBudget>(
        node: &Node<'s>,
        ancestors: Ancestors<'s, '_>,
        _: &B,
    ) -> Result<bool, B::Error> {
        Ok(T::source_doc_binding().is_some_and(|binding| (binding.skips)(node, ancestors)))
    }
    fn gathers<B: WorkBudget>(node: &Node<'_>, code: &[u8], _: &B) -> Result<bool, B::Error> {
        Ok(T::source_doc_binding()
            .and_then(|binding| binding.gathers)
            .is_some_and(|gathers| gathers(node, code)))
    }
    fn opener<'s, B: WorkBudget>(
        run: &RunContext<'_, 's>,
        ancestors: Ancestors<'s, '_>,
        _: &B,
    ) -> Result<Result<Option<OpenerReason>, UnresolvedReason>, B::Error> {
        Ok(T::source_doc_run_opener(run, ancestors))
    }
    fn directive<B: WorkBudget>(node: &Node<'_>, code: &[u8], _: &B) -> Result<bool, B::Error> {
        Ok(T::is_doc_run_directive(node, code))
    }
    fn ends<B: WorkBudget>(node: &Node<'_>, code: &[u8], _: &B) -> Result<bool, B::Error> {
        Ok(T::ends_doc_run(node, code))
    }
}

pub(crate) fn source_target<'s, R: SourceRules, B: WorkBudget>(
    view: CandidateView<'_>,
    node: &Node<'s>,
    code: &[u8],
    ancestors: Ancestors<'s, '_>,
    budget: &B,
) -> Result<bool, B::Error> {
    budget.step()?;
    match view {
        CandidateView::Legacy => Ok(R::Checker::is_doc_target(node, code, ancestors)),
        CandidateView::Source(_)
            if view.covers_with_budget(node.start_byte()..node.end_byte(), budget)? =>
        {
            R::target(node, code, ancestors, budget)
        }
        CandidateView::Source(_) => Ok(false),
    }
}
