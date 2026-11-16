//! Clang's thread-safety annotations after a C++ class member's
//! declarator (`int count_ GUARDED_BY(mu_);`, `void Loop()
//! REQUIRES(mu_);`). Each expands to an attribute or to nothing, and
//! the grammars read each as a function's name and parameter list. Two
//! readings undo that. Where the grammars keep the annotation in the
//! member's declaration, the declarator reader (`cpp_declarators`) asks
//! `cpp_annotated` what it annotates. Where they end the member before
//! it, the two member walks ask `CppTrailingAnnotations` whether the
//! next member is that annotation. Matches on node-kind names and on
//! source text, so it serves the upstream `Cpp` grammar and the Mozilla
//! `Mozcpp` fork alike.

use super::cpp_declarators::cpp_is_cut_off;
use super::cpp_first_word::cpp_is_type_word;
use crate::node::Node;

// The macros Clang's Thread Safety Analysis page defines that take an
// argument list and follow a member's declarator, under the names it
// documents and the ones it lists as their previous spellings
// (<https://clang.llvm.org/docs/ThreadSafetyAnalysis.html>). The page
// gives the first four to data members and the rest to functions, and
// a pointer to function takes those too, so the name does not say
// which the member is. `CAPABILITY(…)` annotates a class and is not
// listed. Neither are the annotations that take no argument
// (`NO_THREAD_SAFETY_ANALYSIS`, `GUARDED_VAR`): the grammars read one
// as a name and no parameter list, which adds no method.
const CPP_THREAD_SAFETY_ANNOTATIONS: [&str; 28] = [
    "GUARDED_BY",
    "PT_GUARDED_BY",
    "ACQUIRED_BEFORE",
    "ACQUIRED_AFTER",
    "REQUIRES",
    "REQUIRES_SHARED",
    "ACQUIRE",
    "ACQUIRE_SHARED",
    "RELEASE",
    "RELEASE_SHARED",
    "RELEASE_GENERIC",
    "TRY_ACQUIRE",
    "TRY_ACQUIRE_SHARED",
    "EXCLUDES",
    "ASSERT_CAPABILITY",
    "ASSERT_SHARED_CAPABILITY",
    "RETURN_CAPABILITY",
    "EXCLUSIVE_LOCKS_REQUIRED",
    "SHARED_LOCKS_REQUIRED",
    "EXCLUSIVE_LOCK_FUNCTION",
    "SHARED_LOCK_FUNCTION",
    "UNLOCK_FUNCTION",
    "EXCLUSIVE_TRYLOCK_FUNCTION",
    "SHARED_TRYLOCK_FUNCTION",
    "ASSERT_EXCLUSIVE_LOCK",
    "ASSERT_SHARED_LOCK",
    "LOCKS_EXCLUDED",
    "LOCK_RETURNED",
];

// Whether a word spells one of them: a listed name, alone or after a
// prefix that ends in `_`, which is how a library names its own
// (`ABSL_GUARDED_BY`, `MOZ_REQUIRES`, `RTC_EXCLUSIVE_LOCKS_REQUIRED`,
// `MOZ_CAPABILITY_ACQUIRE`). The word is capitals, digits and
// underscores throughout, as a macro's name is. The spelling is
// evidence and no proof: `NS_IMPL_RELEASE` spells one too, and a method
// may be named `REQUIRES`. Each reader below therefore asks for the
// tree an annotation leaves as well, and a function named by such a
// word in any other tree keeps the reading it had. A library's
// spelling that is no listed name is not taken: googletest's
// `GTEST_LOCK_EXCLUDED_`, Chromium's `GUARDED_BY_CONTEXT`.
fn cpp_is_thread_safety_annotation(word: &str) -> bool {
    word.bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && CPP_THREAD_SAFETY_ANNOTATIONS.iter().any(|name| {
            word.strip_suffix(name)
                .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with('_'))
        })
}

// Where the declarator is that a trailing annotation annotates. The
// grammars take the annotation for the member's `function_declarator`,
// which then says nothing of the member.
pub(super) enum Annotated<'a> {
    // In this `ERROR` directly before the annotation, whose children
    // say whether the member is a function: `bool mDone
    // MOZ_GUARDED_BY(mu);` leaves the name `mDone` there, `int mArr[4]
    // GUARDED_BY(mu);` an `array_declarator`, and `void Loop()
    // MOZ_REQUIRES(mu);` and `void (*mCb)(int) GUARDED_BY(mu);` each a
    // `function_declarator`, which is a method's or a pointer's.
    Stranded(Node<'a>),
    // Not in an `ERROR` of its own, and nothing to ask. It is the
    // sibling before the annotation, which the reader asked before it:
    // a second annotation leaves the first one in the `ERROR` beside
    // the declarator (`int x GUARDED_BY(mu) ACQUIRED_AFTER(mu);`). Or
    // it is a name the member's type took, which is data: after `long`
    // or `unsigned` the grammars read the name as that type's last
    // word (`long mSize GUARDED_BY(mu);` is the type `long mSize`). A
    // function's name never gets there, because its parameter list
    // keeps it a declarator.
    Elsewhere,
}

