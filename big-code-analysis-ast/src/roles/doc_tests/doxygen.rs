//! The Doxygen family: one `Comment` row, a `DocComment` under the marker
//! gate, and a `DocTarget` row that replaced a *string-keyed* list
//! (`lang_helpers::doxygen::doxygen_entry` matched `node.kind()`, so every
//! alias id of a rule matched for free). The rows list ids, and
//! `old_names_are_the_row` below holds each dialect's row to the id set
//! the old names denote — exactly, with a hand-pinned copy of those
//! names, so a moved alias cannot hide in `excludes`. `ERROR` is the
//! Checker's clause (the enum sentinel owns no row), pinned on recovery
//! fixtures through a walk that tolerates them.

use super::*;
use crate::ParserTrait;
use std::collections::BTreeSet;
use std::path::Path;

/// The names `doxygen_entry` matched before #1699 (its `match
/// node.kind()` arms, plus the specifier and field arms it gated),
/// and the five conditional names `preproc_conditional` matches. A
/// hand-pinned copy, not read from the rows, so this is an oracle.
const OLD_ENTRY_NAMES: &[&str] = &[
    "declaration",
    "function_definition",
    "type_definition",
    "preproc_def",
    "preproc_function_def",
    "linkage_specification",
    "enumerator",
    "namespace_definition",
    "using_declaration",
    "alias_declaration",
    "friend_declaration",
    "template_declaration",
    "class_interface",
    "class_implementation",
    "protocol_declaration",
    "qualified_protocol_interface_declaration",
    "property_declaration",
    "method_declaration",
    "method_definition",
    "instance_variable",
    "struct_specifier",
    "union_specifier",
    "enum_specifier",
    "class_specifier",
    "field_declaration",
    "expression_statement",
    "implementation_definition",
];
const OLD_CONDITIONAL_NAMES: &[&str] = &[
    "preproc_if",
    "preproc_ifdef",
    "preproc_else",
    "preproc_elif",
    "preproc_elifdef",
];

/// Every runtime kind id of `L`'s enum whose `kind()` string is one
/// of `names` — the set the old string compare denoted. Read from
/// the generated `From<Enum> for &str` through the flags sidecar,
/// which lists every id but the `ERROR` sentinel.
fn ids_named<E: From<u16> + Into<&'static str>>(
    names: &[&str],
    flags: &[(u16, bool, bool)],
) -> BTreeSet<u16> {
    flags
        .iter()
        .map(|(id, ..)| *id)
        .filter(|&id| names.contains(&E::from(id).into()))
        .collect()
}

/// Walks `source` with the real chain, error-recovered trees
/// included, and answers `(kind, text, row, checker)` for every node
/// the table names or that either reading answers true for.
#[cfg(feature = "c")]
fn targets_with_recovery<P: ParserTrait>(
    source: &[u8],
    row: fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> bool,
) -> Vec<(String, String, bool, bool)> {
    let parser = P::new(source.to_vec(), Path::new("fixture"), None);
    let root = parser.root();
    let mut chain: Vec<Node<'_>> = Vec::new();
    let mut stack = vec![(root, 0_usize)];
    let mut seen = Vec::new();
    while let Some((node, depth)) = stack.pop() {
        chain.truncate(depth);
        let ancestors = Ancestors::known(&chain);
        let r = row(&node, source, ancestors);
        let c = P::Checker::is_doc_target(&node, source, ancestors);
        if r || c {
            let text = node.utf8_text(source).unwrap_or("<non-utf8>");
            let text = text.lines().next().unwrap_or("").to_owned();
            seen.push((node.kind().to_owned(), text, r, c));
        }
        chain.push(node);
        let first = stack.len();
        stack.extend(node.children().map(|child| (child, depth + 1)));
        stack[first..].reverse();
    }
    seen
}

