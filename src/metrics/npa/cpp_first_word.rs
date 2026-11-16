//! What the C++ declarator reader (`cpp_declarators`) takes the first
//! word of a recovered declarator for: the declarator's own name, which
//! makes a class-scope macro call the method it expands to, or a word
//! nothing settles, which leaves the declarator to the reading of the
//! recovered member pointer. Both consumers ask through
//! `cpp_is_named_function`. Matches on node-kind names and on source
//! text, so it serves the upstream `Cpp` grammar and the Mozilla
//! `Mozcpp` fork alike.

use super::cpp_declarators::cpp_is_innermost_level;
use crate::node::Node;

// What the reader takes the word before a member's first parameter list
// for. The recovery of a parenthesised member pointer reads the
// member's type as that word, so where the word is taken for the
// declarator's own name, a declarator that is the word and one
// parameter list is read as the function it spells and no recovery:
// `int DECL(S::*p);` and `[[noreturn]] FAIL(S::*p);` are the methods
// their macros expand to. Both consumers ask `cpp_is_named_function`,
// so neither reads such a declarator as data. The evidence is strong
// and no proof: each variant names a spelling it misreads.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum FirstWord {
    // Taken for the declarator's name. The declaration writes a type
    // the reader takes for nothing else (a keyword type, a standard
    // typedef name such as `size_t`, a qualified or template name, an
    // elaborated or dependent type, `auto`, `decltype(…)`), or opens
    // with the standard attribute `[[noreturn]]`. An annotation macro
    // between such a type and a parenthesised pointer to data member is
    // the same tree, and `int MOZ_X (S::*pd);` reads as a method
    // (pinned, FIXME(#1829)).
    Name,
    // Nothing settles it. No type is written, or one identifier that
    // is neither a type keyword nor a standard typedef name, which may
    // be an annotation macro before the real type: `MOZ_X T (S::*pd);`
    // is a data member whose type `T` the grammars read as the name.
    // The macro call `T DECL(S::*p);` is the same tree and reads as
    // that data member (pinned, FIXME(#1829)).
    Unsettled,
}

pub(super) fn cpp_first_word<'a>(declaration: &Node<'a>, code: &'a [u8]) -> FirstWord {
    // Only a recovered declarator asks, and a declaration with no parse
    // error holds none: its type and attributes are then not read.
    if !declaration.has_error() {
        return FirstWord::Unsettled;
    }
    let writes_a_type =
        declaration
            .child_by_field_name("type")
            .is_some_and(|written| match written.kind() {
                // One word: a type only when the word is a keyword or a
                // standard typedef name. The kind does not say. The Mozcpp
                // fork lists the Windows typedef names (`DWORD`) and the
                // macro `APIENTRY` as `primitive_type`, and tree-sitter-cpp
                // reads `wchar_t` and `intmax_t` as a `type_identifier`.
                // role-bypass: allow(DeclKeyword, Primitive) — the one word in a
                // C++ type slot, which Groovy's rows claim by the same spelling
                "type_identifier" | "primitive_type" => {
                    let word = written.utf8_text(code);
                    cpp_is_type_keyword(word) || cpp_is_standard_typedef_name(word)
                }
                _ => true,
            });
    if writes_a_type || cpp_opens_with_standard_noreturn(declaration, code) {
        FirstWord::Name
    } else {
        FirstWord::Unsettled
    }
}

// Whether a declaration opens with a `[[…]]` list holding `noreturn`
// with no namespace before it. The standard gives that attribute to a
// function's declaration and to nothing else ([dcl.attr.noreturn]).
// Only the lists that open the declaration are read: they apply to
// what it declares, where a list after a specifier applies to the
// type, and one after the declarator's name is not looked for. The
// attribute does not make every declaration under it a function: GCC
// accepts it on a pointer to function, which is data (`[[noreturn]]
// void (*fail)(int);`; clang rejects that), so it settles only what the
// first word of a recovered declarator is. There it takes one spelling
// only GCC accepts with the call: `[[noreturn]] T (S::*pd);`, a data
// member under an attribute GCC warns that it ignores, reads as a
// method (pinned, FIXME(#1829)). Under a namespace it settles
// nothing: GCC and clang accept `[[gnu::noreturn]] T (S::*pm)(int);`, a
// data member. The GNU and Microsoft spellings
// (`__attribute__((noreturn))`, `__declspec(noreturn)`) are other node
// kinds and are not read.
fn cpp_opens_with_standard_noreturn<'a>(declaration: &Node<'a>, code: &'a [u8]) -> bool {
    declaration
        .children()
        // role-bypass: allow(Comment) — stepping over a comment between two
        // attribute lists; not whether the node is one
        .filter(|child| child.kind() != "comment")
        .take_while(|child| child.kind() == "attribute_declaration")
        // A list the grammars could not read says nothing: they leave
        // the `using gnu:` of `[[using gnu: unused, noreturn]]` in an
        // `ERROR` and its attributes with no namespace.
        .filter(|attributes| !attributes.has_error())
        .flat_map(|attributes| attributes.children())
        .any(|attribute| {
            attribute.child_by_field_name("prefix").is_none()
                && attribute
                    .child_by_field_name("name")
                    .is_some_and(|name| name.utf8_text(code) == Some("noreturn"))
        })
}