// What the `function_declarator` of a member annotates, and `None` for
// one that is the member's own declarator. `previous` is the sibling
// before it, comments left out. Two things make it an annotation: its
// name spells one, and a declarator stands before it that the grammars
// could not join to it. Where nothing stands before it that could name
// a member, it is the declarator itself: `int REQUIRES(int x);` is a
// method named `REQUIRES`, and so is `MOZ_CAN_RUN_SCRIPT void
// RELEASE(int a);`, whose `ERROR` holds only the type keyword the
// macro before it split off. A type that is no keyword cannot be told
// from a name there: `MOZ_X T REQUIRES(int a);`, a method named
// `REQUIRES` after an annotation macro, is the tree of the data member
// `T mDone REQUIRES(mu);` and reads as that data member (pinned,
// FIXME(#1904)).
pub(super) fn cpp_annotated<'a>(
    declarator: &Node<'a>,
    previous: Option<&Node<'a>>,
    code: &[u8],
) -> Option<Annotated<'a>> {
    let previous = previous?;
    let annotated = if previous.is_error() {
        previous
            .children()
            .any(|part| cpp_is_declarator_part(&part, code))
            .then_some(Annotated::Stranded(*previous))?
    } else if cpp_is_declarator_part(previous, code) || cpp_type_took_a_name(previous) {
        Annotated::Elsewhere
    } else {
        return None;
    };
    declarator
        .child_by_field_name("declarator")
        .and_then(|name| name.utf8_text(code))
        .is_some_and(cpp_is_thread_safety_annotation)
        .then_some(annotated)
}

// Whether a node the grammars left before an annotation is a
// declarator: a name, which there is an `identifier` that spells no
// word of a type, or the declarator of an array or of a function. A
// pointer or a reference is not looked for: its `*` or `&` takes the
// annotation in with the name, so the `ERROR` and the annotation both
// stand inside it.
fn cpp_is_declarator_part(node: &Node, code: &[u8]) -> bool {
    match node.kind() {
        "identifier" => !cpp_is_type_word(node.utf8_text(code)),
        "function_declarator" | "array_declarator" => true,
        _ => false,
    }
}

// Whether a `sized_type_specifier` ends in a word that is no keyword:
// the member's name, read as the type's last word. A word before the
// keywords names nothing: it is an annotation macro the grammars read
// into the type, and `MOZ_X long RELEASE();` is a method.
fn cpp_type_took_a_name(node: &Node) -> bool {
    node.kind() == "sized_type_specifier"
        && node
            .children()
            .last()
            // role-bypass: allow(DeclKeyword, Primitive) — the name a C++
            // sized type swallowed, which Groovy's rows claim by the same
            // spelling
            .is_some_and(|word| word.kind() == "type_identifier")
}

// What the two member walks carry from one member of a class body to
// the next: whether the member before ended in a `;` the grammars
// invented. They invent one where they cannot join an annotation to
// the declarator before it, which is most of the time: after a name
// longer than a few letters, after `const`, `noexcept` or `override`,
// after a constructor's parameter list, and before an argument that is
// no plain name (`MOZ_GUARDED_BY(*mMonitor)`). The annotation then
// follows as a member of its own: a constructor's declaration by its
// tree, or a member whose type is the annotation's name. Counting that
// member read `bool mIsWriteScheduled MOZ_GUARDED_BY(mu);` as a data
// member and a method, `void Loop() const MOZ_REQUIRES(mu);` as two
// methods and `nsresult mStatus MOZ_GUARDED_BY(*mMutex) = NS_OK;` as
// two data members (#1904, #1932). One state type for both walks, so
// neither counts a member the other skips.
#[derive(Default)]
pub(crate) struct CppTrailingAnnotations {
    after_cut_off: bool,
}