macro_rules! old_names_are_the_row {
    ($name:ident, $lang:ident, $roles:ident, $flags:ident) => {
        /// The id set the old names denote equals the row — the
        /// migration from strings to ids lost and gained nothing.
        #[test]
        fn $name() {
            let entries = ids_named::<crate::$lang>(OLD_ENTRY_NAMES, crate::$flags);
            let conditionals = ids_named::<crate::$lang>(OLD_CONDITIONAL_NAMES, crate::$flags);
            let row: BTreeSet<u16> = crate::roles::$roles::doc_target::OWNERS
                .iter()
                .copied()
                .collect();
            assert_eq!(
                row,
                &entries | &conditionals,
                "row minus old: {:?}; old minus row: {:?}",
                row.difference(&(&entries | &conditionals))
                    .collect::<Vec<_>>(),
                (&entries | &conditionals)
                    .difference(&row)
                    .collect::<Vec<_>>(),
            );
            assert!(!entries.is_empty() && !conditionals.is_empty());
        }
    };
}

#[cfg(feature = "c")]
old_names_are_the_row!(c_old_names_are_the_row, C, c, C_KIND_FLAGS);
#[cfg(feature = "cpp")]
old_names_are_the_row!(cpp_old_names_are_the_row, Cpp, cpp, CPP_KIND_FLAGS);
#[cfg(feature = "mozcpp")]
old_names_are_the_row!(
    mozcpp_old_names_are_the_row,
    Mozcpp,
    mozcpp,
    MOZCPP_KIND_FLAGS
);
#[cfg(feature = "objc")]
old_names_are_the_row!(objc_old_names_are_the_row, Objc, objc, OBJC_KIND_FLAGS);

// Every C entry kind with its gate's two sides, the three conditional
// forms with and without an entry, a nested conditional, a macro call
// and an unterminated one, and a doc comment in a body (Doxygen's
// in-body description: still a `DocComment`).
#[cfg(feature = "c")]
#[test]
fn c_rows_answer_the_table() {
    use crate::CCode;
    let source = b"/** file */\n#include <stdio.h>\n/** macro */\n#define X 1\n/** fn macro */\n#define F(a) (a)\n/*! decl */\nint x;\n/// fn\nint f(void) { /** in body */ return x; }\n//! typedef\ntypedef int T;\n// plain\n/* block */\n/**/\n/** struct with body */\nstruct S { /** field */ int a; struct In; };\n/** forward */\nstruct Fwd;\nunion UF;\nenum EF;\n/** union */\nunion U { int b; };\n/** enum */\nenum E { /** enumerator */ A, B };\n/** macro call */\nFOO(bar);\n/** empty stmt */\n;\n#if X\n/** in if */\nint in_if;\n#elif Y\nint in_elif;\n#else\nint in_else;\n#endif\n#ifdef Z\n#endif\n#ifdef Y\n#elifdef YY\nint yy;\n#endif\n#if A\n# if B\nint deep;\n# endif\n#endif\nextern \"C\" { int c; }\n";
    let expected: &[(&str, &str, Answers)] = &[
        ("comment", "/** file */", DOC),
        ("comment", "/** macro */", DOC),
        ("preproc_def", "#define X 1", TARGET),
        ("comment", "/** fn macro */", DOC),
        ("preproc_function_def", "#define F(a) (a)", TARGET),
        ("comment", "/*! decl */", DOC),
        ("declaration", "int x;", TARGET),
        ("comment", "/// fn", DOC),
        (
            "function_definition",
            "int f(void) { /** in body */ return x; }",
            TARGET,
        ),
        ("comment", "/** in body */", DOC),
        ("comment", "//! typedef", DOC),
        ("type_definition", "typedef int T;", TARGET),
        ("comment", "// plain", PLAIN),
        ("comment", "/* block */", PLAIN),
        ("comment", "/**/", PLAIN),
        ("comment", "/** struct with body */", DOC),
        (
            "struct_specifier",
            "struct S { /** field */ int a; struct In; }",
            TARGET,
        ),
        ("comment", "/** field */", DOC),
        ("field_declaration", "int a;", TARGET),
        ("field_declaration", "struct In;", NONE),
        ("struct_specifier", "struct In", NONE),
        ("comment", "/** forward */", DOC),
        ("struct_specifier", "struct Fwd", NONE),
        ("union_specifier", "union UF", NONE),
        ("enum_specifier", "enum EF", NONE),
        ("comment", "/** union */", DOC),
        ("union_specifier", "union U { int b; }", TARGET),
        ("field_declaration", "int b;", TARGET),
        ("comment", "/** enum */", DOC),
        (
            "enum_specifier",
            "enum E { /** enumerator */ A, B }",
            TARGET,
        ),
        ("comment", "/** enumerator */", DOC),
        ("enumerator", "A", TARGET),
        ("enumerator", "B", TARGET),
        ("comment", "/** macro call */", DOC),
        ("expression_statement", "FOO(bar);", TARGET),
        ("comment", "/** empty stmt */", DOC),
        ("expression_statement", ";", NONE),
        ("preproc_if", "#if X", TARGET),
        ("comment", "/** in if */", DOC),
        ("declaration", "int in_if;", TARGET),
        ("preproc_elif", "#elif Y", TARGET),
        ("declaration", "int in_elif;", TARGET),
        ("preproc_else", "#else", TARGET),
        ("declaration", "int in_else;", TARGET),
        ("preproc_ifdef", "#ifdef Z", NONE),
        ("preproc_ifdef", "#ifdef Y", TARGET),
        ("preproc_elifdef", "#elifdef YY", TARGET),
        ("declaration", "int yy;", TARGET),
        ("preproc_if", "#if A", TARGET),
        ("preproc_if", "# if B", TARGET),
        ("declaration", "int deep;", TARGET),
        ("linkage_specification", "extern \"C\" { int c; }", TARGET),
        ("declaration", "int c;", TARGET),
    ];
    check::<CCode>(
        source,
        answers!(crate::roles::c, CCode),
        expected,
        &declared!(crate::roles::c),
        // The second `function_definition` id and the conditionals'
        // later aliases are what the old string compare matched for
        // free; the fixture spells one of each rule and the rest are
        // defensive. `expression_statement` is the other way round:
        // every statement the pinned grammar emits carries the
        // *second* id, so the first is the defensive one.
        &defensive_c(),
    );
}

