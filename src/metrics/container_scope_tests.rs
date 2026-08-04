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
//! not "is a container". Ten languages therefore emitted an all-zero
//! block on the file root, and seven of them on every ordinary method
//! too; #1184 then added property accessors and `init` / `static` blocks
//! to the list, next to sibling methods that had none.

use serde_json::Value;

use crate::spaces::SpaceKind;
use crate::test_support::space_verbatim;
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
/// [`opens_container_space`](super::opens_container_space).
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

const FIXTURES: &[Fixture] = &[
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
    Fixture {
        lang: LANG::Csharp,
        containers: &["C", "I"],
        source: "\
interface I {
    int Q();
}
class C : I {
    public int A = 1;
    public int Q() { return A; }
}
",
    },
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
    for fixture in FIXTURES {
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        for want in fixture.containers {
            let found: Vec<&Emitted> = spaces
                .iter()
                .filter(|s| s.name.as_deref() == Some(*want) && s.kind != SpaceKind::Unit)
                .collect();
            assert_eq!(
                found.len(),
                1,
                "{:?}: expected exactly one container space named {want:?}, \
                 got {:?}",
                fixture.lang,
                spaces
                    .iter()
                    .map(|s| (s.kind, s.name.as_deref()))
                    .collect::<Vec<_>>()
            );
            let space = found[0];
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

/// No function space and no file root carries either block.
///
/// This is the assertion #1197 is about. The `<get>` / `<set>` /
/// `<init>` / `<static-init>` spaces #1184 introduced are ordinary
/// function spaces here and are covered by the same sweep.
#[test]
fn function_spaces_and_file_roots_emit_neither() {
    for fixture in FIXTURES {
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        let non_containers: Vec<&Emitted> = spaces
            .iter()
            .filter(|s| matches!(s.kind, SpaceKind::Unit | SpaceKind::Function))
            .collect();
        // A fixture whose functions all failed to open a space would make
        // every assertion below vacuous.
        assert!(
            non_containers.len() >= 2,
            "{:?}: expected the unit root plus at least one function space, \
             got {:?}",
            fixture.lang,
            spaces
                .iter()
                .map(|s| (s.kind, s.name.as_deref()))
                .collect::<Vec<_>>()
        );
        for space in non_containers {
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

/// Every #1184 construct opens a function space that emits neither
/// block, while a plain method beside it does the same.
///
/// [`function_spaces_and_file_roots_emit_neither`] would still pass if a
/// grammar stopped opening these spaces at all; naming them pins that
/// they exist *and* stay quiet.
#[test]
fn the_1184_constructs_open_quiet_function_spaces() {
    let cases: &[(LANG, &[&str])] = &[
        (LANG::Kotlin, &["<get>", "<set>", "<init>"]),
        (LANG::Java, &["<static-init>"]),
        (LANG::Groovy, &["<static-init>"]),
        (LANG::Javascript, &["<static-init>"]),
        (LANG::Mozjs, &["<static-init>"]),
        (LANG::Typescript, &["<static-init>"]),
        (LANG::Tsx, &["<static-init>"]),
    ];
    for (lang, names) in cases {
        let spaces = emitted_spaces(*lang, fixture_source(*lang));
        for name in *names {
            let found: Vec<&Emitted> = spaces
                .iter()
                .filter(|s| s.name.as_deref() == Some(*name))
                .collect();
            assert_eq!(
                found.len(),
                1,
                "{lang:?}: expected exactly one {name:?} space, got {:?}",
                spaces
                    .iter()
                    .map(|s| (s.kind, s.name.as_deref()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(found[0].kind, SpaceKind::Function, "{lang:?}: {name:?}");
            assert!(
                !found[0].has_npm && !found[0].has_npa,
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
fn container_counts_survive_the_narrowed_enable() {
    let space = space_verbatim(
        LANG::Java,
        fixture_source(LANG::Java).as_bytes(),
        MetricsOptions::default(),
    );
    let class = crate::test_support::child_space(&space, "C");
    assert_eq!(class.metrics.npm.class_npm_sum(), 1, "public method `q`");
    assert_eq!(class.metrics.npa.class_npa_sum(), 1, "public attribute `a`");

    // The roll-up still reaches the root even though the root no longer
    // serializes it — the sum is what `bca check` reads at a container,
    // and dropping it would be a real regression rather than a shape one.
    assert_eq!(space.metrics.npm.class_npm_sum(), 1);
    assert_eq!(space.metrics.npa.class_npa_sum(), 1);
}
