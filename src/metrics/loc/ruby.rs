//! `Loc` implementation for Ruby.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Loc for RubyCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        use Ruby as R;

        let (start, end) = init(node, stats, is_func_space);
        match node.kind_id().into() {
            R::Program => {}
            R::Comment => {
                add_cloc_lines(stats, start, end);
            }
            // A Ruby string literal (`"…"` / `'…'` / `%q{…}`) or a heredoc
            // body can span several rows; credit every spanned row to PLOC to
            // match Python's #415 decision (#778).
            R::String | R::HeredocBody => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }
            // LLOC contributors: control-flow constructs, method/class/module
            // declarations, postfix statement modifiers, and the dedicated
            // jump/redo/retry statement nodes. Assignment expressions and
            // ordinary method calls in expression-statement position are
            // intentionally NOT counted to avoid double-counting every
            // sub-expression: a single `a = b + c.d(e)` line would otherwise
            // contribute multiple LLOC. The Ruby grammar has no
            // `expression_statement` wrapper to disambiguate.
            R::If
            | R::Unless
            | R::Elsif
            | R::While
            | R::Until
            | R::For
            | R::Case
            | R::CaseMatch
            | R::Begin
            | R::IfModifier
            | R::UnlessModifier
            | R::WhileModifier
            | R::UntilModifier
            | R::RescueModifier
            | R::RescueModifier2
            | R::RescueModifier3
            | R::Return
            | R::Return2
            | R::Yield
            | R::Yield2
            | R::Break
            | R::Break2
            | R::Next
            | R::Next2
            | R::Redo
            | R::Retry
            | R::Method
            | R::SingletonMethod
            | R::Class
            | R::SingletonClass
            | R::Module
            | R::BeginBlock
            | R::EndBlock
            | R::Undef
            | R::Alias
            | R::EmptyStatement => {
                stats.lloc.count_logical_line();
            }
            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
