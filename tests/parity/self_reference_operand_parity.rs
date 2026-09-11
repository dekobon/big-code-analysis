//! Cross-language parity test for the Halstead classification of a
//! **self-reference** (`this` / `self` / `$this`) and a
//! **super-reference** (`super` / `base`).
//!
//! The workspace used to split three ways on this. Java, C# and Kotlin
//! swept the keyword into their operator arm under a
//! `// Operator: … keywords` heading — classification by lexical class
//! — while every other language that classifies one at all called it an
//! operand, so the same source translated between two languages scored
//! different `n1` / `N1` / `n2` / `N2` and every value derived from
//! them. #1380 settled it on **operand**: a member access is
//! `<receiver> <op> <field>`, and billing the receiver as an operator
//! made `this.x` a binary operator with one operand while `p.x` is one
//! operator with two.
//!
//! Nothing but this test holds the three back together. Each language's
//! `get_op_type` is an independent `match`, and a keyword added to the
//! wrong arm of one of them produces no compile error, no clippy
//! warning and no failure in that language's own suite — which is how
//! the split survived until #1361 tripped over it.
//!
//! The fixture table is an exhaustive `match` on [`LANG`], so adding a
//! language variant fails to compile until its row is supplied. A
//! language with no self-reference says so with `None` rather than
//! falling through a wildcard.
//!
//! The two *declarator* uses of the same keyword — C#'s
//! `public int this[int i]` and Java's `? super String` wildcard bound —
//! are deliberately absent from these fixtures: they are operators, so
//! a fixture containing one would put the keyword in both vocabularies
//! and fail the assertion below. They are pinned instead by
//! `csharp_indexer_declaration_keyword_is_not_a_self_reference` and
//! `java_wildcard_super_bound_stays_an_operator` in
//! `src/metrics/halstead.rs`.

use big_code_analysis::{Ast, LANG, Source};

