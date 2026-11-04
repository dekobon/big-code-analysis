//! The population `average` / `min` / `max` describe for `cyclomatic`,
//! `cognitive` and `nexits` (#1284).
//!
//! All three fold numerator, divisor and extrema from the spaces whose
//! recorded kind is [`SpaceKind::Function`], so `min <= average <= max`
//! holds on every space by construction. Before #1284 the divisor was
//! the function count (`function_spaces` for cyclomatic, `nom.total()`
//! for the other two) while the numerator and the extrema folded every
//! space's own value, the file unit and containers included, which put
//! `fn f() {}` at `average 2.0` against `min 1, max 1` and a function
//! holding one Python lambda at `average 7.0` against `min 14, max 14`.
//!
//! Each `// expected:` comment below carries the value `main` reported
//! before the fix and the population reasoning for the new one, so the
//! anchor is not the rule under test restated (lesson 6). The fixtures
//! are chosen for the two reasons the old divisor and the new population
//! differ: a closure that opens no space (Python `lambda`, counted by
//! `nom` but with no own value to fold) and a `Function` space `nom`
//! does not count at all (Java's `static {}` initializer, Kotlin's
//! property accessors and `init`, JavaScript's `static {}` class block).

// Test-side counts cast to `f64` to re-derive a mean; every value is a
// small fixture count.
#![allow(clippy::cast_precision_loss)]

use crate::spaces::{FuncSpace, SpaceKind};
use crate::test_support::{assert_fixtures_present, space_verbatim};
use crate::{LANG, MetricsOptions};

fn analyse(lang: LANG, source: &str) -> FuncSpace {
    space_verbatim(lang, source.as_bytes(), MetricsOptions::default())
}

/// `(sum, average, min, max)` of one metric on one space.
type Aggregate = (u64, f64, u64, u64);

fn cyclomatic(space: &FuncSpace) -> Aggregate {
    let s = &space.metrics.cyclomatic;
    (
        s.cyclomatic_sum(),
        s.cyclomatic_average(),
        s.cyclomatic_min(),
        s.cyclomatic_max(),
    )
}

fn cognitive(space: &FuncSpace) -> Aggregate {
    let s = &space.metrics.cognitive;
    (
        s.cognitive_sum(),
        s.cognitive_average(),
        s.cognitive_min(),
        s.cognitive_max(),
    )
}

fn nexits(space: &FuncSpace) -> Aggregate {
    let s = &space.metrics.nexits;
    (
        s.nexits_sum(),
        s.nexits_average(),
        s.nexits_min(),
        s.nexits_max(),
    )
}

/// Every space in the tree, root first.
fn flatten<'a>(space: &'a FuncSpace, out: &mut Vec<&'a FuncSpace>) {
    out.push(space);
    for child in &space.spaces {
        flatten(child, out);
    }
}

/// The names of the `Function` spaces under `space`, root first.
#[cfg(any(
    feature = "c",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "python",
    feature = "rust",
))]
fn function_names(space: &FuncSpace) -> Vec<&str> {
    let mut all = Vec::new();
    flatten(space, &mut all);
    all.iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .map(|s| s.name.as_deref().unwrap_or("<unnamed>"))
        .collect()
}

/// The issue's first reproducer: one function, no branches.
#[cfg(feature = "rust")]
#[test]
fn rust_single_function_average_is_its_own_value() {
    let root = analyse(LANG::Rust, "fn f() {}\n");
    // expected: sum = unit 1 + f 1 = 2, unchanged. The one function
    // space scores 1, so average = min = max = 1. Was average 2.0 on
    // main (sum 2 over the one function), above max 1.
    assert_eq!(cyclomatic(&root), (2, 1.0, 1, 1));
    assert_eq!(function_names(&root), ["f"]);
}