#[cfg(feature = "c")]
fn defensive_c() -> Vec<u16> {
    use crate::C;
    vec![
        C::FunctionDefinition2 as u16,
        C::ExpressionStatement as u16,
        C::PreprocIf2 as u16,
        C::PreprocIf3 as u16,
        C::PreprocIf4 as u16,
        C::PreprocIfdef2 as u16,
        C::PreprocIfdef3 as u16,
        C::PreprocIfdef4 as u16,
        C::PreprocElse2 as u16,
        C::PreprocElse3 as u16,
        C::PreprocElse4 as u16,
        C::PreprocElif2 as u16,
        C::PreprocElif3 as u16,
        C::PreprocElif4 as u16,
        C::PreprocElifdef2 as u16,
        C::PreprocElifdef3 as u16,
        C::PreprocElifdef4 as u16,
    ]
}

/// The `ERROR` clause is the Checker's, never the row's. The outer
/// error here holds several named children (a type, a name, an
/// inner error, a parameter), which `no_entry_inside` reads as "too
/// much to be nothing" — Doxygen's benefit of the doubt — so it is
/// a target by the Checker's clause while the row answers `false`;
/// the inner one holds exactly one named non-error child, a
/// `compound_statement`, no entry, so it is not. Recovery shapes are
/// the grammar's and pinned as observed; the claim is the row /
/// Checker split, not the shape.
#[cfg(feature = "c")]
#[test]
fn c_error_clause_is_the_checkers_not_the_rows() {
    use crate::CParser;
    let source = b"int ok;\nint broken( { int inside; }\n}\nint a( int b( ;\nint after;\n";
    let seen = targets_with_recovery::<CParser>(source, crate::roles::c::doc_target::owns);
    let rows: Vec<_> = seen
        .iter()
        .map(|(k, t, r, c)| (k.as_str(), t.as_str(), *r, *c))
        .collect();
    assert_eq!(
        rows,
        [
            ("declaration", "int ok;", true, true),
            ("ERROR", "int broken( { int inside; }", false, true),
            ("declaration", "int inside;", true, true),
        ]
    );
}

