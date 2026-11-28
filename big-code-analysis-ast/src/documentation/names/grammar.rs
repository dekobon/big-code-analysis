//! Exact supported signature grammar, not a compiler type resolver.
//!
//! Base types are one non-keyword identifier or one of BUILTINS below. Each base
//! permits const/volatile before or after it, then pointer stars with pointer cv,
//! then at most one final &/&&. Parameters may append one non-keyword identifier.
//! Empty parameters and sole void are supported. Arrays, nested declarators,
//! qualified/template types, attributes, defaults and trailing qualifiers defer.

use crate::documentation::SourceError;
use crate::documentation::budget::WorkBudget;

const BUILTINS: &[&[&[u8]]] = &[
    &[b"void"],
    &[b"bool"],
    &[b"_Bool"],
    &[b"char"],
    &[b"signed", b"char"],
    &[b"unsigned", b"char"],
    &[b"wchar_t"],
    &[b"char8_t"],
    &[b"char16_t"],
    &[b"char32_t"],
    &[b"short"],
    &[b"short", b"int"],
    &[b"signed", b"short"],
    &[b"signed", b"short", b"int"],
    &[b"unsigned", b"short"],
    &[b"unsigned", b"short", b"int"],
    &[b"int"],
    &[b"signed"],
    &[b"signed", b"int"],
    &[b"unsigned"],
    &[b"unsigned", b"int"],
    &[b"long"],
    &[b"long", b"int"],
    &[b"signed", b"long"],
    &[b"signed", b"long", b"int"],
    &[b"unsigned", b"long"],
    &[b"unsigned", b"long", b"int"],
    &[b"long", b"long"],
    &[b"long", b"long", b"int"],
    &[b"signed", b"long", b"long"],
    &[b"signed", b"long", b"long", b"int"],
    &[b"unsigned", b"long", b"long"],
    &[b"unsigned", b"long", b"long", b"int"],
    &[b"float"],
    &[b"double"],
    &[b"long", b"double"],
];

pub(super) fn identifier(token: &[u8]) -> bool {
    token
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && !keyword(token)
}

#[allow(clippy::too_many_lines)] // Explicit C/C++ keyword exclusions for the narrow grammar.
fn keyword(token: &[u8]) -> bool {
    matches!(
        token,
        b"alignas"
            | b"alignof"
            | b"and"
            | b"and_eq"
            | b"asm"
            | b"auto"
            | b"bitand"
            | b"bitor"
            | b"bool"
            | b"break"
            | b"case"
            | b"catch"
            | b"char"
            | b"char8_t"
            | b"char16_t"
            | b"char32_t"
            | b"class"
            | b"compl"
            | b"concept"
            | b"const"
            | b"consteval"
            | b"constexpr"
            | b"constinit"
            | b"const_cast"
            | b"continue"
            | b"co_await"
            | b"co_return"
            | b"co_yield"
            | b"decltype"
            | b"default"
            | b"delete"
            | b"do"
            | b"double"
            | b"dynamic_cast"
            | b"else"
            | b"enum"
            | b"explicit"
            | b"export"
            | b"extern"
            | b"false"
            | b"float"
            | b"for"
            | b"friend"
            | b"goto"
            | b"if"
            | b"inline"
            | b"int"
            | b"long"
            | b"mutable"
            | b"namespace"
            | b"new"
            | b"noexcept"
            | b"not"
            | b"not_eq"
            | b"nullptr"
            | b"operator"
            | b"or"
            | b"or_eq"
            | b"private"
            | b"protected"
            | b"public"
            | b"register"
            | b"reinterpret_cast"
            | b"requires"
            | b"return"
            | b"short"
            | b"signed"
            | b"sizeof"
            | b"static"
            | b"static_assert"
            | b"static_cast"
            | b"struct"
            | b"switch"
            | b"template"
            | b"this"
            | b"thread_local"
            | b"throw"
            | b"true"
            | b"try"
            | b"typedef"
            | b"typeid"
            | b"typename"
            | b"union"
            | b"unsigned"
            | b"using"
            | b"virtual"
            | b"void"
            | b"volatile"
            | b"wchar_t"
            | b"while"
            | b"xor"
            | b"xor_eq"
            | b"restrict"
            | b"_Alignas"
            | b"_Alignof"
            | b"_Atomic"
            | b"_Bool"
            | b"_Complex"
            | b"_Generic"
            | b"_Imaginary"
            | b"_Noreturn"
            | b"_Static_assert"
            | b"_Thread_local"
    )
}

