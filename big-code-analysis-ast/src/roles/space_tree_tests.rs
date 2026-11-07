//! The space-tree rows of the kind-only languages, pinned independently
//! (#1696): for each language, a fixture spelling every `FuncOpener`,
//! `Closure`, `Container` and `Member` kind, and the expected
//! `(is_func, is_closure, promoted, SpaceKind, is_member)` per kind as
//! the classifiers answered before they read the rows. A kind absent
//! from a language's table answers all-false; every declared owner must
//! be met by the fixture.

use crate::test_support::{for_each_node_with_chain, owners_covered};
use crate::{Ancestors, Checker, Getter, LanguageInfo, Node, SpaceKind};

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
))]
type Answers = (bool, bool, bool, SpaceKind, bool);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "lua",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
type OpensSpace = fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> bool;

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
))]
const F: Answers = (true, false, true, SpaceKind::Function, false);
#[cfg(any(
    feature = "c",
    feature = "csharp",
    feature = "go",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
))]
const C: Answers = (false, true, true, SpaceKind::Function, false);
#[cfg(any(
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "kotlin",
    feature = "mozcpp",
    feature = "objc",
    feature = "php",
    feature = "rust",
))]
const M: Answers = (false, false, false, SpaceKind::Unknown, true);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
))]
const NONE: Answers = (false, false, false, SpaceKind::Unknown, false);

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
))]
fn container(kind: SpaceKind) -> Answers {
    (false, false, true, kind, false)
}

/// `defensive` are the declared kinds the pinned grammar never emits —
/// the aliases listed per grammar-dispatch §2 — which the fixture is
/// required *not* to produce, so a bump that starts emitting one fails
/// here rather than changing a count unseen.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "lua",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
fn check<L: LanguageInfo + Checker + Getter>(
    source: &[u8],
    expected: &[(&str, Answers)],
    member_owns: OpensSpace,
    owners: &[&[u16]],
    defensive: &[u16],
) {
    let mut seen_kinds = std::collections::BTreeSet::new();
    for_each_node_with_chain::<L>(source, |node, chain| {
        let ancestors = Ancestors::known(chain);
        let kind = node.kind();
        let promoted = L::promotes_to_func_space_with_code(node, source, ancestors);
        let got = (
            L::is_func(node, ancestors),
            L::is_closure(node, ancestors),
            promoted,
            L::get_space_kind_with_code(node, source, ancestors),
            member_owns(node, source, ancestors),
        );
        let want = expected
            .iter()
            .find(|(k, _)| *k == kind && node.is_named())
            .map_or(NONE, |(_, v)| *v);
        assert_eq!(
            got,
            want,
            "{kind} `{}`",
            node.utf8_text(source).unwrap_or("?")
        );
        assert_eq!(
            got.3 != SpaceKind::Unknown,
            promoted,
            "{kind}: opens_space vs promotion"
        );
        assert_eq!(
            L::is_func_space_with_code(node, source, ancestors),
            promoted,
            "{kind}: is_func_space is what the walk promotes"
        );
        // The byte-less pair is the kind table, gates aside: on a row
        // whose owners read no chain and no bytes it answers exactly
        // what the `_with_code` pair answers (Ruby's chain-reading gate
        // is pinned apart, in `ruby_byte_less_spellings_stay_the_kind_table`).
        assert_eq!(
            (L::is_func_space(node), L::get_space_kind(node)),
            (promoted, got.3),
            "{kind}: the byte-less spellings are the kind table"
        );
        seen_kinds.insert(node.kind_id());
    });
    owners_covered(&seen_kinds, owners, defensive);
}

#[cfg(feature = "rust")]
#[test]
fn rust_space_tree_rows() {
    use crate::roles::rust::{closure, container, func_opener, member};
    check::<crate::RustCode>(
        b"struct S { a: i32 }\ntrait T { const K: i32; type A; fn tm(&self); }\nimpl T for S { const K: i32 = 1; type A = i32; fn tm(&self) { let c = |x: i32| x; c(1); } }\nstatic G: i32 = 0;\nfn f() {}\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("impl_item", container(SpaceKind::Impl)),
            ("trait_item", container(SpaceKind::Trait)),
            ("function_item", F),
            ("closure_expression", C),
            ("field_declaration", M),
            ("const_item", M),
            ("static_item", M),
            ("associated_type", M),
        ],
        member::owns,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS, member::OWNERS],
        &[],
    );
}