// C++'s own entries: a namespace, `using`, alias, friend, template
// (a forward one included), a class with a body against one without,
// and `#elifdef`; then the C-shaped entries, so every owner is met.
// One fixture for the two C++ grammars: Mozilla's names its nodes as
// tree-sitter-cpp does and owns no file extension, so its table is
// pinned on the same source (grammar-dispatch, "a LANG variant that
// owns no file extension").
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_SOURCE: &[u8] = b"/** ns */\nnamespace N {\n/** using */\nusing std::string;\n/** alias */\nusing A = int;\n/** class */\nclass C {\n  /** friend */\n  friend class D;\n  /** member fn */\n  void m();\n  /** field */\n  int f;\n};\n/** template */\ntemplate <class T> class Tpl { };\n/** template fwd */\ntemplate <class T> class TplFwd;\n/** fn def */\nvoid C::m() { }\n}\n/** class fwd */\nclass Fwd;\nunion UF;\nenum EF;\n#if X\n/** in if */\nint in_if;\n#elifdef Y\nint y;\n#endif\n/** typedef */\ntypedef int T;\n/** macro */\n#define X 1\n/** fn macro */\n#define F(a) (a)\n/** linkage */\nextern \"C\" { int c; }\n/** enum */\nenum E { /** a */ A };\n/** struct */\nstruct S { int a; };\n/** union */\nunion U { int b; };\n/** macro call */\nFOO(bar);\n;\n#ifdef Z\nint z;\n#elif W\nint w;\n#else\nint e;\n#endif\n#ifdef EMPTY\n#endif\n";

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_EXPECTED: &[(&str, &str, Answers)] = &[
    ("comment", "/** ns */", DOC),
    ("namespace_definition", "namespace N {", TARGET),
    ("comment", "/** using */", DOC),
    ("using_declaration", "using std::string;", TARGET),
    ("comment", "/** alias */", DOC),
    ("alias_declaration", "using A = int;", TARGET),
    ("comment", "/** class */", DOC),
    ("class_specifier", "class C {", TARGET),
    ("comment", "/** friend */", DOC),
    ("friend_declaration", "friend class D;", TARGET),
    ("comment", "/** member fn */", DOC),
    ("field_declaration", "void m();", TARGET),
    ("comment", "/** field */", DOC),
    ("field_declaration", "int f;", TARGET),
    ("comment", "/** template */", DOC),
    (
        "template_declaration",
        "template <class T> class Tpl { };",
        TARGET,
    ),
    ("class_specifier", "class Tpl { }", TARGET),
    ("comment", "/** template fwd */", DOC),
    (
        "template_declaration",
        "template <class T> class TplFwd;",
        TARGET,
    ),
    ("class_specifier", "class TplFwd", NONE),
    ("comment", "/** fn def */", DOC),
    ("function_definition", "void C::m() { }", TARGET),
    ("comment", "/** class fwd */", DOC),
    ("class_specifier", "class Fwd", NONE),
    ("union_specifier", "union UF", NONE),
    ("enum_specifier", "enum EF", NONE),
    ("preproc_if", "#if X", TARGET),
    ("comment", "/** in if */", DOC),
    ("declaration", "int in_if;", TARGET),
    ("preproc_elifdef", "#elifdef Y", TARGET),
    ("declaration", "int y;", TARGET),
    ("comment", "/** typedef */", DOC),
    ("type_definition", "typedef int T;", TARGET),
    ("comment", "/** macro */", DOC),
    ("preproc_def", "#define X 1", TARGET),
    ("comment", "/** fn macro */", DOC),
    ("preproc_function_def", "#define F(a) (a)", TARGET),
    ("comment", "/** linkage */", DOC),
    ("linkage_specification", "extern \"C\" { int c; }", TARGET),
    ("declaration", "int c;", TARGET),
    ("comment", "/** enum */", DOC),
    ("enum_specifier", "enum E { /** a */ A }", TARGET),
    ("comment", "/** a */", DOC),
    ("enumerator", "A", TARGET),
    ("comment", "/** struct */", DOC),
    ("struct_specifier", "struct S { int a; }", TARGET),
    ("field_declaration", "int a;", TARGET),
    ("comment", "/** union */", DOC),
    ("union_specifier", "union U { int b; }", TARGET),
    ("field_declaration", "int b;", TARGET),
    ("comment", "/** macro call */", DOC),
    ("expression_statement", "FOO(bar);", TARGET),
    ("expression_statement", ";", NONE),
    ("preproc_ifdef", "#ifdef Z", TARGET),
    ("declaration", "int z;", TARGET),
    ("preproc_elif", "#elif W", TARGET),
    ("declaration", "int w;", TARGET),
    ("preproc_else", "#else", TARGET),
    ("declaration", "int e;", TARGET),
    ("preproc_ifdef", "#ifdef EMPTY", NONE),
];

