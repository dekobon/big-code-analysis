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
//! The rule is [`SpaceKind::is_member_scope`], which `wmc` already used:
//! containers and the file unit carry the block, a function space never
//! does. Both directions are asserted below, because narrowing too far
//! would silently delete the whole-file roll-up rather than the all-zero
//! noise.
//!
//! #1197 left the rule a convention: it routed ten languages through a
//! shared predicate and let the other seven keep enabling from their own
//! node kinds. Those seven disagreed with it in both directions — a Go or
//! Rust `struct` declared inside a function put the block on that
//! *function* space, and a file whose only container sat inside a
//! function left the root without one, so the counts were serialized
//! nowhere. #1203 removed the choice: the space's own kind is now the
//! only input, recorded once in `FuncSpace::new`. Every language is
//! covered below for that reason — the point is no longer that ten obey
//! a predicate, but that the rule has no per-language surface left to
//! deviate on.

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

/// One fixture per language that has an `Npm` / `Npa` impl.
///
/// Each carries a container with one public method and one public
/// attribute, at least one ordinary method, and — where the grammar has
/// one — a #1184 construct (`get`/`set`/`init`/`static`), so a single
/// fixture exercises both halves of the defect.
struct Fixture {
    lang: LANG,
    /// Names of the container spaces that must carry both blocks.
    ///
    /// Empty for Go alone, whose `Getter` has no container `SpaceKind` —
    /// `type … struct` and `type … interface` open no space, so a Go
    /// file's `npm` / `npa` live on the unit root and nowhere else.
    /// [`containers_emit_npm_and_npa`] names Go rather than skipping an
    /// empty list quietly, since a list that silently became empty for
    /// any other language would make that test vacuous for it.
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
    // The seven below gated on their own node kinds until #1203. Each
    // fixture therefore declares a container *inside a function body* as
    // well as at file scope: that is the shape whose block used to land
    // on the function space, and — where the language had no other
    // container — the shape whose counts reached no serialized block at
    // all.
    #[cfg(feature = "rust")]
    Fixture {
        lang: LANG::Rust,
        containers: &["T", "S"],
        source: "\
pub struct S {
    pub a: u8,
    b: u8,
}

pub trait T {
    fn q(&self) -> u8;
}

impl S {
    pub fn m(&self) -> u8 { self.a }
}

fn top() -> u8 {
    struct Inner { pub x: u8 }
    Inner { x: 1 }.x
}
",
    },
    // Go is the one language with no container `SpaceKind` at all; see
    // `Fixture::containers`.
    #[cfg(feature = "go")]
    Fixture {
        lang: LANG::Go,
        containers: &[],
        source: "\
package main

type S struct {
\tPub  int
\tpriv int
}

type I interface {
\tSpeak() string
}

func (s S) Method() int { return s.Pub }

func Outer() int {
\ttype inner struct {
\t\tX int
\t}
\treturn inner{X: 1}.X
}
",
    },
    #[cfg(feature = "python")]
    Fixture {
        lang: LANG::Python,
        containers: &["C", "Inner"],
        source: "\
class C:
    a = 1

    def q(self):
        return self.a

def top(x):
    class Inner:
        b = 2
    return Inner
",
    },
    #[cfg(feature = "cpp")]
    Fixture {
        lang: LANG::Cpp,
        containers: &["N", "C"],
        source: "\
namespace N {
class C {
public:
    int a;
    int q() { return a; }
};
}