#[cfg(feature = "go")]
#[test]
fn go_space_tree_rows() {
    use crate::roles::go::{closure, container, func_opener, member};
    check::<crate::GoCode>(
        b"package p\ntype S struct { A int; b string }\nfunc (s *S) M() { f := func(x int) int { return x }; f(1) }\nfunc top() {}\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("function_declaration", F),
            ("method_declaration", F),
            ("func_literal", C),
            ("field_declaration", M),
        ],
        member::owns,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS, member::OWNERS],
        &[],
    );
}

#[cfg(feature = "lua")]
#[test]
fn lua_space_tree_rows() {
    use crate::roles::lua::{closure, container, func_opener};
    check::<crate::LuaCode>(
        b"local M = {}\nfunction M.f(a) return a end\nlocal function g() end\nfunction h() local c = function(x) return x end; return c(1) end\n",
        &[
            ("chunk", container(SpaceKind::Unit)),
            ("function_declaration", F),
            ("function_definition", C),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS],
        // The pinned grammar emits one `function_declaration` id for
        // `function f`, `local function g`, `function M.h` and
        // `function M:i` alike; the two aliases are defensive.
        &[crate::Lua::FunctionDeclaration2 as u16, crate::Lua::FunctionDeclaration3 as u16],
    );
}

#[cfg(feature = "bash")]
#[test]
fn bash_space_tree_rows() {
    use crate::roles::bash::{container, func_opener};
    check::<crate::BashCode>(
        b"f() { echo 1; }\nfunction g { f; }\n",
        &[
            ("program", container(SpaceKind::Unit)),
            ("function_definition", F),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, container::OWNERS],
        &[],
    );
}

#[cfg(feature = "tcl")]
#[test]
fn tcl_space_tree_rows() {
    use crate::roles::tcl::{container, func_opener};
    check::<crate::TclCode>(
        b"proc f {a} { return $a }\nf 1\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("procedure", F),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, container::OWNERS],
        &[],
    );
}

#[cfg(feature = "irules")]
#[test]
fn irules_space_tree_rows() {
    use crate::roles::irules::{container, func_opener};
    check::<crate::IrulesCode>(
        b"when HTTP_REQUEST {\n  f 1\n}\nproc f {a} { return $a }\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("procedure", F),
            ("when_event", F),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, container::OWNERS],
        &[],
    );
}

#[cfg(feature = "perl")]
#[test]
fn perl_space_tree_rows() {
    use crate::roles::perl::{closure, container, func_opener};
    check::<crate::PerlCode>(
        // `method m { }` is the grammar's `function_definition_without_sub`,
        // nested in a bareword call whose callee is `method`.
        b"package P;\nsub f { my $c = sub { 1 }; $c->(); }\nmethod m { 1 }\nsub g;\n1;\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("function_definition", F),
            ("function_definition_without_sub", F),
            ("anonymous_function", C),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS],
        &[],
    );
}

#[cfg(feature = "php")]
#[test]
fn php_space_tree_rows() {
    use crate::roles::php::{closure, container, func_opener, member};
    check::<crate::PhpCode>(
        b"<?php\ninterface I { const K = 1; public function m(); }\ntrait T { public $t; function tm() {} }\nenum E: int { case A = 1; const C = 2; }\nclass C { public $f = 1; const D = 'd'; function m() { $c = function ($x) { return $x; }; $a = fn($y) => $y; $o = new class { public $z; }; } }\nfunction top() {}\n",
        &[
            ("program", container(SpaceKind::Unit)),
            ("class_declaration", container(SpaceKind::Class)),
            ("anonymous_class", container(SpaceKind::Class)),
            ("enum_declaration", container(SpaceKind::Class)),
            ("trait_declaration", container(SpaceKind::Class)),
            ("interface_declaration", container(SpaceKind::Interface)),
            ("function_definition", F),
            ("method_declaration", F),
            ("anonymous_function", C),
            ("arrow_function", C),
            ("property_declaration", M),
            ("const_declaration", M),
        ],
        member::owns,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS, member::OWNERS],
        // One `const_declaration` id for a class's, an interface's and an
        // enum's constant at the pin; the alias is defensive.
        &[crate::Php::ConstDeclaration2 as u16],
    );
}

