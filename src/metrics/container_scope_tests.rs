//! Which spaces carry an `npm` / `npa` block in serialized output.
//!
//! These assertions cannot be written with `check_metrics`. That helper
//! hands back `spaces::CodeMetrics`, whose `npm` / `npa` fields are plain
//! structs, and the `insta` snapshots in `npm.rs` / `npa.rs` serialize
//! those structs through `serialize_via_wire!`, which bypasses the
//! `Option`. Both surfaces are blind to the emission gate by design — it
//! lives one layer out, in `wire::CodeMetrics::from`, and the only way to
//! observe it is to serialize a whole [`FuncSpace`] and look at the keys.
//!
//! That blindness is exactly how #1197 shipped: `Npm` and `Npa` enabled
//! themselves on `Checker::is_func_space`, which means "opens a space",
//! not "is a scope that owns members". Seven of the ten languages
//! therefore emitted an all-zero block on every ordinary method, and
//! #1184 added Kotlin property accessors and `init` / `static` blocks to
//! the list, next to sibling methods that had none.
//!
//! The rule they now follow is [`SpaceKind::is_member_scope`], which
//! `wmc` already used: containers and the file unit carry the block, a
//! function space never does. Both directions are asserted below,
//! because narrowing too far would silently delete the whole-file
//! roll-up rather than the all-zero noise.

use serde_json::Value;

use crate::spaces::SpaceKind;
use crate::test_support::{assert_fixtures_present, space_verbatim};
use crate::{LANG, MetricsOptions};

/// A serialized space, flattened to what these tests assert on.
struct Emitted {
    kind: SpaceKind,
    /// `None` for the unit root, which `space_verbatim` analyses without a
    /// filename. Modelled as absent rather than defaulted to `""` so a
    /// *nested* space that lost its name fails a lookup below instead of
    /// quietly matching nothing.
    name: Option<String>,
    has_npm: bool,
    has_npa: bool,
}

/// Analyses `source` and flattens every space in the serialized tree,
/// root first.
///
/// Serializing the [`FuncSpace`](crate::spaces::FuncSpace) rather than
/// reading `space.metrics` is the point: `metrics.npm` is always present
/// as a struct, and only the JSON key is gated.
fn emitted_spaces(lang: LANG, source: &str) -> Vec<Emitted> {
    let space = space_verbatim(lang, source.as_bytes(), MetricsOptions::default());
    let value = serde_json::to_value(&space).expect("FuncSpace must serialize");
    let mut out = Vec::new();
    flatten(&value, &mut out);
    out
}

fn flatten(value: &Value, out: &mut Vec<Emitted>) {
    let metrics = value["metrics"]
        .as_object()
        .expect("every space serializes a metrics object");
    // `expect` rather than a default on `kind`: a space missing it would
    // read as `Unknown`, which no assertion below could tell apart from a
    // space these tests are meant to skip.
    let kind = value["kind"]
        .as_str()
        .expect("every space serializes a kind");
    out.push(Emitted {
        kind: SpaceKind::from_serialized(kind),
        name: value["name"].as_str().map(str::to_owned),
        has_npm: metrics.contains_key("npm"),
        has_npa: metrics.contains_key("npa"),
    });
    for child in value["spaces"].as_array().into_iter().flatten() {
        flatten(child, out);
    }
}

/// One fixture per language whose `Npm` / `Npa` impl enables on
/// [`opens_member_scope`](super::opens_member_scope).
///
/// Each carries a container with one public method and one public
/// attribute, at least one ordinary method, and — where the grammar has
/// one — a #1184 construct (`get`/`set`/`init`/`static`), so a single
/// fixture exercises both halves of the defect.
struct Fixture {
    lang: LANG,
    /// Names of the container spaces that must carry both blocks.
    containers: &'static [&'static str],
    source: &'static str,
}

/// The fixture for `lang`.
///
/// Looked up by language rather than by index, so reordering [`FIXTURES`]
/// cannot silently pair a language with another's source.
fn fixture_source(lang: LANG) -> &'static str {
    FIXTURES
        .iter()
        .find(|f| f.lang == lang)
        .unwrap_or_else(|| panic!("no fixture for {lang:?}"))
        .source
}

/// Every space as `(kind, name)`, for an assertion message.
fn summary(spaces: &[Emitted]) -> Vec<(SpaceKind, Option<&str>)> {
    spaces.iter().map(|s| (s.kind, s.name.as_deref())).collect()
}

/// The one space named `name`, or a failure naming every space there was.
///
/// Asserting the match count rather than taking the first hit is what
/// stops a rename or a lost space from making the caller's assertions
/// vacuous.
#[track_caller]
fn only_space<'a>(lang: LANG, spaces: &'a [Emitted], name: &str) -> &'a Emitted {
    let found: Vec<&Emitted> = spaces
        .iter()
        .filter(|s| s.name.as_deref() == Some(name))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{lang:?}: expected exactly one space named {name:?}, got {:?}",
        summary(spaces)
    );
    found[0]
}