/// The issue's second reproducer: the unit's own cognitive 0 no longer
/// pins the file-level `min`.
#[cfg(feature = "python")]
#[test]
fn python_file_min_is_not_pinned_by_the_unit() {
    let root = analyse(
        LANG::Python,
        "def f(x):\n    if x > 1:\n        return 1\n    return x\n",
    );
    // expected: cognitive sum 1 (the `if`), on the one function space,
    // so min = max = average = 1. Was min 0 on main, the unit's own
    // value folded into the extrema.
    assert_eq!(cognitive(&root), (1, 1.0, 1, 1));
    // expected: nexits sum 2 (both `return`s), likewise; was min 0.
    assert_eq!(nexits(&root), (2, 2.0, 2, 2));
    // expected: cyclomatic sum = unit 1 + f (1 + if 1) = 3; the one
    // function scores 2, so average = min = max = 2. Was average 3.0
    // and min 1 on main.
    assert_eq!(cyclomatic(&root), (3, 2.0, 2, 2));
}

/// The issue's third reproducer: a spaceless closure. A Python `lambda`
/// is a `nom` closure but opens no space, so its decisions are part of
/// the enclosing function's own value and it is no divisor unit.
#[cfg(feature = "python")]
#[test]
fn python_lambda_is_not_a_divisor_unit() {
    let root = analyse(
        LANG::Python,
        "def f(xs):\n    if xs:\n        return sorted(xs, key=lambda x: x if x else 0)\n    return []\n",
    );
    assert_eq!(root.metrics.nom.closures_sum(), 1);
    assert_eq!(function_names(&root), ["f"]);
    let f = &root.spaces[0];
    // expected: f's own cognitive is 4 — `if` +1, the lambda's ternary
    // +1 with nesting +2 (inside the `if` and the lambda) — and f is the
    // only function space, so average = min = max = 4 on f and on the
    // file. Was average 2.0 on main against min 4 / max 4: a divisor
    // of `nom.total()` 2 for a numerator only f contributed to.
    assert_eq!(cognitive(f), (4, 4.0, 4, 4));
    assert_eq!(cognitive(&root), (4, 4.0, 4, 4));
    // expected: nexits 2 (two `return`s) over the one function; was
    // average 1.0 on main for the same reason.
    assert_eq!(nexits(f), (2, 2.0, 2, 2));
    assert_eq!(nexits(&root), (2, 2.0, 2, 2));
    // expected: f's cyclomatic is 1 + if + ternary = 3; the file's sum
    // adds the unit's 1. Was average 4.0 and min 1 at file level.
    assert_eq!(cyclomatic(f), (3, 3.0, 3, 3));
    assert_eq!(cyclomatic(&root), (4, 3.0, 3, 3));
}

/// Module-level control flow and exits are the unit's own value: in
/// `sum`, in no per-function aggregate. The one fixture here whose
/// non-function space scores above zero on cognitive and nexits, so it
/// is the one that tells a function-only numerator from `sum` for those
/// two — on every other fixture the unit's and containers' own values
/// are 0 there, and only cyclomatic's base 1 separates the populations.
#[cfg(feature = "python")]
#[test]
fn python_module_level_flow_stays_out_of_the_numerator() {
    let root = analyse(
        LANG::Python,
        "import sys\nif len(sys.argv) < 2:\n    raise SystemExit(1)\n\ndef f(x):\n    if x:\n        return 1\n    return 0\n",
    );
    assert_eq!(function_names(&root), ["f"]);
    // expected: the unit's own cognitive is 1 (the module `if`) and
    // f's is 1, so sum 2 but average = min = max = 1 over the one
    // function. Was average 2.0 and min 0 on main.
    assert_eq!(root.metrics.cognitive.cognitive(), 1);
    assert_eq!(cognitive(&root), (2, 1.0, 1, 1));
    // expected: the module `raise` is the unit's own exit, f's two
    // `return`s are f's: sum 3, average = min = max = 2. Was average
    // 3.0 (above max) and min 0 on main.
    assert_eq!(root.metrics.nexits.nexits(), 1);
    assert_eq!(nexits(&root), (3, 2.0, 2, 2));
    // expected: the unit's own cyclomatic is 1 + the module `if` = 2,
    // f's 1 + its `if` = 2: sum 4, average = min = max = 2. Was
    // average 4.0 on main.
    assert_eq!(cyclomatic(&root), (4, 2.0, 2, 2));
}