#[cfg(feature = "c")]
#[test]
fn c_space_tree_rows() {
    use crate::roles::c::{container, func_opener};
    check::<crate::CCode>(
        b"struct S { int a; };\nstatic int helper(int x) { return x; }\nint main(void) { return helper(1); }\n",
        &[
            ("translation_unit", container(SpaceKind::Unit)),
            ("function_definition", F),
        ],
        |_, _, _| false,
        &[func_opener::OWNERS, container::OWNERS],
        // One `function_definition` id for the prototype, K&R, `static
        // inline` and bodied spellings at the pin; the alias is defensive
        // (#285).
        &[crate::C::FunctionDefinition2 as u16],
    );
}

/// The C++ fixture both dialects read: bodied and bodiless struct /
/// class specifiers (#1555), a lambda (closure, no space), a namespace,
/// a data member, a bodiless member function (a `field_declaration` the
/// method consumer reads by declarator) and an inline method.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_SOURCE: &[u8] = b"namespace ns { int x; }\nstruct S;\nstruct S2 { int a; void m(); };\nclass C { int f; void m() { auto l = [](int x) { return x; }; l(1); } };\nvoid g(struct S *q) {}\nint main() { return 0; }\n";

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_EXPECTED: &[(&str, Answers)] = &[
    (
        "translation_unit",
        (false, false, true, SpaceKind::Unit, false),
    ),
    (
        "namespace_definition",
        (false, false, true, SpaceKind::Namespace, false),
    ),
    ("function_definition", F),
    (
        "lambda_expression",
        (false, true, false, SpaceKind::Unknown, false),
    ),
    ("field_declaration", M),
];

/// The two specifier kinds answer by their body: the definitions open
/// a space, `struct S;` and the parameter type `struct S *q` open none
/// (#1555). Checked by text since one kind spells both.
#[cfg(any(feature = "cpp", feature = "mozcpp"))]
fn check_cpp<L: LanguageInfo + Checker + Getter>(
    owners: &[&[u16]],
    member_owns: fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> bool,
    struct_kind: u16,
    class_kind: u16,
    defensive: &[u16],
) {
    let source = CPP_SOURCE;
    let mut specifiers = Vec::new();
    let mut seen_kinds = std::collections::BTreeSet::new();
    for_each_node_with_chain::<L>(source, |node, chain| {
        let ancestors = Ancestors::known(chain);
        let kind = node.kind();
        let promoted = L::promotes_to_func_space_with_code(node, source, ancestors);
        let got = (
            L::is_func(node, ancestors),
            L::is_closure(node, ancestors),
            promoted,
            L::get_space_kind_with_code(node, source, ancestors),
            member_owns(node, source, ancestors),
        );
        seen_kinds.insert(node.kind_id());
        if node.kind_id() == struct_kind || node.kind_id() == class_kind {
            specifiers.push((
                node.utf8_text(source).unwrap_or("?").to_owned(),
                got.2,
                got.3,
            ));
            return;
        }
        let want = CPP_EXPECTED
            .iter()
            .find(|(k, _)| *k == kind && node.is_named())
            .map_or(NONE, |(_, v)| *v);
        assert_eq!(
            got,
            want,
            "{kind} `{}`",
            node.utf8_text(source).unwrap_or("?")
        );
        assert_eq!(
            got.3 != SpaceKind::Unknown,
            promoted,
            "{kind}: opens_space vs promotion"
        );
        // `has_body` reads the node's own field, so the byte-less pair
        // agrees with the `_with_code` one on every node, specifiers
        // included.
        assert_eq!(
            (L::is_func_space(node), L::get_space_kind(node)),
            (promoted, got.3),
            "{kind}: the byte-less spellings are the kind table"
        );
    });
    assert_eq!(
        specifiers,
        [
            ("struct S".to_owned(), false, SpaceKind::Unknown),
            (
                "struct S2 { int a; void m(); }".to_owned(),
                true,
                SpaceKind::Struct
            ),
            (
                "class C { int f; void m() { auto l = [](int x) { return x; }; l(1); } }"
                    .to_owned(),
                true,
                SpaceKind::Class
            ),
            ("struct S".to_owned(), false, SpaceKind::Unknown),
        ]
    );
    owners_covered(&seen_kinds, owners, defensive);
}