const FIXTURES: &[Fixture] = &[
    #[cfg(feature = "kotlin")]
    Fixture {
        lang: LANG::Kotlin,
        containers: &["C", "I"],
        source: "\
interface I {
    fun q(): Int
}
class C : I {
    var p: Int = 0
        get() = field
        set(v) { field = v }
    init { p = 1 }
    override fun q(): Int { return p }
}
",
    },
    #[cfg(feature = "java")]
    Fixture {
        lang: LANG::Java,
        containers: &["C", "I"],
        source: "\
interface I {
    int q();
}
class C implements I {
    public int a = 1;
    static { System.out.println(\"x\"); }
    public int q() { return a; }
}
",
    },
    #[cfg(feature = "groovy")]
    Fixture {
        lang: LANG::Groovy,
        containers: &["C", "I"],
        source: "\
interface I {
    int q()
}
class C implements I {
    public int a = 1
    static { println 'x' }
    int q() { return a }
}
",
    },
    #[cfg(feature = "javascript")]
    Fixture {
        lang: LANG::Javascript,
        containers: &["C"],
        source: "\
class C {
    a = 1;
    static { this.b = 2; }
    q() { return this.a; }
}
function top(x) { return x; }
",
    },
    #[cfg(feature = "mozjs")]
    Fixture {
        lang: LANG::Mozjs,
        containers: &["C"],
        source: "\
class C {
    a = 1;
    static { this.b = 2; }
    q() { return this.a; }
}
function top(x) { return x; }
",
    },
    #[cfg(feature = "typescript")]
    Fixture {
        lang: LANG::Typescript,
        containers: &["C", "I"],
        source: "\
interface I {
    q(): number;
}
class C implements I {
    public a: number = 1;
    static { }
    public q(): number { return this.a; }
}
function top(x: number): number { return x; }
",
    },
    #[cfg(feature = "typescript")]
    Fixture {
        lang: LANG::Tsx,
        containers: &["C", "I"],
        source: "\
interface I {
    q(): number;
}
class C implements I {
    public a: number = 1;
    static { }
    public q(): number { return this.a; }
}
function top(x: number): number { return x; }
",
    },
    #[cfg(feature = "csharp")]
    Fixture {
        lang: LANG::Csharp,
        containers: &["C", "I"],
        source: "\
interface I {
    int Q();
}
class C : I {
    public int A = 1;
    private int[] _v = new int[4];
    // An expression-bodied property and an accessor-less indexer are
    // `is_func_space` and `SpaceKind::Function` (#464, #472), so before
    // #1197 each carried an all-zero block beside `Q`, which had none.
    public int W => A;
    public int this[int i] => _v[i];
    public int Q() { return A; }
}
",
    },
    #[cfg(feature = "php")]
    Fixture {
        lang: LANG::Php,
        containers: &["C", "I"],
        source: "\
<?php
interface I {
    public function q();
}
class C implements I {
    public $a = 1;
    public function q() { return $this->a; }
}
function top($x) { return $x; }
",
    },
    #[cfg(feature = "ruby")]
    Fixture {
        lang: LANG::Ruby,
        // A Ruby `module` is `SpaceKind::Namespace`, which is a container.
        containers: &["M", "C"],
        source: "\
module M
  class C
    attr_accessor :a
    def q
      @a
    end
  end
end
",
    },
];

/// The container spaces named by each fixture carry both blocks.
///
/// The positive half of the contract, and the guard against "fixing"
/// #1197 by disabling the metric everywhere.
#[test]
fn containers_emit_npm_and_npa() {
    assert_fixtures_present(FIXTURES);
    for fixture in FIXTURES {
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        for want in fixture.containers {
            let space = only_space(fixture.lang, &spaces, want);
            assert!(
                matches!(
                    space.kind,
                    SpaceKind::Class | SpaceKind::Interface | SpaceKind::Namespace
                ),
                "{:?}: {want:?} should be a container kind, is {:?}",
                fixture.lang,
                space.kind
            );
            assert!(
                space.has_npm && space.has_npa,
                "{:?}: container {want:?} must emit npm and npa (npm={}, npa={})",
                fixture.lang,
                space.has_npm,
                space.has_npa
            );
        }
    }
}

