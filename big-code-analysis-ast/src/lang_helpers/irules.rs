//! iRules: the grammar's braced-word kind ids.

use crate::Irules;
use crate::getter::BracedWordKinds;

/// The twin of [`crate::lang_helpers::tcl::BRACED_WORD_KINDS`], at this
/// grammar's own id block. Every field means what the Tcl table
/// documents; only the ids differ.
pub(crate) const BRACED_WORD_KINDS: BracedWordKinds = BracedWordKinds {
    value: Irules::BracedWordSimple as u16,
    script: Irules::BracedWord as u16,
    comment: Irules::Comment as u16,
    command: Irules::Command as u16,
    word_list: Irules::WordList as u16,
    simple_word: Irules::SimpleWord as u16,
    argument: Irules::Argument as u16,
    open_brace: Irules::LBRACE as u16,
};
