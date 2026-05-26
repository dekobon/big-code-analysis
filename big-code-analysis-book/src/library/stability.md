# Stability and versioning

`big-code-analysis` is on the `1.x` line. The full stability
contract lives in [`STABILITY.md`][stability] at the root of the
repository — that file is the source of truth and is updated
alongside the changelog at every release.

[stability]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md

The headlines for library consumers:

- **Shape stability across patch and minor bumps.** Every public
  type and function signature listed in
  [STABILITY.md § "What is stable in shape"][stability-shape]
  is held across the `1.x` line. Additive changes (new items, new
  `LANG` variants, new `MetricsError` variants, new language
  features) are allowed in minor bumps. Breaking shape changes are
  reserved for the next major bump and will appear in the
  [changelog][changelog] under **(breaking)** in the `2.0.0`
  section.
- **No value stability guarantee within `1.x`.** A grammar pin
  bump or a bug fix in a metric definition can shift any metric
  value on any file in any direction, even across a patch bump.
  Each such drift is flagged in the changelog. Pin to an exact
  version (`big-code-analysis = "= 1.1.0"`) if you need bit-for-bit
  reproducibility across runs.
- **MSRV is `1.94`.** Bumping the MSRV is treated as a minor-bump
  event and is flagged in the changelog under **(breaking)** —
  see [STABILITY.md § MSRV policy][stability-msrv].
- **Escape hatches.** The [`Node`][Node] wrapper exposes
  `tree_sitter::Node` through `.0`, and the `tree_sitter` crate is
  re-exported as `big_code_analysis::tree_sitter`. Anything reached
  through those seams follows the pinned `tree-sitter` version, not
  our own SemVer. See [STABILITY.md § Escape hatches][stability-escape]
  before depending on them.

[stability-shape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#what-is-stable-in-shape
[stability-msrv]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#msrv-policy
[stability-escape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches
[changelog]: https://github.com/dekobon/big-code-analysis/blob/main/CHANGELOG.md
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html

## On the `2.0` horizon

A small number of loose ends are deferred to `2.0`; they are
listed in [STABILITY.md § "On the `2.0` horizon"][stability-2x].
The headline items are:

- The per-metric `Stats` structs gain `#[non_exhaustive]`, so
  field additions stop being a shape break in the strict SemVer
  sense.
- The deprecated `metrics` / `metrics_with_options` shims (in
  favour of `analyze`) are removed.
- The accumulated metric-definition fixes that have shifted values
  across `1.x` get a clean re-baseline note.

`2.0` is not scheduled. Until then, `1.x` is the surface you should
depend on.

[stability-2x]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#on-the-20-horizon