/// No function space carries either block.
///
/// This is the assertion #1197 is about. The `<get>` / `<set>` /
/// `<init>` / `<static-init>` spaces #1184 introduced are ordinary
/// function spaces here and are covered by the same sweep, as are C#'s
/// expression-bodied property and indexer.
#[test]
fn function_spaces_emit_neither() {
    assert_fixtures_present(FIXTURES);
    for fixture in FIXTURES {
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        let functions: Vec<&Emitted> = spaces
            .iter()
            .filter(|s| s.kind == SpaceKind::Function)
            .collect();
        // A fixture whose functions all failed to open a space would make
        // every assertion below vacuous.
        assert!(
            !functions.is_empty(),
            "{:?}: expected at least one function space, got {:?}",
            fixture.lang,
            summary(&spaces)
        );
        for space in functions {
            assert!(
                !space.has_npm && !space.has_npa,
                "{:?}: {:?} space {:?} must not emit npm/npa (npm={}, npa={})",
                fixture.lang,
                space.kind,
                space.name,
                space.has_npm,
                space.has_npa
            );
        }
    }
}

/// The whole-file roll-up survives on the unit root, exactly as `wmc`'s
/// does.
///
/// Narrowing the enable to containers alone would have deleted this — an
/// information loss, not the all-zero-noise removal #1197 asked for, and
/// it would have left `npm` / `npa` disagreeing with `wmc` about a root
/// the three metrics share a [`MetricScope`](crate::metric_catalog::MetricScope).
#[test]
fn the_file_root_keeps_its_rollup() {
    assert_fixtures_present(FIXTURES);
    for fixture in FIXTURES {
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        let root = spaces.first().expect("the root space is always emitted");
        assert_eq!(root.kind, SpaceKind::Unit, "{:?}: root kind", fixture.lang);
        assert!(
            root.has_npm && root.has_npa,
            "{:?}: the unit root must keep its npm/npa roll-up (npm={}, npa={})",
            fixture.lang,
            root.has_npm,
            root.has_npa
        );
    }
}

/// Every #1184 construct opens a function space that emits neither
/// block, while a plain method beside it does the same.
///
/// [`function_spaces_and_file_roots_emit_neither`] would still pass if a
/// grammar stopped opening these spaces at all; naming them pins that
/// they exist *and* stay quiet.
#[test]
fn the_1184_constructs_open_quiet_function_spaces() {
    let cases: &[(LANG, &[&str])] = &[
        #[cfg(feature = "kotlin")]
        (LANG::Kotlin, &["<get>", "<set>", "<init>"]),
        #[cfg(feature = "java")]
        (LANG::Java, &["<static-init>"]),
        #[cfg(feature = "groovy")]
        (LANG::Groovy, &["<static-init>"]),
        #[cfg(feature = "javascript")]
        (LANG::Javascript, &["<static-init>"]),
        #[cfg(feature = "mozjs")]
        (LANG::Mozjs, &["<static-init>"]),
        #[cfg(feature = "typescript")]
        (LANG::Typescript, &["<static-init>"]),
        #[cfg(feature = "typescript")]
        (LANG::Tsx, &["<static-init>"]),
    ];
    assert_fixtures_present(cases);
    for (lang, names) in cases {
        let spaces = emitted_spaces(*lang, fixture_source(*lang));
        for name in *names {
            let space = only_space(*lang, &spaces, name);
            assert_eq!(space.kind, SpaceKind::Function, "{lang:?}: {name:?}");
            assert!(
                !space.has_npm && !space.has_npa,
                "{lang:?}: {name:?} must not emit npm/npa"
            );
        }
    }
}

/// Narrowing the enable predicate did not change what the containers
/// count.
///
/// The emission gate and the counters are independent — `merge` sums the
/// `_sum` fields regardless of `is_class_space` — but that independence
/// is worth pinning rather than assuming, since a wrong predicate could
/// have skipped a `ClassBody` walk instead of just a block.
#[test]
#[cfg(feature = "java")]
fn container_counts_survive_the_narrowed_enable() {
    let space = space_verbatim(
        LANG::Java,
        fixture_source(LANG::Java).as_bytes(),
        MetricsOptions::default(),
    );
    let class = crate::test_support::child_space(&space, "C");
    assert_eq!(class.metrics.npm.class_npm_sum(), 1, "public method `q`");
    assert_eq!(class.metrics.npa.class_npa_sum(), 1, "public attribute `a`");

    // The interface half of the same fixture, which a class-only
    // assertion would leave free to regress to zero.
    let interface = crate::test_support::child_space(&space, "I");
    assert_eq!(interface.metrics.npm.interface_npm_sum(), 1, "`I::q`");

    // The roll-up still reaches the root even though the root no longer
    // serializes it — the sum is what `bca check` reads at a container,
    // and dropping it would be a real regression rather than a shape one.
    assert_eq!(space.metrics.npm.class_npm_sum(), 1);
    assert_eq!(space.metrics.npa.class_npa_sum(), 1);
}