impl CppTrailingAnnotations {
    // Whether `member` is the annotation of the member before it and
    // no member itself. Asked once of every child of a class body, in
    // order. An annotation that is cut off in turn hands the state on,
    // so a second one after it is claimed as well (`void AssertSame()
    // MOZ_REQUIRES(*mMutex) MOZ_ASSERT_CAPABILITY(*aMutex) {}`, where
    // the last carries the method's body). The whole member is claimed,
    // and with it what the grammars read into it after the annotation:
    // a declarator after a comma is lost (`int mLongNameA
    // GUARDED_BY(*mu), mLongNameB GUARDED_BY(*mu);` loses `mLongNameB`,
    // pinned, FIXME(#1904)). The evidence is the name and the invented
    // `;`. Anything named as an annotation is that follows a cut-off
    // member has both: `HRESULT STDMETHODCALLTYPE RELEASE();`, a method
    // named `RELEASE` that the grammars split at its calling
    // convention, loses its method (pinned, FIXME(#1904)).
    pub(crate) fn claims(&mut self, member: &Node, code: &[u8]) -> bool {
        let claimed = self.after_cut_off && cpp_opens_with_annotation(member, code);
        self.after_cut_off = cpp_ends_cut_off(member);
        claimed
    }
}

// Whether a member opens with an annotation's name: as the name of the
// function a typeless `declaration` or `function_definition` declares
// (`MOZ_REQUIRES(mu);`, `MOZ_REQUIRES(mu) {}`), or as the type of a
// member whose declarator is the parenthesised argument
// (`MOZ_GUARDED_BY(*mMonitor) = false;`). An annotation after another
// word is not looked for, and still counts as a method: the word is an
// annotation macro that takes no argument (`int* mPtr
// MOZ_NON_OWNING_REF MOZ_GUARDED_BY(mu);`), or the member's own name
// where a macro the grammars do not know opens the declaration
// (`MOZ_X static Mutex other_ GUARDED_BY(mu);`). A method named as an
// annotation is has that tree after any cut-off member, with its
// return type for the word (pinned, FIXME(#1904)).
fn cpp_opens_with_annotation(member: &Node, code: &[u8]) -> bool {
    let Some(first) = member.children().next() else {
        return false;
    };
    let word = if first.kind() == "function_declarator" {
        first.child_by_field_name("declarator")
    } else {
        Some(first)
    };
    word.and_then(|word| word.utf8_text(code))
        .is_some_and(cpp_is_thread_safety_annotation)
}

// Whether a member ends in a `;` the grammars invented
// (`cpp_is_cut_off`), looked for also in the declaration a template or
// a `friend` wraps: `template <class U> void Visit(U u) const
// MOZ_REQUIRES(mu);` and `friend void Swap(C& a, C& b)
// MOZ_REQUIRES(mu);` are cut off inside the wrapper. A loop, because a
// template may wrap a `friend` and the source chooses how many.
fn cpp_ends_cut_off(member: &Node) -> bool {
    let mut declaration = *member;
    while declaration.has_error() {
        if cpp_is_cut_off(&declaration) {
            return true;
        }
        let wrapped = match declaration.kind() {
            "template_declaration" | "friend_declaration" => declaration.children().last(),
            _ => None,
        };
        let Some(wrapped) = wrapped else {
            return false;
        };
        declaration = wrapped;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // expected: every name the page defines for a member is taken, alone
    // and under each prefix a library is known to use.
    #[test]
    fn documented_names_are_annotations_under_any_prefix() {
        for name in CPP_THREAD_SAFETY_ANNOTATIONS {
            assert!(cpp_is_thread_safety_annotation(name), "{name}");
            for prefix in ["ABSL_", "MOZ_", "RTC_", "TF_", "MOZ_CAPABILITY_", "_"] {
                let word = format!("{prefix}{name}");
                assert!(cpp_is_thread_safety_annotation(&word), "{word}");
            }
        }
    }

    // expected: a word that only holds a documented name, or spells one
    // in another case, is no annotation; neither are a library's
    // spellings that end in no documented name, the annotations that
    // take no argument, nor the class annotation.
    #[test]
    fn other_words_are_not_annotations() {
        for word in [
            "",
            "_",
            "GUARDED",
            "GUARDED_BYE",
            "XGUARDED_BY",
            "UNGUARDED_BY",
            "PREREQUIRES",
            "GUARDED_BY_",
            "GUARDED_BY_CONTEXT",
            "GTEST_LOCK_EXCLUDED_",
            "GTEST_EXCLUSIVE_LOCK_REQUIRED_",
            "MOZ_SCOPED_UNLOCK_REACQUIRE",
            "guarded_by",
            "Requires",
            "absl_GUARDED_BY",
            "MOZ_GUARDED_BY2",
            "RELEASE_ASSERT",
            "MOZ_RELEASE_ASSERT",
            "NO_THREAD_SAFETY_ANALYSIS",
            "GUARDED_VAR",
            "CAPABILITY",
            "SCOPED_CAPABILITY",
            "MOZ_UNANNOTATED",
        ] {
            assert!(!cpp_is_thread_safety_annotation(word), "{word}");
        }
    }
}