/// A closure that opens its own space is a population member like any
/// named function, and the enclosing function's own value excludes it.
#[cfg(feature = "rust")]
#[test]
fn rust_closure_space_is_a_population_member() {
    let root = analyse(
        LANG::Rust,
        "fn f(xs: &[i32]) -> i32 {\n    let g = |x: i32| if x > 0 { x } else { 0 };\n    if xs.is_empty() { return 0; }\n    g(xs[0])\n}\n",
    );
    assert_eq!(function_names(&root), ["f", "<anonymous>"]);
    // expected: f's own cyclomatic is 1 + its `if` = 2 and the
    // closure's 1 + its `if` = 2, so the file averages (2 + 2) / 2 =
    // 2.0 with min = max = 2; sum adds the unit's 1 = 5. Was average
    // 2.5 (5 / 2) and min 1 on main.
    assert_eq!(cyclomatic(&root), (5, 2.0, 2, 2));
    // expected: cognitive — f's own: `if` +1 = 1; the closure's: `if`
    // +1 nested one level under the closure (+1) plus its `else` +1 =
    // 3. File: sum 4, average 2.0, min 1, max 3. Was min 0 on main.
    assert_eq!(cognitive(&root), (4, 2.0, 1, 3));
    // expected: nexits — f's `return` 1, the closure 0: sum 1, average
    // 0.5, min 0, max 1 — unchanged from main, where the unit's 0
    // happened to coincide with the closure's.
    assert_eq!(nexits(&root), (1, 0.5, 0, 1));
}

/// A language with no closures: the only change is that the unit's
/// base 1 leaves the numerator and the extrema.
#[cfg(feature = "c")]
#[test]
fn c_unit_base_leaves_the_numerator() {
    let root = analyse(
        LANG::C,
        "int f(int x) {\n    if (x > 0) return 1;\n    return 0;\n}\nint g(int x) {\n    return x;\n}\n",
    );
    assert_eq!(function_names(&root), ["f", "g"]);
    // expected: f scores 2, g scores 1, so average (2 + 1) / 2 = 1.5,
    // min 1, max 2; sum adds the unit's 1 = 4. Was average 2.0 (4 / 2)
    // on main.
    assert_eq!(cyclomatic(&root), (4, 1.5, 1, 2));
    // expected: nexits f 2, g 1: sum 3, average 1.5, min 1, max 2. Was
    // min 0 on main, from the unit.
    assert_eq!(nexits(&root), (3, 1.5, 1, 2));
    // expected: cognitive f 1, g 0: (1, 0.5, 0, 1), unchanged from
    // main — g's 0 coincides with the unit's.
    assert_eq!(cognitive(&root), (1, 0.5, 0, 1));
}