#[cfg(feature = "cpp")]
#[test]
fn cpp_space_tree_rows() {
    use crate::roles::cpp::{closure, container, func_opener, member};
    check_cpp::<crate::CppCode>(
        &[
            func_opener::OWNERS,
            closure::OWNERS,
            container::OWNERS,
            member::OWNERS,
        ],
        member::owns,
        crate::Cpp::StructSpecifier as u16,
        crate::Cpp::ClassSpecifier as u16,
        // One `function_definition` id for a free function, a method, a
        // constructor, a destructor, a conversion operator, a template
        // and an out-of-line definition at the pin (probed with `bca
        // dump`); the three aliases are defensive (#285, lesson 2).
        &[
            crate::Cpp::FunctionDefinition2 as u16,
            crate::Cpp::FunctionDefinition3 as u16,
            crate::Cpp::FunctionDefinition4 as u16,
        ],
    );
}

#[cfg(feature = "mozcpp")]
#[test]
fn mozcpp_space_tree_rows() {
    use crate::roles::mozcpp::{closure, container, func_opener, member};
    check_cpp::<crate::MozcppCode>(
        &[
            func_opener::OWNERS,
            closure::OWNERS,
            container::OWNERS,
            member::OWNERS,
        ],
        member::owns,
        crate::Mozcpp::StructSpecifier as u16,
        crate::Mozcpp::ClassSpecifier as u16,
        &[
            crate::Mozcpp::FunctionDefinition2 as u16,
            crate::Mozcpp::FunctionDefinition3 as u16,
            crate::Mozcpp::FunctionDefinition4 as u16,
        ],
    );
}

#[cfg(feature = "objc")]
#[test]
fn objc_space_tree_rows() {
    use crate::roles::objc::{closure, container, func_opener, member};
    check::<crate::ObjcCode>(
        b"@interface Foo : NSObject { int ivar; }\n@property (nonatomic) int p;\n- (void)m:(int)x;\n@end\n@implementation Foo\n- (void)m:(int)x { void (^blk)(int) = ^(int y) { }; blk(1); }\nstatic void helper(void) {}\n@end\n@protocol P\n- (void)pm;\n@required\n@property int q;\n@end\nint cfun(int a) { return a; }\n",
        &[
            ("translation_unit", container(SpaceKind::Unit)),
            ("class_implementation", container(SpaceKind::Class)),
            ("class_interface", container(SpaceKind::Interface)),
            ("protocol_declaration", container(SpaceKind::Interface)),
            ("function_definition", F),
            ("method_definition", F),
            ("block_literal", (false, true, false, SpaceKind::Unknown, false)),
            ("property_declaration", M),
            ("instance_variable", M),
        ],
        member::owns,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS, member::OWNERS],
        // One `function_definition` id for every spelling at the pin; the
        // alias is defensive (#285).
        &[crate::Objc::FunctionDefinition2 as u16],
    );
}