fn cv<B: WorkBudget>(
    tokens: &[&[u8]],
    at: &mut usize,
    seen: &mut u8,
    budget: &B,
) -> Result<bool, SourceError> {
    while let Some(token) = tokens.get(*at) {
        budget.step().map_err(Into::into)?;
        let bit = match *token {
            b"const" => 1,
            b"volatile" => 2,
            _ => break,
        };
        if *seen & bit != 0 {
            return Ok(false);
        }
        *seen |= bit;
        *at += 1;
    }
    Ok(true)
}

fn type_end<B: WorkBudget>(
    tokens: &[&[u8]],
    budget: &B,
) -> Result<Option<(usize, bool)>, SourceError> {
    let mut at = 0;
    let mut seen = 0;
    if !cv(tokens, &mut at, &mut seen, budget)? {
        return Ok(None);
    }
    let Some(&base) = tokens.get(at) else {
        return Ok(None);
    };
    let start = at;
    if identifier(base) {
        at += 1;
    } else {
        while tokens.get(at).is_some_and(|token| {
            matches!(
                *token,
                b"void"
                    | b"bool"
                    | b"_Bool"
                    | b"char"
                    | b"wchar_t"
                    | b"char8_t"
                    | b"char16_t"
                    | b"char32_t"
                    | b"short"
                    | b"signed"
                    | b"unsigned"
                    | b"int"
                    | b"long"
                    | b"float"
                    | b"double"
            )
        }) {
            budget.step().map_err(Into::into)?;
            at += 1;
            if at - start > 4 {
                return Ok(None);
            }
        }
        if !BUILTINS.contains(&&tokens[start..at]) {
            return Ok(None);
        }
    }
    if !cv(tokens, &mut at, &mut seen, budget)? {
        return Ok(None);
    }
    let mut pointer = false;
    while tokens.get(at) == Some(&b"*".as_slice()) {
        budget.step().map_err(Into::into)?;
        pointer = true;
        at += 1;
        if !cv(tokens, &mut at, &mut 0, budget)? {
            return Ok(None);
        }
    }
    if tokens
        .get(at)
        .is_some_and(|token| matches!(*token, b"&" | b"&&"))
    {
        at += 1;
        if base == b"void" && !pointer {
            return Ok(None);
        }
    }
    Ok(Some((at, base == b"void" && !pointer)))
}

pub(super) fn return_type<B: WorkBudget>(
    tokens: &[&[u8]],
    budget: &B,
) -> Result<bool, SourceError> {
    Ok(type_end(tokens, budget)?.is_some_and(|(at, _)| at == tokens.len()))
}

pub(super) fn parameters<B: WorkBudget>(tokens: &[&[u8]], budget: &B) -> Result<bool, SourceError> {
    if tokens.is_empty() || tokens == [b"void".as_slice()] {
        return Ok(true);
    }
    let mut start = 0;
    for end in 0..=tokens.len() {
        budget.step().map_err(Into::into)?;
        if end < tokens.len() && tokens[end] != b"," {
            continue;
        }
        let group = &tokens[start..end];
        let Some((at, void)) = type_end(group, budget)? else {
            return Ok(false);
        };
        if void || !(at == group.len() || (at + 1 == group.len() && identifier(group[at]))) {
            return Ok(false);
        }
        start = end + 1;
    }
    Ok(true)
}
