# Stability and versioning

`big-code-analysis` is on the `2.x` line (currently `2.2.0`). The
full stability contract lives in [`STABILITY.md`][stability] at the
root of the repository — that file is the source of truth and is
updated alongside the changelog at every release.

[stability]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md

The headlines for library consumers:

- **Shape stability across patch and minor bumps.** Every public
  type and function signature listed in
  [STABILITY.md § "What is stable in shape"][stability-shape]
  is held across the `2.x` line. Additive changes (new items, new
  `LANG` variants, new `MetricsError` variants, new language
  features) are allowed in minor bumps. Breaking shape changes are
  reserved for the next major bump and will appear in the
  [changelog][changelog] under **(breaking)** in the `3.0.0`
  section.
- **No value stability guarantee within `2.x`.** A grammar pin
  bump or a bug fix in a metric definition can shift any metric
  value on any file in any direction, even across a patch bump.
  Each such drift is flagged in the changelog. Pin to an exact
  version (`big-code-analysis = "= 2.2.0"`) if you need bit-for-bit
  reproducibility across runs.
- **MSRV is `1.94`.** Bumping the MSRV is treated as a minor-bump
  event and is flagged in the changelog under **(breaking)** —
  see [STABILITY.md § MSRV policy][stability-msrv].
- **Escape hatches.** The [`Node`][Node] wrapper exposes
  `tree_sitter::Node` through `.0`, and the `tree_sitter` crate is
  re-exported as `big_code_analysis::tree_sitter`. Anything reached
  through those seams follows the pinned `tree-sitter` version, not
  our own [SemVer]. See [STABILITY.md § Escape hatches][stability-escape]
  before depending on them.

[stability-shape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#what-is-stable-in-shape
[stability-msrv]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#msrv-policy
[stability-escape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches
[changelog]: https://github.com/dekobon/big-code-analysis/blob/main/CHANGELOG.md
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html
[SemVer]: https://semver.org/

## On the `3.0` horizon

The breaking changes once staged for `2.0` have shipped in `2.0.0`:
the `#[non_exhaustive]` markers on the open public enums, the
serialized-key normalization, the integer-metric `u64` shift, the
language-dispatch and grammar defaults, the Python and REST surface
changes, and a consolidated metric-value re-baseline folding in the
drift accumulated since `1.0`. The path-positional callback dispatch
(`action` / the `Callback` trait), the free `metrics` /
`metrics_with_options` / `get_function_spaces` / `metrics_from_tree` /
`get_ops` functions, and the generic `Parser<T>` / `ParserTrait`
plumbing were removed at the same time — [`analyze`] and [`Ast`] are
now the single analysis seam, with `Parser` and the per-language
parser/tag types demoted to `pub(crate)`.

One loose end is deferred to the next major: the per-metric `Stats`
structs are not yet `#[non_exhaustive]`, so adding a field is a shape
break in the strict SemVer sense. In practice field additions are
treated as additive in minor bumps and flagged in the changelog;
marking the structs `#[non_exhaustive]` is on the
[`3.0` roadmap][stability-3x] so that carve-out can be retired.

No `3.0` is scheduled. The `#[non_exhaustive]` markers added at `2.0`
keep most future additions (new enum variants, new fields)
non-breaking, so `2.x` is the surface you should depend on.

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[`Ast`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html
[stability-3x]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#on-the-30-horizon