/// C#: the accessor-less indexer and property (`this[int i] => i;`,
/// `int W => F;`) open a `Function` space as callables, the bodied ones
/// defer to their accessors (#464, #472 — lesson 65), checked by text
/// since one kind spells both; every other kind by the table.
#[cfg(feature = "csharp")]
#[test]
fn csharp_space_tree_rows() {
    use crate::roles::csharp::{closure, container, func_opener, member};
    let source = b"namespace N {\n  interface I { int P { get; set; } void M(); }\n  struct S { public int F; public S(int f) { F = f; } }\n  record Rec(int X) { public int Y { get; init; } }\n  enum E { A, B }\n  class C : I {\n    public int F1, F2 = 2;\n    public int P { get; set; }\n    public int W => F1;\n    public int this[int i] => i;\n    public int this[string k] { get { return 1; } }\n    public C() { }\n    ~C() { }\n    public void M() { int L(int z) { return z; } System.Func<int, int> f = y => y + 1; System.Action a = delegate { }; L(1); }\n    public static C operator +(C a, C b) => a;\n    public static implicit operator int(C c) => 1;\n  }\n}\n";
    let expected: &[(&str, Answers)] = &[
        ("compilation_unit", container(SpaceKind::Unit)),
        ("class_declaration", container(SpaceKind::Class)),
        ("struct_declaration", container(SpaceKind::Class)),
        ("record_declaration", container(SpaceKind::Class)),
        ("enum_declaration", container(SpaceKind::Class)),
        ("interface_declaration", container(SpaceKind::Interface)),
        ("method_declaration", F),
        ("constructor_declaration", F),
        ("destructor_declaration", F),
        ("local_function_statement", F),
        ("accessor_declaration", F),
        ("operator_declaration", F),
        ("conversion_operator_declaration", F),
        ("lambda_expression", C),
        ("anonymous_method_expression", C),
        ("field_declaration", M),
    ];
    let mut gated = Vec::new();
    let mut seen_kinds = std::collections::BTreeSet::new();
    for_each_node_with_chain::<crate::CsharpCode>(source, |node, chain| {
        use crate::{Checker, CsharpCode, Getter};
        let ancestors = Ancestors::known(chain);
        let kind = node.kind();
        let promoted = CsharpCode::promotes_to_func_space_with_code(node, source, ancestors);
        let got = (
            CsharpCode::is_func(node, ancestors),
            CsharpCode::is_closure(node, ancestors),
            promoted,
            CsharpCode::get_space_kind_with_code(node, source, ancestors),
            member::owns(node, source, ancestors),
        );
        seen_kinds.insert(node.kind_id());
        // `has_no_accessors` reads the node's own children, so the
        // byte-less pair agrees with the `_with_code` one everywhere.
        assert_eq!(
            (
                CsharpCode::is_func_space(node),
                CsharpCode::get_space_kind(node)
            ),
            (promoted, got.3),
            "{kind}: the byte-less spellings are the kind table"
        );
        if matches!(kind, "property_declaration" | "indexer_declaration") {
            let text = node.utf8_text(source).unwrap_or("?");
            let head = text.split_once(['{', '=']).map_or(text, |(h, _)| h).trim();
            gated.push((head.to_owned(), got));
            return;
        }
        let want = expected
            .iter()
            .find(|(k, _)| *k == kind && node.is_named())
            .map_or(NONE, |(_, v)| *v);
        assert_eq!(
            got,
            want,
            "{kind} `{}`",
            node.utf8_text(source).unwrap_or("?")
        );
        assert_eq!(
            got.3 != SpaceKind::Unknown,
            promoted,
            "{kind}: opens_space vs promotion"
        );
    });
    // A property or indexer is never a closure and never a member,
    // whichever way its gate answers.
    assert_eq!(
        gated,
        [
            ("int P".to_owned(), NONE),
            ("public int Y".to_owned(), NONE),
            ("public int P".to_owned(), NONE),
            ("public int W".to_owned(), F),
            ("public int this[int i]".to_owned(), F),
            ("public int this[string k]".to_owned(), NONE),
        ]
    );
    for &k in [
        func_opener::OWNERS,
        closure::OWNERS,
        container::OWNERS,
        member::OWNERS,
    ]
    .iter()
    .copied()
    .flatten()
    {
        assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
    }
}