/// Java's `static {}` initializer is a `Function` space `nom` does not
/// count, and the anonymous class is a container nested in a field: the
/// two reasons the old `nom.total()` divisor and the function-space
/// population differ beyond spaceless lambdas.
#[cfg(feature = "java")]
#[test]
fn java_static_initializer_and_anonymous_class() {
    let root = analyse(
        LANG::Java,
        "class A {\n    static { int x = 1; }\n    Runnable r = new Runnable() {\n        public void run() { if (true) { } }\n    };\n    int f(int x) { if (x > 0) return 1; return 0; }\n}\n",
    );
    assert_eq!(function_names(&root), ["<static-init>", "run", "f"]);
    assert_eq!(
        root.metrics.nom.total(),
        2,
        "nom does not count the static block"
    );
    // expected: three function spaces scoring 1 (static block), 2
    // (`run`), 2 (`f`): average 5 / 3, min 1, max 2. sum adds the
    // unit, class A and the anonymous class at 1 each = 8. Was average
    // 8 / 3 ≈ 2.67 on main, above max.
    assert_eq!(cyclomatic(&root), (8, 5.0 / 3.0, 1, 2));
    // expected: cognitive — static block 0, `run` 1, `f` 1: sum 2,
    // average 2 / 3, min 0, max 1. Was average 1.0 on main, the divisor
    // being `nom.total()` 2 rather than the three function spaces.
    assert_eq!(cognitive(&root), (2, 2.0 / 3.0, 0, 1));
    // expected: nexits — only `f` returns, twice: sum 2, average 2 / 3,
    // min 0, max 2. Was average 1.0 on main.
    assert_eq!(nexits(&root), (2, 2.0 / 3.0, 0, 2));
    // The class aggregates the same three: it contains every function.
    let class = &root.spaces[0];
    assert_eq!(class.kind, SpaceKind::Class);
    assert_eq!(cyclomatic(class), (7, 5.0 / 3.0, 1, 2));
    assert_eq!(cognitive(class), (2, 2.0 / 3.0, 0, 1));
}

/// Kotlin's property accessors and `init` block are `Function` spaces
/// with no `nom` count.
#[cfg(feature = "kotlin")]
#[test]
fn kotlin_accessors_and_init_are_population_members() {
    let root = analyse(
        LANG::Kotlin,
        "class A {\n    init { if (true) { } }\n    var p: Int = 0\n        get() = if (field > 0) field else 0\n        set(v) { field = v }\n    fun f(x: Int): Int { if (x > 0) return 1; return 0 }\n}\n",
    );
    assert_eq!(function_names(&root), ["<init>", "<get>", "<set>", "f"]);
    assert_eq!(root.metrics.nom.total(), 1, "nom counts only `f`");
    // expected: four function spaces scoring 2, 2, 1, 2: average 7 / 4
    // = 1.75, min 1, max 2. sum adds the unit and the class = 9. Was
    // average 2.25 (9 / 4) on main, above max.
    assert_eq!(cyclomatic(&root), (9, 1.75, 1, 2));
    // expected: cognitive — init 1, get 2 (`if` +1, `else` +1), set 0,
    // f 1: sum 4, average 1.0, min 0, max 2. Was average 4.0 on main,
    // the divisor being `nom.total()` 1 — twice max.
    assert_eq!(cognitive(&root), (4, 1.0, 0, 2));
    // expected: nexits — only `f` returns, twice: sum 2, average 0.5,
    // min 0, max 2. Was average 2.0 on main over `nom.total()` 1.
    assert_eq!(nexits(&root), (2, 0.5, 0, 2));
}

/// JavaScript's `static {}` class block is a `Function` space with no
/// `nom` count.
#[cfg(feature = "javascript")]
#[test]
fn javascript_static_block_is_a_population_member() {
    let root = analyse(
        LANG::Javascript,
        "class A {\n    static { if (true) { } }\n    f(x) { if (x > 0) return 1; return 0; }\n}\n",
    );
    assert_eq!(function_names(&root), ["<static-init>", "f"]);
    assert_eq!(root.metrics.nom.total(), 1, "nom counts only `f`");
    // expected: two function spaces scoring 2 each: average = min = max
    // = 2; sum adds the unit and the class = 6. Was average 3.0 (6 / 2)
    // and min 1 on main.
    assert_eq!(cyclomatic(&root), (6, 2.0, 2, 2));
    // expected: cognitive — static block 1, f 1: sum 2, average 1.0,
    // min 1, max 1. Was average 2.0 (over `nom.total()` 1) and min 0
    // on main.
    assert_eq!(cognitive(&root), (2, 1.0, 1, 1));
    // expected: nexits — f returns twice, the static block never: sum
    // 2, average 1.0, min 0, max 2. Was average 2.0 on main.
    assert_eq!(nexits(&root), (2, 1.0, 0, 2));
}

