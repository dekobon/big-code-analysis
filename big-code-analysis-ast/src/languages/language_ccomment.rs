// See `src/languages/mod.rs` for the rationale behind the per-file
// pedantic carve-outs below.
#![allow(clippy::match_same_arms, clippy::too_many_lines)]
// One variant per grammar kind id, named after the rule; the name is
// the documentation and there is nothing per-variant to add.
#![allow(missing_docs)]

// Code generated; DO NOT EDIT.

use num_derive::FromPrimitive;

#[derive(Clone, Debug, PartialEq, Eq, FromPrimitive)]
pub enum Ccomment {
    End = 0,
    Nothing = 1,
    PreprocContinuationLine = 2,
    PreprocLine = 3,
    DefineToken1 = 4,
    StringLiteralToken1 = 5,
    CharLiteralToken1 = 6,
    Comment = 7,
    RawStringLiteral = 8,
    TranslationUnit = 9,
    TopLevelItem = 10,
    Define = 11,
    StringLiteral = 12,
    CharLiteral = 13,
    TranslationUnitRepeat1 = 14,
    DefineRepeat1 = 15,
    Error = 16,
}

/// Each runtime kind id of `Ccomment` with tree-sitter's `named` and
/// `visible` flags, in id order. A label for the id the enum identifies,
/// read by the role-coverage gate; the generator's ERROR sentinel has no
/// entry.
pub const CCOMMENT_KIND_FLAGS: &[(u16, bool, bool)] = &[
    (0, false, false),
    (1, true, true),
    (2, true, true),
    (3, true, true),
    (4, false, false),
    (5, false, false),
    (6, false, false),
    (7, true, true),
    (8, true, true),
    (9, true, true),
    (10, false, false),
    (11, true, true),
    (12, true, true),
    (13, true, true),
    (14, false, false),
    (15, false, false),
];

impl From<Ccomment> for &'static str {
    #[inline]
    fn from(tok: Ccomment) -> Self {
        match tok {
            Ccomment::End => "end",
            Ccomment::Nothing => "nothing",
            Ccomment::PreprocContinuationLine => "preproc_continuation_line",
            Ccomment::PreprocLine => "preproc_line",
            Ccomment::DefineToken1 => "define_token1",
            Ccomment::StringLiteralToken1 => "string_literal_token1",
            Ccomment::CharLiteralToken1 => "char_literal_token1",
            Ccomment::Comment => "comment",
            Ccomment::RawStringLiteral => "raw_string_literal",
            Ccomment::TranslationUnit => "translation_unit",
            Ccomment::TopLevelItem => "_top_level_item",
            Ccomment::Define => "define",
            Ccomment::StringLiteral => "string_literal",
            Ccomment::CharLiteral => "char_literal",
            Ccomment::TranslationUnitRepeat1 => "translation_unit_repeat1",
            Ccomment::DefineRepeat1 => "define_repeat1",
            Ccomment::Error => "ERROR",
        }
    }
}

impl From<u16> for Ccomment {
    #[inline]
    fn from(x: u16) -> Self {
        num_traits::FromPrimitive::from_u16(x).unwrap_or(Self::Error)
    }
}

// Ccomment == u16
impl PartialEq<u16> for Ccomment {
    #[inline]
    fn eq(&self, x: &u16) -> bool {
        *self == Into::<Self>::into(*x)
    }
}

// u16 == Ccomment
impl PartialEq<Ccomment> for u16 {
    #[inline]
    fn eq(&self, x: &Ccomment) -> bool {
        *x == *self
    }
}