#[cfg(feature = "kotlin")]
#[test]
fn kotlin_space_tree_rows() {
    use crate::roles::kotlin::{closure, container, func_opener, member};
    // Multi-line bodies: the pinned grammar rejects `object O { val o = 1 }`
    // on one line.
    let source = b"interface I {\n  val p: Int\n  fun m()\n}\nobject O {\n  val o = 1\n}\nclass C(private val f: Int) : I {\n  override val p: Int = 1\n  var q: String = \"q\"\n    get() = field\n    set(v) { field = v }\n  companion object {\n    val K = 1\n  }\n  init { println(1) }\n  constructor() : this(1)\n  override fun m() {\n    val l = { x: Int -> x }\n    val a = fun(y: Int) = y\n    val o = object : I {\n      override val p = 2\n      override fun m() {}\n    }\n    l(1); a(2); o.m()\n  }\n}\n";
    let mut classes = Vec::new();
    let mut seen_kinds = std::collections::BTreeSet::new();
    for_each_node_with_chain::<crate::KotlinCode>(source, |node, chain| {
        use crate::{Checker, Getter, KotlinCode};
        let ancestors = Ancestors::known(chain);
        let kind = node.kind();
        let promoted = KotlinCode::promotes_to_func_space_with_code(node, source, ancestors);
        let got = (
            KotlinCode::is_func(node, ancestors),
            KotlinCode::is_closure(node, ancestors),
            promoted,
            KotlinCode::get_space_kind_with_code(node, source, ancestors),
            member::owns(node, source, ancestors),
        );
        seen_kinds.insert(node.kind_id());
        // `class_or_interface` reads the node's own children, so the
        // byte-less pair agrees with the `_with_code` one everywhere.
        assert_eq!(
            (
                KotlinCode::is_func_space(node),
                KotlinCode::get_space_kind(node)
            ),
            (promoted, got.3),
            "{kind}: the byte-less spellings are the kind table"
        );
        // One kind, two space kinds, by the keyword token.
        if kind == "class_declaration" {
            classes.push((
                node.utf8_text(source)
                    .unwrap_or("?")
                    .chars()
                    .take(11)
                    .collect::<String>(),
                got.3,
            ));
            assert_eq!(got, (false, false, true, got.3, false), "{kind}");
            return;
        }
        let want = [
            ("source_file", container(SpaceKind::Unit)),
            ("object_declaration", container(SpaceKind::Class)),
            ("companion_object", container(SpaceKind::Class)),
            ("object_literal", container(SpaceKind::Class)),
            ("function_declaration", F),
            ("secondary_constructor", F),
            ("getter", (false, false, true, SpaceKind::Function, false)),
            ("setter", (false, false, true, SpaceKind::Function, false)),
            (
                "anonymous_initializer",
                (false, false, true, SpaceKind::Function, false),
            ),
            (
                "lambda_literal",
                (false, true, false, SpaceKind::Unknown, false),
            ),
            (
                "anonymous_function",
                (false, true, false, SpaceKind::Unknown, false),
            ),
            ("property_declaration", M),
        ]
        .iter()
        .find(|(k, _)| *k == kind && node.is_named())
        .map_or(NONE, |(_, v)| *v);
        assert_eq!(
            got,
            want,
            "{kind} `{}`",
            node.utf8_text(source).unwrap_or("?")
        );
        assert_eq!(
            got.3 != SpaceKind::Unknown,
            promoted,
            "{kind}: opens_space vs promotion"
        );
    });
    assert_eq!(
        classes,
        [
            ("interface I".to_owned(), SpaceKind::Interface),
            ("class C(pri".to_owned(), SpaceKind::Class),
        ]
    );
    for &k in [
        func_opener::OWNERS,
        closure::OWNERS,
        container::OWNERS,
        member::OWNERS,
    ]
    .iter()
    .copied()
    .flatten()
    {
        assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
    }
}

#[cfg(feature = "groovy")]
#[test]
fn groovy_space_tree_rows() {
    use crate::roles::groovy::{closure, container, func_opener, member};
    check::<crate::GroovyCode>(
        b"interface I { void m() }\nenum E { A, B }\ntrait Tr { def tm() {} }\n@interface An { int v() }\nrecord R(int x) {}\nclass C implements I {\n  int f\n  static { int s = 1 }\n  C() { }\n  void m() { def c = { x -> x }; c(1) }\n}\n",
        &[
            ("source_file", container(SpaceKind::Unit)),
            ("class_declaration", container(SpaceKind::Class)),
            ("enum_declaration", container(SpaceKind::Class)),
            ("record_declaration", container(SpaceKind::Class)),
            ("interface_declaration", container(SpaceKind::Interface)),
            ("trait_declaration", container(SpaceKind::Interface)),
            ("annotation_type_declaration", container(SpaceKind::Interface)),
            ("method_declaration", F),
            ("constructor_declaration", F),
            ("static_initializer", (false, false, true, SpaceKind::Function, false)),
            ("closure", (false, true, false, SpaceKind::Unknown, false)),
            ("field_declaration", M),
        ],
        member::owns,
        &[func_opener::OWNERS, closure::OWNERS, container::OWNERS, member::OWNERS],
        &[],
    );
}