/// A file with no function space at all: the extrema stay at their
/// collapsed `0` and the average is `0.0`, not the unit's base 1.
#[cfg(feature = "rust")]
#[test]
fn no_function_space_reports_zero_aggregates() {
    let root = analyse(
        LANG::Rust,
        "struct S;\nconst N: u32 = if true { 1 } else { 2 };\n",
    );
    assert_eq!(function_names(&root), Vec::<&str>::new());
    // expected: the unit scores 1 + the const's `if` = 2 on its own,
    // and that is the whole sum; with no function in the population the
    // average is 0.0 and min = max = 0. Was average 2.0, min 2, max 2
    // on main — numbers describing a function that does not exist.
    assert_eq!(cyclomatic(&root), (2, 0.0, 0, 0));
    // expected: cognitive sum 2 (`if` +1, `else` +1) on the unit alone;
    // was average 2.0, min 2, max 2 on main.
    assert_eq!(cognitive(&root), (2, 0.0, 0, 0));
    assert_eq!(nexits(&root), (0, 0.0, 0, 0));
}

/// A multi-language fixture set over which `min <= average <= max`
/// holds on every space for all three metrics, and the average is the
/// arithmetic mean of the `Function` spaces' own values re-derived from
/// the tree. Each fixture mixes containers, named functions and the
/// closure form the language has, so the invariant is asked where the
/// two populations used to differ.
struct Fixture {
    lang: LANG,
    source: &'static str,
}

const FIXTURES: &[Fixture] = &[
    #[cfg(feature = "rust")]
    Fixture {
        lang: LANG::Rust,
        source: "struct S;\nimpl S {\n    fn a(&self, x: i32) -> i32 { if x > 0 { return 1; } 0 }\n    fn b(&self) -> i32 { let c = || if true { 1 } else { 2 }; c() }\n}\nfn free() {}\n",
    },
    #[cfg(feature = "python")]
    Fixture {
        lang: LANG::Python,
        source: "class C:\n    def m(self, xs):\n        if xs:\n            return [x for x in xs if x]\n        return list(map(lambda y: y if y else 0, xs))\n\ndef f():\n    return 1\n",
    },
    #[cfg(feature = "javascript")]
    Fixture {
        lang: LANG::Javascript,
        source: "class A {\n    static { if (true) { } }\n    m(x) { return x ? () => { if (x) return 1; return 2; } : null; }\n}\nfunction f() {}\n",
    },
    #[cfg(feature = "java")]
    Fixture {
        lang: LANG::Java,
        source: "class A {\n    static { int x = 1; }\n    Runnable r = new Runnable() { public void run() { if (true) { } } };\n    int f(int x) { if (x > 0) return 1; return 0; }\n}\n",
    },
    #[cfg(feature = "kotlin")]
    Fixture {
        lang: LANG::Kotlin,
        source: "class A {\n    init { if (true) { } }\n    var p: Int = 0\n        get() = if (field > 0) field else 0\n    fun f(x: Int): Int { if (x > 0) return 1; return 0 }\n}\n",
    },
    #[cfg(feature = "c")]
    Fixture {
        lang: LANG::C,
        source: "int f(int x) { if (x > 0) return 1; return 0; }\nint g(int x) { return x; }\n",
    },
    #[cfg(feature = "cpp")]
    Fixture {
        lang: LANG::Cpp,
        source: "namespace n {\nstruct S { int m(int x) { auto l = [](int y) { return y ? 1 : 0; }; if (x) return l(x); return 0; } };\n}\n",
    },
    #[cfg(feature = "csharp")]
    Fixture {
        lang: LANG::Csharp,
        source: "class A {\n    int F(int x) { System.Func<int, int> l = y => y > 0 ? 1 : 0; if (x > 0) return l(x); return 0; }\n    int W => 1;\n}\n",
    },
    #[cfg(feature = "go")]
    Fixture {
        lang: LANG::Go,
        source: "package p\nfunc f(x int) int {\n\tg := func(y int) int { if y > 0 { return 1 }; return 0 }\n\tif x > 0 { return g(x) }\n\treturn 0\n}\n",
    },
    #[cfg(feature = "ruby")]
    Fixture {
        lang: LANG::Ruby,
        source: "class A\n  def m(xs)\n    xs.each { |x| return x if x > 0 }\n    nil\n  end\nend\n",
    },
    #[cfg(feature = "typescript")]
    Fixture {
        lang: LANG::Typescript,
        source: "class A {\n    static { if (true) { } }\n    m(x: number): number { const l = (y: number) => y ? 1 : 0; if (x) return l(x); return 0; }\n}\n",
    },
];