/// Returns `(source, extension, keywords)` for a language that spells a
/// self- or super-reference, or `None` for one that does not.
///
/// Every keyword listed must be the *value* form — a receiver standing
/// where a variable would stand. The source spelling is what reaches
/// the vocabulary, so PHP's row names `$this` and C#'s names `base`.
///
/// Five rows reach the operand vocabulary through the generic
/// identifier arm rather than through a decision anyone made: Groovy,
/// Objective-C, Python and Lua spell the keyword as a plain
/// `identifier` (kind 1), and PHP's `$this` is a `variable_name`. Each
/// says so at its row. They still earn their place — each is a guard
/// against a grammar bump promoting the keyword to a kind of its own
/// and the language falling out of the majority unnoticed.
fn fixture(lang: LANG) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, so a new language cannot be added
    // without deciding whether it has a self-reference. The repo's own
    // `.bcaignore` excludes `./tests/**`, so this marker is for the
    // per-edit `bca check` hook rather than for the self-scan gate.
    // bca: suppress(cyclomatic)
    let row: (&str, &str, &[&str]) = match lang {
        LANG::Javascript | LANG::Mozjs => (
            "class A extends B {\n  f() { return this.x; }\n  g() { return super.h(); }\n}\n",
            "js",
            &["this", "super"],
        ),
        LANG::Typescript => (
            "class A extends B {\n  f(): number { return this.x; }\n  \
             g(): number { return super.h(); }\n}\n",
            "ts",
            &["this", "super"],
        ),
        LANG::Tsx => (
            "class A extends B {\n  f(): number { return this.x; }\n  \
             g(): number { return super.h(); }\n}\n",
            "tsx",
            &["this", "super"],
        ),
        LANG::Java => (
            "class A extends B {\n  int f() { return this.x; }\n  \
             int g() { return super.h(); }\n}\n",
            "java",
            &["this", "super"],
        ),
        LANG::Kotlin => (
            "class A : B() {\n  fun f() = this.x\n  fun g() = super.h()\n}\n",
            "kt",
            &["this", "super"],
        ),
        // C# spells the super-reference `base`.
        LANG::Csharp => (
            "class A : B {\n  int F() { return this.x; }\n  int G() { return base.H(); }\n}\n",
            "cs",
            &["this", "base"],
        ),
        // Grammar accident, and the interesting one: `getter/groovy.rs`
        // *does* list `Super` among its operators, but the grammar emits
        // a plain `identifier` for both `this` and `super` in receiver
        // position — verified by dump for `super(1)`, `super.h()`,
        // `A.super.h()` and `super::h` — so that arm is dead at the
        // current pin and Groovy is an operand language in fact. This
        // row is what notices if a bump ever wakes the arm up.
        LANG::Groovy => (
            "class A extends B {\n  def f() { return this.x }\n  \
             def g() { return super.h() }\n}\n",
            "groovy",
            &["this", "super"],
        ),
        LANG::Ruby => (
            "class A < B\n  def f; self.x; end\n  def g; super; end\nend\n",
            "rb",
            &["self", "super"],
        ),
        // Rust's `self` is a receiver; it has no super-reference
        // (`super::` is a module path, not an object).
        LANG::Rust => (
            "struct A { x: i32 }\nimpl A { fn f(&self) -> i32 { self.x } }\n",
            "rs",
            &["self"],
        ),
        // C++ `this` is a pointer, so the fixture dereferences with
        // `->`. #1361 added the arm that classifies it at all.
        LANG::Cpp | LANG::Mozcpp => (
            "struct S { int x; int f() { return this->x; } };\n",
            "cpp",
            &["this"],
        ),
        // Grammar accident: `self` is a plain `identifier`, so no arm is
        // involved.
        LANG::Objc => (
            "@implementation S\n- (int)f { return [self g]; }\n- (int)g { return 1; }\n@end\n",
            "m",
            &["self"],
        ),
        // PHP's `$this` is a `variable_name`, not a keyword, so it is an
        // operand by grammar rather than by an arm — the fixture pins
        // that it stays one. PHP's `Zelf` / `Parent` arms cover the
        // unrelated `self::` / `parent::` class references, which are
        // scope-resolution operators and deliberately untouched (#1380).
        LANG::Php => (
            "<?php\nclass A { public $x; function f() { return $this->x; } }\n",
            "php",
            &["$this"],
        ),
        // Grammar accidents, as Groovy and Objective-C are: `self` is a
        // plain `identifier` in both grammars.
        LANG::Python => (
            "class A:\n    def f(self):\n        return self.x\n",
            "py",
            &["self"],
        ),
        LANG::Lua => (
            "local S = {}\nfunction S:m1() return self.x end\n",
            "lua",
            &["self"],
        ),
        // No self-reference to classify. Go names the receiver in the
        // method signature; C, Bash, Tcl, iRules and Perl have no
        // object receiver at all (Perl's `$self` is an ordinary lexical,
        // indistinguishable from any other variable); Elixir spells the
        // enclosing module `__MODULE__`, a compile-time macro rather
        // than a receiver; and the two helper grammars parse fragments.
        LANG::Go
        | LANG::C
        | LANG::Bash
        | LANG::Tcl
        | LANG::Irules
        | LANG::Perl
        | LANG::Elixir
        | LANG::Ccomment
        | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn every_language_bills_a_self_reference_as_an_operand() {
    let mut checked = 0;

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some((source, ext, keywords)) = fixture(lang) else {
            continue;
        };
        checked += 1;

        let name = format!("parity.{ext}");
        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name)))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));

        for keyword in keywords {
            assert!(
                ops.operands.iter().any(|o| o == keyword),
                "{lang:?}: `{keyword}` must be a Halstead operand; operands were {:?}",
                ops.operands,
            );
            // The other half of the parity claim. A keyword listed in
            // both arms is billed twice — once into `n1`/`N1` and once
            // into `n2`/`N2` — and would satisfy the assertion above.
            assert!(
                !ops.operators.iter().any(|o| o == keyword),
                "{lang:?}: `{keyword}` must not be a Halstead operator; operators were {:?}",
                ops.operators,
            );
        }
    }

    // Every language is feature-gated, so a build enabling only
    // languages with no self-reference leaves a zero-iteration loop and
    // a test that reports green while asserting nothing. The `cfg` above
    // keeps this from firing spuriously: it names exactly the features
    // whose rows are `Some`.
    assert!(
        checked > 0,
        "at least one language feature with a self-reference must be \
         enabled for this test to mean anything",
    );
}