/// Ruby: a `{ }` / `do end` block is a closure and a space except as a
/// stabby lambda's own body, in either spelling (#465, #1257); an
/// `@ivar` / `@@cvar` is a member only as an assignment target.
#[cfg(feature = "ruby")]
#[test]
fn ruby_space_tree_rows() {
    use crate::roles::ruby::{closure, container, func_opener, member};
    let source = b"module M\n  class C\n    @@cv = 1\n    def initialize(x); @x = x; @x += 1; puts @x; end\n    def m; l = ->(y) { y }; l2 = -> do 1 end; [1].each { |i| i }; [1].each do |i| i end; l.call(1); end\n    def self.sm; end\n    class << self; def eig; end; end\n  end\nend\n";
    let mut blocks = Vec::new();
    let mut vars = Vec::new();
    let mut seen_kinds = std::collections::BTreeSet::new();
    for_each_node_with_chain::<crate::RubyCode>(source, |node, chain| {
        use crate::{Checker, Getter, RubyCode};
        let ancestors = Ancestors::known(chain);
        let kind = node.kind();
        let promoted = RubyCode::promotes_to_func_space_with_code(node, source, ancestors);
        let got = (
            RubyCode::is_func(node, ancestors),
            RubyCode::is_closure(node, ancestors),
            promoted,
            RubyCode::get_space_kind_with_code(node, source, ancestors),
            member::owns(node, source, ancestors),
        );
        seen_kinds.insert(node.kind_id());
        let text = node.utf8_text(source).unwrap_or("?");
        if matches!(kind, "block" | "do_block") {
            blocks.push((text.to_owned(), got));
            return;
        }
        if matches!(kind, "instance_variable" | "class_variable") {
            vars.push((text.to_owned(), got.4));
            assert_eq!(
                (got.0, got.1, got.2, got.3),
                (false, false, false, SpaceKind::Unknown)
            );
            return;
        }
        let want = [
            ("program", container(SpaceKind::Unit)),
            ("class", container(SpaceKind::Class)),
            ("singleton_class", container(SpaceKind::Class)),
            ("module", container(SpaceKind::Namespace)),
            ("method", F),
            ("singleton_method", F),
            ("lambda", C),
        ]
        .iter()
        .find(|(k, _)| *k == kind && node.is_named())
        .map_or(NONE, |(_, v)| *v);
        assert_eq!(got, want, "{kind} `{text}`");
        assert_eq!(
            got.3 != SpaceKind::Unknown,
            promoted,
            "{kind}: opens_space vs promotion"
        );
    });
    // The two lambda bodies are neither closure nor space; the two
    // iterator blocks are both, and no block is a named function or a
    // member.
    assert_eq!(
        blocks,
        [
            ("{ y }".to_owned(), NONE),
            ("do 1 end".to_owned(), NONE),
            ("{ |i| i }".to_owned(), C),
            ("do |i| i end".to_owned(), C),
        ]
    );
    // Only the two plain assignments declare; `+=` is an operator
    // assignment, a read, and `puts @x` a read.
    assert_eq!(
        vars,
        [
            ("@@cv".to_owned(), true),
            ("@x".to_owned(), true),
            ("@x".to_owned(), false),
            ("@x".to_owned(), false),
        ]
    );
    for &k in [
        func_opener::OWNERS,
        closure::OWNERS,
        container::OWNERS,
        member::OWNERS,
    ]
    .iter()
    .copied()
    .flatten()
    {
        assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
    }
}

/// The byte-less `Checker::is_func_space` / `Getter::get_space_kind`
/// are the kind table, gates aside — a stabby lambda's own body block
/// answers `(true, Function)` there as it did before the rows, while
/// the `_with_code` spellings the walk reads decline it (#1257). The
/// byte-less pair forwards with a known-empty chain for that reason;
/// `Ancestors::unknown()` would climb and answer the gated result.
#[cfg(feature = "ruby")]
#[test]
fn ruby_byte_less_spellings_stay_the_kind_table() {
    use crate::{Checker, Getter, RubyCode};
    let source = b"l = ->(y) { y }\nm = -> do 1 end\n[1].each { |i| i }\n";
    let mut blocks = Vec::new();
    for_each_node_with_chain::<RubyCode>(source, |node, chain| {
        if !matches!(node.kind(), "block" | "do_block") {
            return;
        }
        let ancestors = Ancestors::known(chain);
        blocks.push((
            node.utf8_text(source).unwrap_or("?").to_owned(),
            (
                RubyCode::is_func_space(node),
                RubyCode::get_space_kind(node),
            ),
            (
                RubyCode::is_func_space_with_code(node, source, ancestors),
                RubyCode::get_space_kind_with_code(node, source, ancestors),
            ),
        ));
    });
    assert_eq!(
        blocks,
        [
            (
                "{ y }".to_owned(),
                (true, SpaceKind::Function),
                (false, SpaceKind::Unknown)
            ),
            (
                "do 1 end".to_owned(),
                (true, SpaceKind::Function),
                (false, SpaceKind::Unknown)
            ),
            (
                "{ |i| i }".to_owned(),
                (true, SpaceKind::Function),
                (true, SpaceKind::Function)
            ),
        ]
    );
}

