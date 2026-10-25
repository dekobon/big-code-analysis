//! The space-tree rows of the kind-only languages, pinned independently
//! (#1696): for each language, a fixture spelling every `FuncOpener`,
//! `Closure`, `Container` and `Member` kind, and the expected
//! `(is_func, is_closure, promoted, SpaceKind, is_member)` per kind as
//! the classifiers answered before they read the rows. A kind absent
//! from a language's table answers all-false; every declared owner must
//! be met by the fixture.

use crate::test_support::for_each_node_with_chain;
use crate::{Ancestors, Checker, Getter, LanguageInfo, Node, SpaceKind};

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "irules",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
type Answers = (bool, bool, bool, SpaceKind, bool);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "irules",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
type OpensSpace = fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> bool;

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "irules",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
const F: Answers = (true, false, true, SpaceKind::Function, false);
#[cfg(any(
    feature = "c",
    feature = "go",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "rust",
))]
const C: Answers = (false, true, true, SpaceKind::Function, false);
#[cfg(any(feature = "go", feature = "php", feature = "rust"))]
const M: Answers = (false, false, false, SpaceKind::Unknown, true);
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "irules",
    feature = "lua",
    feature = "perl",
    feature = "php",
    feature = "rust",
    feature = "tcl",
))]
const NONE: Answers = (false, false, false, SpaceKind::Unknown, false);

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "go",
    feature = "irules",
    feature = "lua",
    feature = "perl",
    feature = "php",
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
    feature = "irules",
    feature = "lua",
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
        seen_kinds.insert(node.kind_id());
    });
    for &k in owners.iter().copied().flatten() {
        if defensive.contains(&k) {
            assert!(
                !seen_kinds.contains(&k),
                "defensive kind {k} is now emitted; its row needs a look"
            );
        } else {
            assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
        }
    }
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