/// The C++ aliases the pinned grammar gives no spelling in the
/// fixture, in `$lang`'s ids: `declaration` and `function_definition`
/// are aliased four ways and every spelling takes the first; every
/// conditional takes its first alias; `expression_statement` the
/// *second*, as in C. All declared, none emitted — the `defensive`
/// contract.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
macro_rules! cpp_doc_defensive {
    ($lang:ident) => {
        [
            crate::$lang::Declaration2 as u16,
            crate::$lang::Declaration3 as u16,
            crate::$lang::Declaration4 as u16,
            crate::$lang::FunctionDefinition2 as u16,
            crate::$lang::FunctionDefinition3 as u16,
            crate::$lang::FunctionDefinition4 as u16,
            crate::$lang::ExpressionStatement as u16,
            crate::$lang::PreprocIf2 as u16,
            crate::$lang::PreprocIf3 as u16,
            crate::$lang::PreprocIf4 as u16,
            crate::$lang::PreprocIfdef2 as u16,
            crate::$lang::PreprocIfdef3 as u16,
            crate::$lang::PreprocIfdef4 as u16,
            crate::$lang::PreprocElse2 as u16,
            crate::$lang::PreprocElse3 as u16,
            crate::$lang::PreprocElse4 as u16,
            crate::$lang::PreprocElif2 as u16,
            crate::$lang::PreprocElif3 as u16,
            crate::$lang::PreprocElif4 as u16,
            crate::$lang::PreprocElifdef2 as u16,
            crate::$lang::PreprocElifdef3 as u16,
            crate::$lang::PreprocElifdef4 as u16,
        ]
    };
}

#[cfg(feature = "cpp")]
#[test]
fn cpp_rows_answer_the_table() {
    check::<crate::CppCode>(
        CPP_SOURCE,
        answers!(crate::roles::cpp, crate::CppCode),
        CPP_EXPECTED,
        &declared!(crate::roles::cpp),
        &cpp_doc_defensive!(Cpp),
    );
}

#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_rows_answer_the_table() {
    check::<crate::MozcppCode>(
        CPP_SOURCE,
        answers!(crate::roles::mozcpp, crate::MozcppCode),
        CPP_EXPECTED,
        &declared!(crate::roles::mozcpp),
        &cpp_doc_defensive!(Mozcpp),
    );
}