/// Which languages' closures open a space, read from the rows: a
/// `Closure` owner that the same table's `FuncOpener` row also owns is
/// a closure with a space of its own (contract 2 of #1690). The book's
/// `metrics.md` names the two sets in prose; this is the table it
/// quotes, and the comment there says so.
// The table is the twenty-three rows; rustfmt puts each on five lines.
#[allow(clippy::too_many_lines)]
#[test]
fn closures_that_open_a_space_are_what_the_book_says() {
    use crate::roles;
    let tables: &[(&str, &[u16], &[u16])] = &[
        (
            "bash",
            roles::bash::closure::OWNERS,
            roles::bash::func_opener::OWNERS,
        ),
        (
            "c",
            roles::c::closure::OWNERS,
            roles::c::func_opener::OWNERS,
        ),
        (
            "cpp",
            roles::cpp::closure::OWNERS,
            roles::cpp::func_opener::OWNERS,
        ),
        (
            "csharp",
            roles::csharp::closure::OWNERS,
            roles::csharp::func_opener::OWNERS,
        ),
        (
            "elixir",
            roles::elixir::closure::OWNERS,
            roles::elixir::func_opener::OWNERS,
        ),
        (
            "go",
            roles::go::closure::OWNERS,
            roles::go::func_opener::OWNERS,
        ),
        (
            "groovy",
            roles::groovy::closure::OWNERS,
            roles::groovy::func_opener::OWNERS,
        ),
        (
            "irules",
            roles::irules::closure::OWNERS,
            roles::irules::func_opener::OWNERS,
        ),
        (
            "java",
            roles::java::closure::OWNERS,
            roles::java::func_opener::OWNERS,
        ),
        (
            "javascript",
            roles::javascript::closure::OWNERS,
            roles::javascript::func_opener::OWNERS,
        ),
        (
            "kotlin",
            roles::kotlin::closure::OWNERS,
            roles::kotlin::func_opener::OWNERS,
        ),
        (
            "lua",
            roles::lua::closure::OWNERS,
            roles::lua::func_opener::OWNERS,
        ),
        (
            "mozcpp",
            roles::mozcpp::closure::OWNERS,
            roles::mozcpp::func_opener::OWNERS,
        ),
        (
            "mozjs",
            roles::mozjs::closure::OWNERS,
            roles::mozjs::func_opener::OWNERS,
        ),
        (
            "objc",
            roles::objc::closure::OWNERS,
            roles::objc::func_opener::OWNERS,
        ),
        (
            "perl",
            roles::perl::closure::OWNERS,
            roles::perl::func_opener::OWNERS,
        ),
        (
            "php",
            roles::php::closure::OWNERS,
            roles::php::func_opener::OWNERS,
        ),
        (
            "python",
            roles::python::closure::OWNERS,
            roles::python::func_opener::OWNERS,
        ),
        (
            "ruby",
            roles::ruby::closure::OWNERS,
            roles::ruby::func_opener::OWNERS,
        ),
        (
            "rust",
            roles::rust::closure::OWNERS,
            roles::rust::func_opener::OWNERS,
        ),
        (
            "tcl",
            roles::tcl::closure::OWNERS,
            roles::tcl::func_opener::OWNERS,
        ),
        (
            "tsx",
            roles::tsx::closure::OWNERS,
            roles::tsx::func_opener::OWNERS,
        ),
        (
            "typescript",
            roles::typescript::closure::OWNERS,
            roles::typescript::func_opener::OWNERS,
        ),
    ];
    let (mut with_space, mut without, mut none) = (Vec::new(), Vec::new(), Vec::new());
    for (lang, closures, openers) in tables {
        if closures.is_empty() {
            none.push(*lang);
        } else if closures.iter().all(|k| openers.contains(k)) {
            with_space.push(*lang);
        } else if closures.iter().all(|k| !openers.contains(k)) {
            without.push(*lang);
        } else {
            panic!("{lang}: some closures open a space and some do not");
        }
    }
    assert_eq!(
        with_space,
        [
            "csharp",
            "elixir",
            "go",
            "javascript",
            "lua",
            "mozjs",
            "perl",
            "php",
            "ruby",
            "rust",
            "tsx",
            "typescript"
        ]
    );
    assert_eq!(
        without,
        [
            "cpp", "groovy", "java", "kotlin", "mozcpp", "objc", "python"
        ]
    );
    assert_eq!(none, ["bash", "c", "irules", "tcl"]);
}
