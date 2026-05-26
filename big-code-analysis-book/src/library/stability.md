# Stability and versioning

`big-code-analysis` is currently pre-`1.0`. The full stability
contract lives in [`STABILITY.md`][stability] at the root of the
repository — that file is the source of truth and is updated
alongside the changelog at every release.

[stability]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md

The headlines for library consumers:

- **Shape stability across patch bumps.** The
  `0.X.Y` → `0.X.Y+1` path holds the public type / function
  signatures listed in
  [STABILITY.md § "What is stable in shape"][stability-shape].
  Any shape break appears under a minor bump and is called out in
  the [changelog][changelog] under **(breaking)**.
- **No value stability before `1.0`.** A grammar pin bump or a
  bug fix in a metric definition can shift any metric value on
  any file in any direction, even across a patch bump. Pin to an
  exact version (`big-code-analysis = "= 1.1.0"`) if you need
  bit-for-bit reproducibility across runs.
- **MSRV is `1.94`.** Bumping the MSRV is treated as a minor-bump
  event and is flagged in the changelog under **(breaking)** —
  see [STABILITY.md § MSRV policy][stability-msrv].
- **Escape hatches.** The [`Node`][Node] wrapper exposes
  `tree_sitter::Node` through `.0`. Anything reached through that
  field follows the pinned `tree-sitter` version, not our own
  SemVer. See [STABILITY.md § Escape hatches][stability-escape]
  before depending on it.

[stability-shape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#what-is-stable-in-shape
[stability-msrv]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#msrv-policy
[stability-escape]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches
[changelog]: https://github.com/dekobon/big-code-analysis/blob/main/CHANGELOG.md
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html

## Planned changes that will affect this section

Several entries in this section reference issues that, when they
land, will rename or reshape part of the public API. They will
all appear under a minor bump and in the changelog under
**(breaking)**.

- [#251] — a first-class parse seam that re-exports `tree_sitter`,
  unblocking [Reusing an existing tree-sitter Tree](reuse-tree.md).
- [#253] — *landed*: every public entry point now returns
  `Result<FuncSpace, MetricsError>` (and `Result<Ops, MetricsError>` /
  `Result<Vec<Node>, MetricsError>` for the sibling APIs).
  See [Error handling](error-handling.md) for the variant set.
- [#254] — a `Source` newtype around `(path, bytes)`.
- [#255] — a curated prelude and tighter `pub use` set.
- [#256] — *landed*: `ParserTrait`, the per-metric compute traits,
  `Parser<T>`, `Filter`, and the deprecated generic shims are now
  `#[doc(hidden)]` so they no longer appear in the curated rustdoc
  surface. `Callback` / `action::<T>` remain documented and are
  re-evaluated separately.
- [#257] — per-metric Cargo features, picked up by
  [Selecting metrics](selecting-metrics.md).
- [#252] — per-language Cargo features.

The tracker is [#250]; subscribe there to follow the rollup.

[#250]: https://github.com/dekobon/big-code-analysis/issues/250
[#251]: https://github.com/dekobon/big-code-analysis/issues/251
[#252]: https://github.com/dekobon/big-code-analysis/issues/252
[#253]: https://github.com/dekobon/big-code-analysis/issues/253
[#254]: https://github.com/dekobon/big-code-analysis/issues/254
[#255]: https://github.com/dekobon/big-code-analysis/issues/255
[#256]: https://github.com/dekobon/big-code-analysis/issues/256
[#257]: https://github.com/dekobon/big-code-analysis/issues/257