int top() { return 0; }
",
    },
    #[cfg(feature = "objc")]
    Fixture {
        lang: LANG::Objc,
        // Distinct names deliberately: an `@interface C` and its
        // `@implementation C` open two spaces with the *same* name, which
        // `only_space` rejects. A `@protocol` carries the interface half
        // instead.
        containers: &["P", "C"],
        source: "\
@protocol P
- (int)r;
@end

@implementation C {
    int a;
}
- (int)q { return a; }
@end
",
    },
    #[cfg(feature = "elixir")]
    Fixture {
        lang: LANG::Elixir,
        containers: &["Outer", "Inner", "Sibling"],
        source: "\
defmodule Outer do
  defstruct [:a]
  def q, do: 1
  defp r, do: 2

  defmodule Inner do
    def s, do: 3
  end
end

defmodule Sibling do
  def t, do: 4
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
        // An empty list means every following assertion is skipped, so
        // name the one language that is allowed to have one rather than
        // letting a fixture go quiet by accident.
        assert_eq!(
            fixture.containers.is_empty(),
            fixture.lang == LANG::Go,
            "{:?}: only Go has no container SpaceKind",
            fixture.lang
        );
        let spaces = emitted_spaces(fixture.lang, fixture.source);
        for want in fixture.containers {
            let space = only_space(fixture.lang, &spaces, want);
            assert!(
                matches!(
                    space.kind,
                    SpaceKind::Class
                        | SpaceKind::Interface
                        | SpaceKind::Namespace
                        | SpaceKind::Struct
                        | SpaceKind::Trait
                        | SpaceKind::Impl
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
/// [`function_spaces_emit_neither`] would still pass if a
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
/// `_sum` fields without consulting the space kind — but that
/// independence is worth pinning rather than assuming, since a wrong
/// predicate could have skipped a `ClassBody` walk instead of just a
/// block.
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

    // The roll-up reaches the root — the sum is what `bca check` reads at
    // a container, and dropping it would be a real regression rather than
    // a shape one.
    assert_eq!(space.metrics.npm.class_npm_sum(), 1);
    assert_eq!(space.metrics.npa.class_npa_sum(), 1);
}

/// A type declared *inside a function body* leaves that function quiet
/// and is reported by the file root instead (#1203).
///
/// This is the shape Go and Rust got wrong. Neither language opens a
/// space for a `struct`, so its counts landed on whichever space enclosed
/// it — a `Function` space for a type declared in a function body, which
/// then serialized a block that #1197 had already ruled out everywhere
/// else.
///
/// The roll-up half is not a formality. Go enables `npm` at the root only
/// for a file with a direct `MethodDeclaration` child, and Rust `npa`
/// only for a module-scope `struct`, so for a file whose only container
/// sits inside a function, merely *clearing* the function's block would
/// have serialized the counts nowhere at all — they survive in the root's
/// `_sum` fields either way, which is exactly why an absence-only test
/// could not tell the two outcomes apart. Both fixtures below therefore
/// assert the root's sums include the nested declaration's members.
#[test]
fn a_type_declared_inside_a_function_reaches_the_root_rollup() {
    // (language, the function holding the declaration, the root's
    // `class_na_sum` / `class_npa_sum` once the nested type is folded in)
    let cases: &[(LANG, &str, u64, u64)] = &[
        // `S{Pub, priv}` + `inner{X}` = 3 attributes, of which `Pub` and
        // `X` are exported by Go's leading-uppercase rule.
        #[cfg(feature = "go")]
        (LANG::Go, "Outer", 3, 2),
        // `S{a, b}` + `Inner{x}` = 3 fields, of which `pub a` and
        // `pub x` are public.
        #[cfg(feature = "rust")]
        (LANG::Rust, "top", 3, 2),
    ];
    assert_fixtures_present(cases);
    for (lang, holder, na_sum, npa_sum) in cases {
        let source = fixture_source(*lang);

        let spaces = emitted_spaces(*lang, source);
        let function = only_space(*lang, &spaces, holder);
        assert_eq!(function.kind, SpaceKind::Function, "{lang:?}: {holder:?}");
        assert!(
            !function.has_npm && !function.has_npa,
            "{lang:?}: {holder:?} holds a nested type but must not carry a block \
             (npm={}, npa={})",
            function.has_npm,
            function.has_npa
        );

        let root = spaces.first().expect("the root space is always emitted");
        assert!(
            root.has_npm && root.has_npa,
            "{lang:?}: the unit root must carry the roll-up (npm={}, npa={})",
            root.has_npm,
            root.has_npa
        );

        // The values behind that block, so a rule that emitted an
        // all-zero root would fail here rather than pass the key check
        // above.
        let space = space_verbatim(*lang, source.as_bytes(), MetricsOptions::default());
        assert_eq!(
            space.metrics.npa.class_na_sum(),
            *na_sum,
            "{lang:?}: root attributes, including the type declared in {holder:?}"
        );
        assert_eq!(
            space.metrics.npa.class_npa_sum(),
            *npa_sum,
            "{lang:?}: root public attributes, including the type declared in {holder:?}"
        );
    }
}

/// A C++ `namespace` carries both blocks.
///
/// Namespaces are the largest population the #1203 rule moved — 1,337 of
/// them in this repository's own integration corpora, none of which
/// serialized either block before, because C++ enabled from
/// `ClassSpecifier` / `StructSpecifier` and a namespace is neither.
/// `SpaceKind::Namespace` is asserted directly rather than through
/// [`containers_emit_npm_and_npa`]'s any-container-kind check, which
/// would still pass if the grammar started reporting `N` as a class.
#[test]
#[cfg(feature = "cpp")]
fn a_cpp_namespace_is_a_member_scope() {
    let spaces = emitted_spaces(LANG::Cpp, fixture_source(LANG::Cpp));
    let namespace = only_space(LANG::Cpp, &spaces, "N");
    assert_eq!(namespace.kind, SpaceKind::Namespace);
    assert!(
        namespace.has_npm && namespace.has_npa,
        "a namespace rolls its classes up and must carry both blocks \
         (npm={}, npa={})",
        namespace.has_npm,
        namespace.has_npa
    );
}

/// A language with no class-shaped construct emits no block at all.
///
/// The file unit is a member scope like any other, so making emission
/// depend on the space kind alone would have given a shell script a
/// `class_npa_sum: 0` block on every file — noise for a grammar that
/// cannot produce anything else. `Npm::HAS_MEMBERS` / `Npa::HAS_MEMBERS`
/// keep those languages out, the way `wmc`'s no-op `compute` does by
/// never recording a kind. Asserted on the root because it is the only
/// space these fixtures have that is a member scope.
#[test]
fn a_language_with_no_member_construct_emits_neither_block() {
    let cases: &[(LANG, &str)] = &[
        #[cfg(feature = "bash")]
        (LANG::Bash, "foo() { echo hi; }\nfoo\n"),
        #[cfg(feature = "lua")]
        (LANG::Lua, "function f(a) return a end\n"),
        #[cfg(feature = "c")]
        (LANG::C, "int add(int a, int b) { return a + b; }\n"),
    ];
    assert_fixtures_present(cases);
    for (lang, source) in cases {
        let spaces = emitted_spaces(*lang, source);
        let root = spaces.first().expect("the root space is always emitted");
        assert_eq!(root.kind, SpaceKind::Unit, "{lang:?}: root kind");
        assert!(
            !root.has_npm && !root.has_npa,
            "{lang:?}: a grammar with no member construct must emit neither \
             block (npm={}, npa={})",
            root.has_npm,
            root.has_npa
        );
    }
}

/// Each Elixir `defmodule` counts only its own members.
///
/// Until #1203 the Elixir impls opened with `if !stats.is_disabled() ||
/// …  { return; }` — a first-wins guard reusing the emission flag, which
/// went away with the flag. It was inert, because the walker pushes a
/// `defmodule`'s space before running any metric against it, so a nested
/// module never reaches its parent's stats. "Was inert" is a claim about
/// walk order rather than about this file, so it is pinned here: were the
/// guard load-bearing, `Outer` would absorb `Inner`'s members twice.
#[test]
#[cfg(feature = "elixir")]
fn each_elixir_module_counts_only_its_own_members() {
    use crate::test_support::child_space;

    let root = space_verbatim(
        LANG::Elixir,
        fixture_source(LANG::Elixir).as_bytes(),
        MetricsOptions::default(),
    );

    let outer = child_space(&root, "Outer");
    // `def q` + `defp r`, plus `Inner`'s `def s` through the roll-up;
    // `defp` is private, so two of the three are public.
    assert_eq!(outer.metrics.npm.class_nm_sum(), 3, "Outer: q, r, Inner::s");
    assert_eq!(outer.metrics.npm.class_npm_sum(), 2, "Outer: q, Inner::s");
    // `defstruct [:a]`, and Elixir struct fields are all public.
    assert_eq!(outer.metrics.npa.class_na_sum(), 1, "Outer: defstruct :a");

    let inner = child_space(outer, "Inner");
    assert_eq!(inner.metrics.npm.class_nm_sum(), 1, "Inner: s");
    assert_eq!(
        inner.metrics.npa.class_na_sum(),
        0,
        "Inner has no defstruct"
    );

    // A sibling module at file scope, which a guard that fired once per
    // *file* rather than once per space would have silenced.
    let sibling = child_space(&root, "Sibling");
    assert_eq!(sibling.metrics.npm.class_nm_sum(), 1, "Sibling: t");

    // Four methods across three modules, three of them public.
    assert_eq!(root.metrics.npm.class_nm_sum(), 4, "q, r, Inner::s, t");
    assert_eq!(root.metrics.npm.class_npm_sum(), 3, "q, Inner::s, t");
}
