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
//! The *declarator* uses of the same keywords — C#'s
//! `public int this[int i]`, and the `? super String` wildcard bound in
//! Java and Groovy — are deliberately absent from these fixtures: they
//! are operators, so a fixture containing one would put the keyword in
//! both vocabularies and fail the assertion below. They are pinned
//! instead by `csharp_indexer_declaration_keyword_is_not_a_self_reference`,
//! `java_wildcard_super_bound_stays_an_operator` and
//! `groovy_wildcard_super_bound_stays_an_operator` in
//! `src/metrics/halstead.rs`.

use big_code_analysis::{Ast, LANG, MetricsOptions, Source, analyze};

/// The same fixture as [`fixture`], with the *receiver* occurrence of
/// the keyword deleted and nothing else changed — or `None` for a row
/// whose fixture already spells the keyword only as a receiver.
///
/// Two rows need this because their language spells the keyword in the
/// *declaration* as well: Python's `def f(self)` and Rust's `&self`.
/// The vocabulary in [`Ops::operands`] is deduplicated, so in those two
/// the declaration alone satisfies "`self` is an operand" and the
/// receiver assertion is vacuous — deleting `return self.x` leaves the
/// row green. Measured: Python's `n2` is 4 either way, Rust's `self`
/// survives in the vocabulary from `&self`.
///
/// `N2` is the axis that can see the difference, so the test asserts
/// the receiver contributes exactly one operand *occurrence*. If a
/// grammar bump gave the receiver a dedicated kind that no arm
/// classifies, `N2` would not drop and the delta assertion fails —
/// which is the drift these rows exist to catch.
///
/// [`Ops::operands`]: big_code_analysis::Ops::operands
fn receiver_stripped(lang: LANG) -> Option<&'static str> {
    match lang {
        // `return self.x` -> `return x`. Parses clean; N2 5 -> 4.
        LANG::Python => Some("class A:\n    def f(self):\n        return x\n"),
        // `self.x` -> `x`. Parses clean (metrics never type-check);
        // N2 7 -> 6.
        LANG::Rust => Some("struct A { x: i32 }\nimpl A { fn f(&self) -> i32 { x } }\n"),
        _ => None,
    }
}

/// Total operand occurrences (`N2`) for `source` under `lang`.
fn total_operands(lang: LANG, source: &str, name: &str) -> u64 {
    analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name.to_owned())),
        MetricsOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"))
    .metrics
    .halstead
    .total_operands()
}

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
        // lists `Super` among its operators with no parent gate, but the
        // pinned grammar emits `Groovy::Super` only as a `wildcard` bound
        // (`? super T`, a declarator use kept an operator as in Java and
        // left out of this fixture). In receiver position `this` and
        // `super` are a plain `identifier` — verified by dump for
        // `super(1)`, `super.h()`, `A.super.h()` and `super::h` — so the
        // arm never sees a reference, and Groovy is an operand language
        // in fact. This row is what notices if a bump ever routes a
        // reference to that kind (#1419).
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
        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name.clone())))
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

        // Where the declaration spells the keyword too, the assertions
        // above pass on the declaration alone and say nothing about the
        // receiver. Pin the receiver by its occurrence count instead.
        if let Some(stripped) = receiver_stripped(lang) {
            let with = total_operands(lang, source, &name);
            let without = total_operands(lang, stripped, &name);
            assert_eq!(
                with,
                without + 1,
                "{lang:?}: the receiver must contribute exactly one operand occurrence; \
                 N2 was {with} with it and {without} without",
            );
        }
    }

    // Every language is feature-gated, so a build enabling only
    // languages with no self-reference leaves a zero-iteration loop and
    // a test that reports green while asserting nothing. What keeps this
    // from firing spuriously is the `#[cfg(any(…))]` on this file's
    // `mod` declaration in `tests/parity/main.rs`, which names exactly
    // the features whose rows below are `Some` — there is no `cfg` in
    // this file to look up at.
    assert!(
        checked > 0,
        "at least one language feature with a self-reference must be \
         enabled for this test to mean anything",
    );
}
