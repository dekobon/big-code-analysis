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
    fn compute<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        is_func_space: bool,
    ) {
        use Ruby as R;

        let start = init(node, stats, is_func_space);

        match node.kind_id().into() {
            R::Program => {}
            R::Comment => {
                add_comment_lines::<Self>(node, code, ancestors, stats);
                add_leading_doc_run::<Self>(node, code, ancestors, stats);
            }
            // A Ruby string literal (`"…"` / `'…'` / `%q{…}`) or a heredoc
            // body can span several rows; credit every spanned row to PLOC to
            // match Python's #415 decision (#778). A `%w[]` / `%i[]` array,
            // a regex, a quoted symbol (`:"…"`) and a backtick command are
            // literals of the same shape and own their rows the same way: an
            // empty row between an array's words is covered by no child, and
            // a `\` + newline inside the others is an `escape_sequence` (or,
            // in a non-interpolating array, the word's own `string_content`)
            // that starts on the row above, so the leaf credit alone left the
            // row to `blank` (#1816). The uppercase `%W` / `%I` spellings
            // parse to the same `string_array` / `symbol_array` kinds.
            R::String
            | R::HeredocBody
            | R::StringArray
            | R::SymbolArray
            | R::Regex
            | R::DelimitedSymbol
            | R::Subshell => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }
            // The `then` wrapper of an `if` / `elsif` / `unless` / `when` /
            // `in` / `rescue` body and the `do` wrapper of a loop body without
            // the keyword start where a comment parked ahead of the body's
            // first statement ends, so crediting their start row would turn
            // that comment row into code — a YARD doc row then stayed in MI's
            // size term (#1597). They spell no token of their own; the
            // statements and the `then` / `do` keywords inside them credit
            // their rows.
            R::Then | R::Do => {}
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

    // A `#` comment ends with its row: `ruby` runs the row after `# c \`.
    fn bare_rows() -> Option<BareRowPolicy> {
        Some(BareRowPolicy::Gap(BareRowComments {
            is_comment: crate::roles::ruby::comment::lists_kind,
            continued: false,
        }))
    }
}