// Objective-C's own entries: the three containers and a protocol's
// `@optional` section, a property, an instance variable, a method
// declaration and definition — the `implementation_definition`
// wrapper of a definition is a target, of a `@synthesize` not.
#[cfg(feature = "objc")]
#[test]
fn objc_rows_answer_the_table() {
    use crate::{Objc, ObjcCode};
    let source = b"/** interface */\n@interface C : NSObject {\n  /** ivar */\n  int iv;\n}\n/** property */\n@property int p;\n/** method decl */\n- (void)m;\n+ (void)c;\n@end\n/** impl */\n@implementation C\n/** method def */\n- (void)m { }\n@synthesize p;\n@end\n/** protocol */\n@protocol P\n- (void)q;\n@end\n/** fn */\nint f(void) { return 1; }\n#if X\n/** in if */\nint in_if;\n#else\nint e;\n#endif\n@protocol Q\n/** d */\n@optional\n- (void)r;\n@end\n/** typedef */\ntypedef int T;\n/** macro */\n#define X 1\n/** fn macro */\n#define F(a) (a)\n/** linkage */\nextern \"C\" { int c; }\n/** enum */\nenum E { /** a */ A };\n/** struct */\nstruct S { int a; };\n/** union */\nunion U { int b; };\nstruct SF;\nunion UF;\nenum EF;\n/** macro call */\nFOO(bar);\n;\n#ifdef Z\nint z;\n#elif W\nint w;\n#endif\n#ifdef Y\n#elifdef YY\nint yy;\n#endif\n#ifdef EMPTY\n#endif\n";
    let expected: &[(&str, &str, Answers)] = &[
        ("comment", "/** interface */", DOC),
        ("class_interface", "@interface C : NSObject {", TARGET),
        ("comment", "/** ivar */", DOC),
        ("instance_variable", "int iv;", TARGET),
        ("comment", "/** property */", DOC),
        ("property_declaration", "@property int p;", TARGET),
        ("comment", "/** method decl */", DOC),
        ("method_declaration", "- (void)m;", TARGET),
        ("method_declaration", "+ (void)c;", TARGET),
        ("comment", "/** impl */", DOC),
        ("class_implementation", "@implementation C", TARGET),
        ("comment", "/** method def */", DOC),
        ("implementation_definition", "- (void)m { }", TARGET),
        ("method_definition", "- (void)m { }", TARGET),
        ("implementation_definition", "@synthesize p;", NONE),
        ("comment", "/** protocol */", DOC),
        ("protocol_declaration", "@protocol P", TARGET),
        ("method_declaration", "- (void)q;", TARGET),
        ("comment", "/** fn */", DOC),
        ("function_definition", "int f(void) { return 1; }", TARGET),
        ("preproc_if", "#if X", TARGET),
        ("comment", "/** in if */", DOC),
        ("declaration", "int in_if;", TARGET),
        ("preproc_else", "#else", TARGET),
        ("declaration", "int e;", TARGET),
        ("protocol_declaration", "@protocol Q", TARGET),
        ("comment", "/** d */", DOC),
        (
            "qualified_protocol_interface_declaration",
            "@optional",
            TARGET,
        ),
        ("method_declaration", "- (void)r;", TARGET),
        ("comment", "/** typedef */", DOC),
        ("type_definition", "typedef int T;", TARGET),
        ("comment", "/** macro */", DOC),
        ("preproc_def", "#define X 1", TARGET),
        ("comment", "/** fn macro */", DOC),
        ("preproc_function_def", "#define F(a) (a)", TARGET),
        ("comment", "/** linkage */", DOC),
        ("linkage_specification", "extern \"C\" { int c; }", TARGET),
        ("declaration", "int c;", TARGET),
        ("comment", "/** enum */", DOC),
        ("enum_specifier", "enum E { /** a */ A }", TARGET),
        ("comment", "/** a */", DOC),
        ("enumerator", "A", TARGET),
        ("comment", "/** struct */", DOC),
        ("struct_specifier", "struct S { int a; }", TARGET),
        ("field_declaration", "int a;", TARGET),
        ("comment", "/** union */", DOC),
        ("union_specifier", "union U { int b; }", TARGET),
        ("field_declaration", "int b;", TARGET),
        ("struct_specifier", "struct SF", NONE),
        ("union_specifier", "union UF", NONE),
        ("enum_specifier", "enum EF", NONE),
        ("comment", "/** macro call */", DOC),
        ("expression_statement", "FOO(bar);", TARGET),
        ("expression_statement", ";", NONE),
        ("preproc_ifdef", "#ifdef Z", TARGET),
        ("declaration", "int z;", TARGET),
        ("preproc_elif", "#elif W", TARGET),
        ("declaration", "int w;", TARGET),
        ("preproc_ifdef", "#ifdef Y", TARGET),
        ("preproc_elifdef", "#elifdef YY", TARGET),
        ("declaration", "int yy;", TARGET),
        ("preproc_ifdef", "#ifdef EMPTY", NONE),
    ];
    check::<ObjcCode>(
        source,
        answers!(crate::roles::objc, ObjcCode),
        expected,
        &declared!(crate::roles::objc),
        // The aliases the pinned grammar gives no spelling: the second
        // `function_definition` id, the first `expression_statement`
        // (every statement takes the second, as in C), and every
        // conditional's later aliases (`#elifdef` under `#ifdef` takes
        // the first; under `#if` it is a bare directive).
        &[
            Objc::FunctionDefinition2 as u16,
            Objc::ExpressionStatement as u16,
            Objc::PreprocIf2 as u16,
            Objc::PreprocIf3 as u16,
            Objc::PreprocIf4 as u16,
            Objc::PreprocIfdef2 as u16,
            Objc::PreprocIfdef3 as u16,
            Objc::PreprocElse2 as u16,
            Objc::PreprocElse3 as u16,
            Objc::PreprocElse4 as u16,
            Objc::PreprocElse5 as u16,
            Objc::PreprocElif2 as u16,
            Objc::PreprocElif3 as u16,
            Objc::PreprocElif4 as u16,
            Objc::PreprocElif5 as u16,
            Objc::PreprocElifdef2 as u16,
        ],
    );
}