// Whether a `function_declarator` with no parenthesised name is read as
// the function its first word names: the word is taken for a name, it
// is no word of a type (`cpp_is_type_word`), and the declarator is that
// word and one parameter list. A declarator that nests another is left
// to the recovered chain, which read it before the first word was
// asked: the second parameter list is a pointee's (`int MOZ_X
// (S::*pf)(int);`, a pointer to member function after an annotation
// macro, is data), or the levels are a comma list's (`[[noreturn]] T
// (S::*pa), (S::*pb);`, which GCC accepts as two data members). So is
// one with an `ERROR` of its own: nothing stands between a function's
// name and its parameter list, and `int MOZ_X long (S::*pq);` leaves
// the `long` of its type there, stranded after the macro the grammars
// took for the name.
pub(super) fn cpp_is_named_function<'a>(
    declarator: &Node<'a>,
    first_word: FirstWord,
    code: &'a [u8],
) -> bool {
    first_word == FirstWord::Name
        && cpp_is_innermost_level(declarator)
        && declarator.children().all(|part| !part.is_error())
        && !cpp_is_type_word(
            declarator
                .child_by_field_name("declarator")
                .and_then(|word| word.utf8_text(code)),
        )
}

// The words only a fundamental type spells.
const CPP_TYPE_KEYWORDS: [&str; 14] = [
    "bool", "char", "char8_t", "char16_t", "char32_t", "wchar_t", "short", "int", "long", "signed",
    "unsigned", "float", "double", "void",
];

// Whether a word is a type keyword, whichever node kind a grammar gives
// it.
fn cpp_is_type_keyword(word: Option<&str>) -> bool {
    word.is_some_and(|word| CPP_TYPE_KEYWORDS.contains(&word))
}

// Seventeen typedef names the standard library declares: those of
// `<cstddef>`, the exact-width, pointer-width and greatest-width
// integers of `<cstdint>` (its `least` and `fast` families are not
// listed), and POSIX `ssize_t`. None is a keyword, so the spelling is
// evidence and no proof: a program that defines one as an annotation
// macro has `size_t T (S::*pd);` read as a method. Asked of the type
// slot only. `cpp_is_type_word` does not list them: no data member was
// found that needs one read as a type's last word, and before a
// parameter list such a word can be a macro's name.
const CPP_STANDARD_TYPEDEF_NAMES: [&str; 17] = [
    "size_t",
    "ssize_t",
    "ptrdiff_t",
    "nullptr_t",
    "max_align_t",
    "intptr_t",
    "uintptr_t",
    "intmax_t",
    "uintmax_t",
    "int8_t",
    "int16_t",
    "int32_t",
    "int64_t",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
];

fn cpp_is_standard_typedef_name(word: Option<&str>) -> bool {
    word.is_some_and(|word| CPP_STANDARD_TYPEDEF_NAMES.contains(&word))
}

// A word that is part of a declaration's type and so names no function:
// a type keyword, `typedef`, or an identifier the implementation
// reserves ([lex.name]: two leading underscores, or one and a capital),
// which is how the extension types and qualifiers are spelled
// (`__int128`, `_Complex`, `__const`). The grammars give a type's last
// word to the declarator when a qualifier, a specifier or a word they
// do not know splits it from the first: `unsigned const int (S::*pd);`
// is a `function_declarator` named `int` after the type `unsigned`.
pub(super) fn cpp_is_type_word(word: Option<&str>) -> bool {
    cpp_is_type_keyword(word)
        || word.is_some_and(|word| {
            word == "typedef"
                || word.starts_with("__")
                || word
                    .strip_prefix('_')
                    .and_then(|rest| rest.chars().next())
                    .is_some_and(|second| second.is_ascii_uppercase())
        })
}