/// The `own` value of every `Function` space under `space`, so the
/// aggregate it reports can be re-derived from the tree.
fn population(space: &FuncSpace, own: fn(&FuncSpace) -> u64) -> Vec<u64> {
    let mut all = Vec::new();
    flatten(space, &mut all);
    all.iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .map(|s| own(s))
        .collect()
}

#[track_caller]
fn assert_population(lang: LANG, space: &FuncSpace, name: &str, agg: Aggregate, own: &[u64]) {
    let (_, average, min, max) = agg;
    let where_ = format!("{lang:?} {name} on {:?}", space.name);
    assert!(
        min as f64 <= average && average <= max as f64,
        "{where_}: average {average} outside [{min}, {max}]"
    );
    let expected_average = if own.is_empty() {
        0.0
    } else {
        own.iter().sum::<u64>() as f64 / own.len() as f64
    };
    assert_eq!(average, expected_average, "{where_}: mean of {own:?}");
    assert_eq!(
        min,
        own.iter().copied().min().unwrap_or(0),
        "{where_}: min of {own:?}"
    );
    assert_eq!(
        max,
        own.iter().copied().max().unwrap_or(0),
        "{where_}: max of {own:?}"
    );
}

/// Every space of every fixture reports the mean, minimum and maximum of
/// its subtree's `Function` spaces for all three metrics.
#[cfg(any(
    feature = "c",
    feature = "cpp",
    feature = "csharp",
    feature = "go",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "typescript"
))]
#[test]
fn average_lies_within_min_and_max_on_every_space() {
    assert_fixtures_present(FIXTURES);
    let mut function_spaces_seen = 0;
    let mut populations_differed = 0;
    for fixture in FIXTURES {
        let root = analyse(fixture.lang, fixture.source);
        let mut all = Vec::new();
        flatten(&root, &mut all);
        for space in all {
            let own = population(space, |s| s.metrics.cyclomatic.cyclomatic());
            function_spaces_seen += own.len();
            // A space whose all-space sum exceeds its functions' own
            // values is one the old fold would have read differently.
            if cyclomatic(space).0 > own.iter().sum::<u64>() {
                populations_differed += 1;
            }
            assert_population(fixture.lang, space, "cyclomatic", cyclomatic(space), &own);
            assert_population(
                fixture.lang,
                space,
                "cognitive",
                cognitive(space),
                &population(space, |s| s.metrics.cognitive.cognitive()),
            );
            assert_population(
                fixture.lang,
                space,
                "nexits",
                nexits(space),
                &population(space, |s| s.metrics.nexits.nexits()),
            );
        }
    }
    assert!(function_spaces_seen > 0, "no function space in any fixture");
    // Every fixture's unit carries a base 1 no function supplies, so the
    // count is at least the fixture count; a fixture that stopped
    // having one would make the invariant vacuous for its language.
    assert!(
        populations_differed >= FIXTURES.len(),
        "{populations_differed} of {} fixtures asked the invariant where the populations differ",
        FIXTURES.len()
    );
}
