# Cutting a release

This document is the step-by-step procedure for releasing
`big-code-analysis`. It describes what to do, in what order, and what
to check when something looks wrong.

The pipeline is defined in `.github/workflows/release.yml`.
Everything downstream of `git push --tags` is automated.

## MSRV (Minimum Supported Rust Version)

The workspace pins MSRV at **Rust 1.94** via
`[workspace.package] rust-version = "1.94"`. Every member crate
inherits this with `rust-version.workspace = true`.

Rationale:

- Edition 2024 is the active edition for every crate; `let-else`,
  let-chains, and the relaxed lifetime-elision rules used across
  `src/languages/` require Rust 1.85+, but several individual
  improvements rely on later releases (e.g. const slice indexing
  stabilizations, refined drop-order semantics).
- Treating 1.94 as the floor avoids "works on my machine" reports
  where a contributor on a slightly older toolchain hits an
  edition-2024 surprise that the CI image silently papers over.
- 1.94 is the toolchain the `msrv` job in `.github/workflows/ci.yml`
  exercises (the rest of the CI matrix uses `stable`). Lowering MSRV
  without updating that job is meaningless; raising MSRV without
  updating it is a foot-gun. (A repo-root `rust-toolchain.toml` pin is
  on the roadmap but not yet committed; once it lands, treat it as the
  third point of truth that must move in lockstep.)

Bumping MSRV is a deliberate workspace-wide change: update
`[workspace.package] rust-version`, the CI matrix, and any clippy
`msrv` directives in lockstep (plus `rust-toolchain.toml` once it
lands). Note the bump in the CHANGELOG under `### Changed`.

## What the release pipeline will do

One push of a `v*` tag will run this end-to-end:

1. **preflight**: validates the tag, checks `Cargo.toml` version
   parity against `[workspace.package] version`, confirms
   `minisign.pub` is not a placeholder, and extracts the matching
   `CHANGELOG.md` section as release notes.
2. **build**: cross-compiles `bca` and `bca-web` for the target
   matrix: Linux gnu/musl × x86_64/aarch64, macOS aarch64, Windows
   x86_64/aarch64. `x86_64-unknown-freebsd` is tracked separately
   (see [#346](https://github.com/dekobon/big-code-analysis/issues/346)
   under [Known pipeline limitations](#known-pipeline-limitations)). Strips
   binaries, captures debug symbols, and produces per-target
   `.tar.gz` / `.zip` archives.
3. **package-***: builds `.deb`, `.rpm`, `.apk`, and any other OS
   packages from the staged binaries.
4. **smoke-***: installs each package inside the appropriate
   container/VM and asserts `bca --version` and `bca-web --version`
   match the tag.
5. **sign-attest**: flattens every artefact into `release/`,
   generates CycloneDX SBOMs, computes `SHA256SUMS`, signs it with
   minisign, and attaches SLSA build provenance.
6. **publish**: creates/updates the GitHub Release, attaches every
   artefact + `SHA256SUMS` + `SHA256SUMS.minisig`, and (for non
   pre-releases, **subject to the gating variables below**) pushes the
   Homebrew formula and Scoop manifest.
7. **publish-crates**: for non pre-releases, **subject to the gating
   variables below**, runs `cargo publish` for each publishable
   workspace crate in dependency order: the five `bca-tree-sitter-*`
   grammar leaves first, then `big-code-analysis` (library), then
   `big-code-analysis-cli` and `big-code-analysis-web`. Skips
   idempotently if the version is already on crates.io.
8. **verify**: downloads the published musl tarball back out of the
   release, verifies the minisign signature, checksum, and SLSA
   provenance.

If any stage fails, nothing downstream runs. `publish` and
`publish-crates` are the only jobs that mutate anything outside this
repo; they run in parallel so a crates.io failure does not block the
GitHub Release's `verify` step (and vice versa).

The **same `v*` tag push also triggers two independent PyPI wheel
workflows** that are *not* part of `release.yml` and run fully in
parallel with it (a failure in one does not block the others):

- **`python-wheels.yml`** publishes the importable library bindings
  (`big-code-analysis`), an abi3 extension wheel. See
  [Python wheels (PyPI)](#python-wheels-pypi).
- **`python-cli-wheels.yml`** publishes the `bca` command-line tool
  (`big-code-analysis-cli`), a `-b bin` wheel that drops `bca` onto
  `PATH`. See [CLI wheels (PyPI)](#cli-wheels-pypi).

Both read the workspace version (`dynamic = ["version"]`), so they
publish in lockstep with the crates above on every bump (no separate
version step). Their one-time Trusted-Publisher setup is in the
[Post-public-release checklist](#post-public-release-checklist); after
that they fire automatically on each tag.

## Defer-and-gate state for public publication

The repository is staging for a future public release. Until the
maintainer flips the dials, the workflow **must not** push to
crates.io, Homebrew, or Scoop, even on a stable tag. This is enforced
by three repo-level GitHub Actions variables (Settings → Secrets and
variables → Actions → Variables), each defaulting to unset:

| Variable                | Gates                                            |
| ----------------------- | ------------------------------------------------ |
| `ENABLE_CRATES_PUBLISH` | The `publish-crates` job.                        |
| `ENABLE_HOMEBREW_TAP`   | The Homebrew formula push inside `publish`.      |
| `ENABLE_SCOOP_BUCKET`   | The Scoop manifest push inside `publish`.        |

Each variable is skipped when unset or set to anything other than the
literal string `true`. Each gated step uses an `if:` guard of the
shape:

```yaml
if: vars.ENABLE_CRATES_PUBLISH == 'true'
    && needs.preflight.outputs.prerelease != 'true'
```

So:

- Pre-release tags (`-rc1`, `-beta2`, `-alpha3`) never publish
  externally, regardless of the variable.
- A stable tag with the variable unset still produces signed
  artefacts on the GitHub Release; it just does not push to crates.io
  or downstream package managers.

To turn on publication for the public-release cutover, set the
relevant variable(s) to the literal string `true`. Leave them unset
to keep the dry-run posture.

## Vendored tree-sitter grammar publishability

The workspace vendors five tree-sitter grammar crates under path
dependencies. As of issue
[#149](https://github.com/dekobon/big-code-analysis/issues/149), they
publish to crates.io under project-namespaced names so they don't
collide with the Mozilla-published originals (which sit at older
versions and a different owner):

| Path-dep directory     | Published crate name        | Rust import path        |
| ---------------------- | --------------------------- | ----------------------- |
| `tree-sitter-ccomment` | `bca-tree-sitter-ccomment`  | `tree_sitter_ccomment`  |
| `tree-sitter-mozcpp`   | `bca-tree-sitter-mozcpp`    | `tree_sitter_mozcpp`    |
| `tree-sitter-mozjs`    | `bca-tree-sitter-mozjs`     | `tree_sitter_mozjs`     |
| `tree-sitter-preproc`  | `bca-tree-sitter-preproc`   | `tree_sitter_preproc`   |
| `tree-sitter-tcl`      | `bca-tree-sitter-tcl`       | `tree_sitter_tcl`       |

Each leaf manifest sets `[lib] name = "tree_sitter_<lang>"` so the
*produced* Rust crate keeps its original import path even though the
*published* package name is `bca-tree-sitter-<lang>`. The workspace
alias in the root `Cargo.toml` (and `enums/Cargo.toml`) uses Cargo's
`package = ...` aliasing so every consumer site reads
`tree-sitter-<lang> = { workspace = true }` as before; call sites
under `src/`, `enums/`, and feature flags did not change.

**Publish order is leaf-first.** The `publish-crates` job in
`release.yml` publishes the five `bca-tree-sitter-*` crates ahead of
`big-code-analysis`, because the parent's `=<leaf-version>` pin can
only resolve once each leaf is on crates.io. The sparse-index
existence check in each step makes the job idempotent across re-runs
of the same tag.

**The three top-level crates cannot be dry-run before the tag.**
`big-code-analysis` pins each leaf at `bca-tree-sitter-<lang> =
"=<version>"`, and `big-code-analysis-cli` / `big-code-analysis-web`
pin `big-code-analysis` the same way. Packaging resolves those
requirements against the registry, and the Lockstep version policy
below makes the pinned version the one this tag is releasing — which
is, by definition, not yet published. So it fails, every time, not
only on a first release:

```console
$ cargo package -p big-code-analysis --allow-dirty --no-verify
error: failed to select a version for the requirement
  `bca-tree-sitter-ccomment = "=2.1.1"`
candidate versions found which didn't match: 2.1.0, 2.0.0, 1.1.0, ...
```

Until [#1224](https://github.com/dekobon/big-code-analysis/issues/1224)
both `release.yml` and `make release-check` wrapped the parent dry-run
in a sparse-index probe for that same version and described the skip
as a first-release bootstrap. Because the probed version is always the
version being released, the probe never hit and the dry-run never ran
ahead of an upload. This page said the opposite.

What runs in its place is `make check-publish-metadata`
(`utils/check-publish-metadata.py`), which needs nothing on the
registry. For every publishable crate it asserts that `description`,
`readme`, `repository`, and `license` (or `license-file`) are present
and non-empty; that a crate rooted at the workspace root carries a
non-empty `[package].include`; and that the files `cargo package
--list` reports — the one packaging step that does not resolve
dependencies — total under 32 MiB. Dropping the `include` block
currently yields 445 MiB across 33,398 files, so the bound is not a
close call. It does not compile anything, so a feature-resolution or
build error in the packaged crate is still only caught by the real
`cargo publish`.

Running `--list` also validates `readme` and `license-file` for free:
cargo fails outright on a path that does not exist, and packages one
that `include` does not cover rather than dropping it. That is why the
gate does not check either itself.

It runs in `make lint`, `make pre-commit`, `make ci`, `make
release-check`, and the `preflight` job of `release.yml`. The five
vendored leaves are not in its scope: they carry no internal pins, so
`release-check` and `preflight` dry-run them for real.

**The leaves still publish first.** That ordering is what lets the
parent resolve during `publish-crates`, and it is unchanged. It also
means a metadata regression in the parent or the binaries would fail
*after* the five leaves are irrevocably on crates.io — the split
release the gate above exists to make unreachable.

**Lockstep version policy.** Every crate in this repository (the
library, the CLI, the web crate, the Python crate, the `enums` /
`xtask` helpers, and the five `bca-tree-sitter-*` vendored grammar
leaves) shares one version number. There is no per-crate version
drift. A version bump touches:

1. `[workspace.package] version` in the root `Cargo.toml`; this
   covers every workspace member that declares
   `version.workspace = true`.
2. `[package] version` in `enums/Cargo.toml` (excluded from the
   workspace; cannot inherit).
3. `[package] version` in each of the five
   `tree-sitter-<lang>/Cargo.toml` files (also excluded).
4. The `version = "=<new>"` pin on every `bca-tree-sitter-*` entry
   in `[workspace.dependencies]` (root `Cargo.toml`) and the
   matching block in `enums/Cargo.toml`.
5. The `version = "=<new>"` pin on the `big-code-analysis` path-dep
   in `big-code-analysis-cli/Cargo.toml` and
   `big-code-analysis-web/Cargo.toml`.
6. Only when the bump is the release-prep commit for the version
   being cut: the hard-coded version references in user-facing docs
   (`README.md`, `STABILITY.md`, the book's `quick-start.md` and
   `cargo-features.md`) **and** the install snippet in every
   leaf's `bindings/rust/README.md` (5 files), since those ship
   inside the published `bca-tree-sitter-*` tarballs and render as
   the crates.io landing page. A post-tag bump to the next
   development version leaves them at the published release; see
   [Version strings in documentation](#version-strings-in-documentation).
7. The man pages (re-run `cargo run -p xtask`).
8. The SARIF tool-version snapshots (re-run `cargo insta test` and
   accept).
9. A new `## [<new>]` section in `CHANGELOG.md` (the unreleased
   block is collapsed into it at release time).

Run `./utils/check-versions.py` (also wired into `make pre-commit` and
the `lint` job in `.github/workflows/ci.yml`) after editing to
catch any item the human eye missed.

A grammar refresh (`recreate-grammars.sh` regenerates the parsers)
is a normal change *under* the current version: bumping the
grammars does not bump the version on its own. The next workspace
release picks up the regenerated grammars at whatever leaf version
already matches the workspace version.

## Prerequisites (one-time setup)

You only need to do this once per project, but verify each item
before the first real release.

### Local tooling

`make release-check` shells out to two cargo subcommands that the
day-to-day gate never touches, plus `minisign` if you verify a
published release locally:

```bash
cargo install --locked cargo-deny
cargo install --locked cargo-about --features cli
```

`--features cli` is not optional. cargo-about moved its binary behind
a non-default `cli` feature in 0.9.0, so `cargo install cargo-about`
compiles the library, installs no binary, reports the miss as a
*warning*, and exits 0 — after which `cargo about` still answers `no
such command: about`, which reads like a PATH problem rather than an
install that did nothing.

`make check-tools` lists both alongside the rest of the toolchain, and
`make release-check` probes them up front and names whichever is
missing, so neither failure mode has to be diagnosed from cargo's
generic error.

### Repository secrets

Configure these under **Settings → Secrets and variables → Actions →
Secrets**:

| Secret                   | Purpose                                          |
| ------------------------ | ------------------------------------------------ |
| `MINISIGN_SECRET_KEY`    | minisign secret key, signs `SHA256SUMS`.         |
| `MINISIGN_PASSWORD`      | Password for the minisign key.                   |
| `ALPINE_ABUILD_KEY_PRIV` | abuild RSA private key (Alpine `.apk` signing).  |
| `ALPINE_ABUILD_KEY_PUB`  | Matching abuild public key.                      |
| `HOMEBREW_TAP_TOKEN`     | Fine-grained PAT for the Homebrew tap repo.      |
| `SCOOP_BUCKET_TOKEN`     | Fine-grained PAT for the Scoop bucket repo.      |

The two PATs need write access to
`dekobon/homebrew-tap` (shared tap; the workflow only touches
`Formula/big-code-analysis.rb`) and `dekobon/scoop-bucket` (shared
bucket; the workflow only touches `bucket/big-code-analysis.json`)
respectively. Both are minted at
<https://github.com/settings/personal-access-tokens/new> as
fine-grained PATs with **Repository access: Only select repositories**
(scoped to the single tap or bucket repo) and **Repository permissions
→ Contents: Read and write**; leave every other permission at *No
access*. Store each token under Settings → Secrets and variables →
Actions → Secrets on `dekobon/big-code-analysis`.

crates.io authentication uses
[Trusted Publishing](https://crates.io/docs/trusted-publishing): no
long-lived `CARGO_REGISTRY_TOKEN` is stored as a secret. The
`publish-crates` job mints a GitHub OIDC ID token and exchanges it for
a short-lived registry token scoped to that run.

If `HOMEBREW_TAP_TOKEN` or `SCOOP_BUCKET_TOKEN` is missing, or if the
target tap/bucket repo is unreachable (deleted, renamed, or the PAT
cannot see it), the corresponding step emits a GitHub Actions
warning and skips without failing the release.

### Minisign key

`minisign.pub` at the repo root must be a real public key, not a
committed placeholder. The preflight job greps for the placeholder
comment and aborts if it is still present.

To create a fresh key:

```bash
minisign -G -p minisign.pub -s minisign.key
```

Commit `minisign.pub`. Store `minisign.key` as the
`MINISIGN_SECRET_KEY` repo secret via stdin redirection; **do not
paste the contents into the web UI**:

```bash
gh secret set MINISIGN_SECRET_KEY -R dekobon/big-code-analysis < minisign.key
# The second command opens an interactive prompt on stdin; type the
# password, press Enter, then Ctrl-D to signal EOF.
gh secret set MINISIGN_PASSWORD -R dekobon/big-code-analysis
```

A minisign secret key file is two lines and ends with `\n`. Paste-via-
UI silently strips the trailing newline (and can introduce other
whitespace artefacts) so that `minisign -S` later fails with `Error
while loading the secret key file`, masquerading as a wrong-key /
wrong-password failure when the bytes are actually one newline short.
Stdin redirection from the file preserves the exact file bytes,
including the trailing newline that the web UI eats. Keep
`minisign.key` itself out of the repo.

### External repos

Stable releases push to (subject to the gating variables above):

- `dekobon/homebrew-tap`: shared Homebrew tap; the release workflow
  commits only `Formula/big-code-analysis.rb` and leaves the other
  formulae in the tap untouched.
- `dekobon/scoop-bucket`: shared Scoop bucket; the release workflow
  commits only `bucket/big-code-analysis.json` and leaves the other
  manifests in the bucket untouched.
- crates.io, leaf-first: the five `bca-tree-sitter-*` grammar
  crates, then `big-code-analysis` (library), then
  `big-code-analysis-cli` and `big-code-analysis-web`. See
  [crates.io ownership](#cratesio-ownership) for the publish loop
  and rate-limit details.

Both tap and bucket repos must exist and accept the configured PAT.

### crates.io ownership

Before the first automated publish you must manually claim **all eight
crate names**: the five `bca-tree-sitter-*` leaves plus the three
top-level crates. The `publish-crates` job in `release.yml` uses
Trusted Publishing which requires the crate to exist before TP can be
registered, so the very first publish has to be a hand-rolled
`cargo publish` from your workstation.

1. **Check name availability.** Open each of the following on
   `https://crates.io/crates/<name>`:

   - `bca-tree-sitter-ccomment`, `…-mozcpp`, `…-mozjs`, `…-preproc`,
     `…-tcl`
   - `big-code-analysis`
   - `big-code-analysis-cli`
   - `big-code-analysis-web`

   If any name is taken by someone else, pick a different name and
   update the matching `[package].name` (and the workspace alias for
   leaves) before tagging; `cargo owner --add` only works on crates
   you already own.

2. **Verify the parent's `include` whitelist is present.** The
   `[package].include = […]` block in the root `Cargo.toml`
   restricts the published `.crate` to `src/**`, `Cargo.toml`,
   `README.md`, `LICENSE`, and `CHANGELOG.md`. Without it,
   `cargo publish` packages the entire repo, notably
   `tests/repositories/` (~130 MiB compressed of snapshot
   fixtures), and the upload fails against crates.io's size
   limit with a Varnish `503 backend write error` rather than a
   useful error message. Verify before the first publish:

   ```bash
   make check-publish-metadata
   ```

   Do **not** reach for `cargo package --no-verify` here: it resolves
   the `=<version>` leaf pins against the registry and fails before it
   measures anything (see
   [Vendored tree-sitter grammar publishability](#vendored-tree-sitter-grammar-publishability)).
   The gate above uses `cargo package --list`, which does not. To see
   the file list yourself:

   ```bash
   cargo package -p big-code-analysis --allow-dirty --list | wc -l
   # ~1.3k entries, all under src/ plus Cargo.toml, README.md,
   # LICENSE, CHANGELOG.md. A count in the tens of thousands means
   # `include` has regressed and tests/repositories/ is being packaged.
   ```

3. **Publish leaf-first, with rate-limit pacing.** crates.io
   rate-limits **new** crates at roughly one per ten minutes after
   a short burst. Publishing all eight in a single pass will trip
   the limit; the second-half publishes return `429 Too Many
   Requests` with an explicit `try again after <timestamp>` hint.
   The simplest workaround is to retry on a loop:

   ```bash
   cargo login <your-token>

   # Leaf-first — the parent's `=<leaf-version>` pin cannot resolve
   # until each leaf is on the sparse index. cargo publish waits
   # for the index to catch up, so the next publish can resolve the
   # previous one without an explicit sleep.
   for d in tree-sitter-{ccomment,mozcpp,mozjs,preproc,tcl}; do
     until cargo publish --locked --manifest-path "$d/Cargo.toml"; do sleep 60; done
   done

   # Parent + binaries. These will hit the new-crate rate limit on
   # the first try; the until-loop retries every 60s until cargo
   # exits 0.
   until cargo publish -p big-code-analysis --locked;     do sleep 60; done
   until cargo publish -p big-code-analysis-cli --locked; do sleep 60; done
   until cargo publish -p big-code-analysis-web --locked; do sleep 60; done
   ```

   After all eight crates are on the registry, the `publish-crates`
   job's idempotency check makes it a no-op for any tag at the same
   version.

4. **Add additional owners.** `cargo owner --add <github-handle>
   <crate>` for each of the eight crates. A single-owner crate is
   one forgotten password away from being orphaned. If you have a
   GitHub team, use `github:<org>:<team>`.

5. **Register a Trusted Publisher for each crate** (see below).
   This replaces any long-lived API token a future contributor
   might otherwise wire into the workflow.

### crates.io Trusted Publisher setup

Trusted Publishing lets the release workflow authenticate to crates.io
via a short-lived OIDC token instead of a static API token. Two
one-time setup steps are required on top of the
[crates.io ownership](#cratesio-ownership) checklist above:

1. **Create a `release` GitHub Environment.** Go to **Settings →
   Environments → New environment** and name it exactly `release`.
   The `publish-crates` job references this environment and the
   crates.io trusted publisher matches the `environment` OIDC claim
   against it. Optional protection rules (required reviewers,
   deployment branch filters) act as a manual gate on every publish;
   the environment is the right place to add them, not the workflow.
   The name must match the TP registration exactly; a typo here is
   the most common self-inflicted failure mode.

2. **Register a Trusted Publisher for each of the eight crates.**
   On crates.io, open the settings page for each of the five
   `bca-tree-sitter-*` leaves, `big-code-analysis`,
   `big-code-analysis-cli`, and `big-code-analysis-web`. In the
   **Trusted Publishing** section, add a GitHub publisher with:

   - Repository owner: `dekobon`.
   - Repository name: `big-code-analysis`.
   - Workflow filename: `release.yml` (basename only, not a path).
   - Environment: `release`.

   Every publishable crate needs its own trusted-publisher entry: a
   TP registered on `big-code-analysis` does not cover the CLI, the
   web crate, or any of the leaves. The workflow still performs a
   single `auth` exchange for all publishes because crates.io
   issues one token covering every crate whose TP config matches
   the JWT claims.

3. **First stable release after cutover validates the path.** The
   prerelease gate (`if: needs.preflight.outputs.prerelease != 'true'`)
   skips `publish-crates` for `-rc` tags, so TP cannot be rehearsed
   via `workflow_dispatch`. The first non-prerelease tag after the
   cutover, with `ENABLE_CRATES_PUBLISH=true`, is the real
   end-to-end test. Watch the `auth` step logs.

## Bumping the version

The release pipeline is strict about version parity: the preflight job
rejects the tag if it does not match the workspace version, and the
smoke jobs reject the build if `bca --version` does not contain the
tag string. Bump the version deliberately, in one commit, before
tagging.

Member crates inherit their version from `[workspace.package]`, so
edit these in lockstep:

1. Root `Cargo.toml`, `[workspace.package] version = "x.y.z"`: the
   canonical version that every member crate picks up via
   `version.workspace = true`.
2. Any `[workspace.dependencies]` entries that pin an internal crate
   (e.g. `big-code-analysis = { path = "...", version = "x.y.z", ...
   }`). Must match the workspace version, otherwise `cargo publish`
   on the dependent crate will reject the dependency.
3. The `enums/` helper crate (excluded from the root workspace).
   Its own `[package] version` carries the same value; bump it
   alongside the workspace bump, never on its own.
4. Each `tree-sitter-<lang>/Cargo.toml` (also excluded). Same
   discipline as `enums/`: bump in lockstep with the workspace.

After editing, regenerate the lockfile and sanity-check the bump:

```bash
cargo update --workspace
cargo metadata --format-version 1 --no-deps \
  | python3 -c "import json,sys; d=json.load(sys.stdin); \
      print({p['name']: p['version'] for p in d['packages']})"
# Expect big-code-analysis, big-code-analysis-cli, and
# big-code-analysis-web at the target version.
```

The `cargo update --workspace` step is **mandatory**, not
nice-to-have: `publish-crates` runs `cargo publish --locked`, which
fails late in the release pipeline if `Cargo.lock` drifts from what
the workspace resolves to. Commit the refreshed lockfile alongside
the `Cargo.toml` edits.

**Refresh the workspace-excluded lockfiles too.** `cargo update
--workspace` reaches only the root lockfile, and an excluded crate has
its own. `fuzz/Cargo.lock` records `big-code-analysis` at the version it
resolved against, so the bump above strands it and every `--locked`
invocation in `make fuzz-check` then fails with "cannot update the lock
file … because --locked was passed" — verified by bumping
`[workspace.package] version` and re-running it. `enums/Cargo.lock` is
the same shape once its own `[package] version` and grammar pins move
in steps 3 and 4:

```bash
cargo update --manifest-path fuzz/Cargo.toml --workspace
cargo update --manifest-path enums/Cargo.toml --workspace
for leaf in ccomment mozcpp mozjs preproc tcl; do
  cargo update --manifest-path "tree-sitter-${leaf}/Cargo.toml" --workspace
done
```

Commit all of them alongside the root lockfile. `fuzz` and `enums` are
not published, so a stale lockfile there surfaces later, as a red fuzz
or `enums-check` job on a tree that looks correct. The five grammar
leaves *are* published, and each carries its own `Cargo.lock` recording
the leaf at the old version: `make release-check` dry-runs them with
`cargo publish --locked`, which refuses to touch the stale lockfile and
fails the gate (the v2.2.0 cut hit exactly this).

Regenerate the committed man pages in the same release-prep commit:

```bash
cargo xtask
```

`man/*.1` embeds both the binary version (`big-code-analysis x.y.z`
in the `.TH` line and `vX.Y.Z` in `.SH VERSION`) and the live clap
schema, so any version bump, workspace-wide or CLI-only (e.g. the
`big-code-analysis-cli` version override at #235), leaves the
committed pages stale. The per-PR `man pages up to date` CI job
gates against drift; `release.yml` regenerates the pages again per
build leg so the shipped artefacts cannot ship with a stale schema,
but committing the regenerated pages keeps the gate green between
release-prep and tag push. Same rule applies any time a CLI flag is
added or renamed, not just at release time.

Pick the version using semver. The workspace is on the `2.x` line
and ships under the [STABILITY.md][stability] contract: the public
Rust API surface (`big-code-analysis` library re-exports, the `bca`
CLI argument grammar, and the `bca-web` REST schema) is held stable
across patch and minor bumps. Additive shape changes (new items,
new `LANG` / `MetricsError` variants, new language features) belong
under a minor bump. Breaking shape changes are reserved for the
next major bump and must be called out under **(breaking)** in the
CHANGELOG entry for that release; do not slip a SemVer break into a
patch or minor bump. Metric value drift from a grammar pin move or
a metric-definition fix remains allowed under patch and minor bumps
and must be listed in the entry that introduces it.

[stability]: ./STABILITY.md

Commit the version bump together with the changelog move (see below)
so the release-prep commit is a single, self-contained change:

```text
chore(release): prepare v1.2.0
```

## Version strings in documentation

The docs carry two kinds of version string, and they move at
different times. Reader-facing pins must always reference the
**latest published release** (what a reader can download today),
while the workspace manifests track the **next** version being
developed. Between a tag and the next release the two deliberately
diverge: after the post-tag bump, `Cargo.toml` reads ahead of every
documentation example, and that is correct. When in doubt, apply the
reader test: if someone copies this line today, does it resolve?

`./utils/check-versions.py` (`make check-versions`, wired into
`make pre-commit` and the CI lint job) enforces the two automatable
categories: dependency-example pins must equal the latest released
`## [X.Y.Z]` section of `CHANGELOG.md`, and the `recipes/ci.md`
install pins may cite that release or the one immediately before it
(they move in the post-publish follow-up). Because release-prep
creates the new changelog section, the gate forces the release-prep
doc bump and rejects a premature one in the same stroke.

### Bump in the release-prep commit (before tagging vX.Y.Z)

These strings must read `X.Y.Z` in the tagged commit. They reference
crates.io / PyPI versions, which resolve as soon as `publish-crates`
and the wheel workflows finish, so setting them at release-prep is
safe:

| File | String |
| --- | --- |
| `STABILITY.md` | `(currently \`X.Y.Z\`)` in the opening paragraph |
| `STABILITY.md` | both exact-pin examples `big-code-analysis = "= X.Y.Z"` |
| `big-code-analysis-book/src/library/stability.md` | the same two shapes as `STABILITY.md` |
| `big-code-analysis-book/src/library/quick-start.md` | the `[dependencies]` example |
| `big-code-analysis-book/src/library/cargo-features.md` | both `[dependencies]` examples |
| `CHANGELOG.md` | `## [Unreleased]` entries move into `## [X.Y.Z]` (see the checklist) |

Optionally refresh the sample REST responses in
`big-code-analysis-book/src/commands/rest.md` (`"server"`,
`"library"`, `"version"` fields) so captured output matches the
release; they are illustrative, not pins, so this is non-blocking.

`man/*.1` also embeds the version but is generated: run `cargo xtask`
(already part of the release-prep steps above); never hand-edit.

Find stragglers with a scoped sweep before tagging. Never run a
blind repo-wide `sed`: the SemVer / Keep-a-Changelog spec URLs,
historical changelog sections, and illustrative examples match
version-shaped patterns and must not be rewritten. Review each hit
against the categories on this page:

```bash
PREV=2.0.0   # the previous published release
rg -n "$PREV" README.md STABILITY.md docs/ big-code-analysis-py/README.md \
  big-code-analysis-book/src/ --glob '!**/CHANGELOG.md'
```

### Update only after the release is actually published

Every pin in `big-code-analysis-book/src/recipes/ci.md` references a
published GitHub Release or crates.io artifact. They all move in a
single follow-up commit to `main` once the pipeline finishes:

- `BCA_VERSION` (the GitHub Actions `env:` block and the GitLab
  `variables:` block) and their paired `BCA_SHA256` values. The
  sha256 is the digest of
  `big-code-analysis-X.Y.Z-${BCA_TARGET}.tar.gz` from the release's
  `SHA256SUMS` asset; it cannot exist before the `sign-attest` job
  runs, which is why this category exists at all. Bump the version
  and the sha together, never one without the other.
- The `taiki-e/install-action` pins
  (`tool: big-code-analysis-cli@X.Y.Z`, two sites), the
  `cargo binstall ... --version X.Y.Z` line, and the cargo cache key
  `bca-${{ runner.os }}-X.Y.Z`. These carry no checksum and could
  technically move at release-prep, but they mean "a published
  release", so they move with their sha-pinned siblings in the one
  post-release sweep.

Fetch the new checksum straight from the release:

```bash
VERSION=X.Y.Z
gh release download "v${VERSION}" -R dekobon/big-code-analysis \
  -p SHA256SUMS -O - | grep 'x86_64-unknown-linux-gnu\.tar\.gz$'
```

This follow-up commit is part of cutting the release, not of the
next development cycle; land it before (or together with) the
post-tag version bump.

### Leave alone when bumping main to the next version

The post-tag bump to the next version changes the workspace
manifests (the `Cargo.toml` sites listed under
[Bumping the version](#bumping-the-version), plus `Cargo.lock`) and
the regenerated `man/*.1`, and **nothing else**. In particular do
not touch:

- Any pin in `recipes/ci.md` (the previous category): readers deploy
  the published release, not `main`.
- The dependency examples and `(currently ...)` strings from the
  release-prep category: they keep reading the published release
  until the next release-prep commit moves them.

### Never change (outside a major bump)

- Major-line strings: `2.x` in `STABILITY.md`, `AGENTS.md`,
  `README.md`, `docs/conventions/documentation.md`, the book's
  stability page, and this file, plus the README dependency example
  `big-code-analysis = "2"`. These move only when `3.0` ships.
- Historical records: released `CHANGELOG.md` sections, prose
  describing past releases ("shipped in `2.0.0`", "removed in 2.0"),
  and `docs/development/lessons_learned.md`.
- Illustrative placeholders: the `v1.2.0` / `v0.1.0` example
  versions in this file's command snippets.
- External spec versions that merely look like ours: the
  [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html) and
  [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/)
  URLs.

## Pre-release checklist

Before tagging, on `main`:

- [ ] All intended changes are merged and CI is green.
- [ ] Workspace version is bumped per
      [Bumping the version](#bumping-the-version): all
      `Cargo.toml` sites, plus a refreshed `Cargo.lock`.
- [ ] `cargo xtask` has been run and the resulting `man/*.1` edits
      are committed in the release-prep commit. `git diff man/`
      after a fresh `cargo xtask` must be empty.
- [ ] `CHANGELOG.md` has a `## [x.y.z]` section with the release
      notes. The header must match the tag exactly, minus the
      leading `v`. Move entries out of `## [Unreleased]` into the
      new section.
- [ ] Reader-facing documentation pins read the new version per the
      release-prep category of
      [Version strings in documentation](#version-strings-in-documentation),
      and the scoped `rg` sweep there shows no stragglers.
- [ ] `cargo test --workspace --all-features` passes locally
      (including integration snapshots; initialize submodules first).
- [ ] `minisign.pub` is a real key (run
      `grep '^untrusted comment: placeholder' minisign.pub`; it
      should print nothing).
- [ ] `make check-publish-metadata` passes. It is the only pre-tag
      gate on the three top-level crates' publish metadata — none of
      them can be `cargo publish --dry-run`-ed before the tag — and it
      is what catches an `[package].include` block that has regressed
      or stopped covering a newly-added directory. See
      [Vendored tree-sitter grammar publishability](#vendored-tree-sitter-grammar-publishability).
- [ ] The defer-and-gate variables (`ENABLE_CRATES_PUBLISH`,
      `ENABLE_HOMEBREW_TAP`, `ENABLE_SCOOP_BUCKET`) are set to the
      intended state for this release.

Then commit, gate, and tag, in that order:

1. **Commit** the release-prep changes. This is the last commit on
   `main` before the tag.
2. **Run `make release-check VERSION=x.y.z`.** This is the pre-tag
   gate: license and advisory policy, the two `THIRD-PARTY-LICENSES`
   renders, a real `cargo publish --dry-run` of the five vendored
   grammar leaves, and the publish-metadata and changelog checks
   above. It only works on a committed tree, because those dry-runs
   are `cargo publish`, which refuses to package a directory holding
   uncommitted changes, and the release-prep commit always touches
   all five leaves (each carries the version and the install
   snippet). If the gate fails, amend the commit and re-run. Do not
   reach for the `--allow-dirty` that cargo's error suggests: it
   would gate content the tag will not carry.
3. **Push, then tag**
   ([Cutting a stable release](#cutting-a-stable-release)). The gate
   is entirely local, so the push can happen on either side of
   step 2.

## Cutting a stable release

Pick a semver version (e.g. `1.2.0`). The tag is the version prefixed
with `v`. Work through the
[Pre-release checklist](#pre-release-checklist) first:
`make release-check VERSION=1.2.0` runs there, against the
release-prep commit and before this tag.

```bash
# From a clean main checkout at the release-prep commit:
git tag -a v1.2.0 -m "v1.2.0"
git push origin v1.2.0
```

That's it: the push of the tag triggers `release.yml` **and** the two
PyPI wheel workflows (`python-wheels.yml` for the library bindings,
`python-cli-wheels.yml` for the `bca` CLI), all in parallel. Watch all
three in the Actions tab:

```bash
gh run watch
# or, per workflow:
gh run list --workflow=Release
gh run list --workflow="Python wheels"
gh run list --workflow="Python CLI wheels"
```

The wheel workflows publish to PyPI automatically once their one-time
Trusted Publishers are registered (see the
[Post-public-release checklist](#post-public-release-checklist)); no
per-release action beyond the tag is needed. Confirm both wheels landed
in [Post-release verification](#post-release-verification).

## Cutting a pre-release

Pre-release tags match `vX.Y.Z-<suffix>` where `<suffix>` is
`[A-Za-z][0-9]*`, e.g. `v1.2.0-rc1`, `v1.2.0-beta2`,
`v1.2.0-alpha3`. **Do not use dotted forms like `v1.2.0-rc.1`**:
Alpine's abuild grammar rejects dots in the pre-release suffix.

The preflight classifier sets `prerelease=true` for any suffix, which:

- Marks the GitHub Release as a pre-release.
- Skips the Homebrew tap, Scoop bucket, and crates.io publish steps
  regardless of the defer-and-gate variables. crates.io uploads are
  irrevocable, so rehearsal tags like `v0.0.0-test1` must not reach
  the registry.

Use this for any version that should not reach package managers.
Signed artefacts, SBOMs, and SLSA provenance still publish normally,
so a pre-release is a full test of everything except the external
pushes.

To re-run the release workflow against a tag that already exists,
dispatch it *from* the tag — `gh workflow run release.yml --ref
v0.0.0-test1`, or pick the tag under "Use workflow from" in the
Actions tab. There is no tag input: preflight refuses a dispatch from
a branch, so the checked-out commit is always the tag's. A dispatch
runs the workflow file *at that tag*, so tags cut before this change
(`v2.1.0` and earlier) still carry the old required `tag` input and
need `-f tag=<tag>` as well. The flip side: a fix to `release.yml`
itself cannot be rehearsed against an existing tag, because the
dispatch runs the tag's copy of the file — cut a fresh `v0.0.0-testN`
on the commit that carries the fix instead.

Both PyPI wheel workflows still **build and smoke-test** every wheel on
a pre-release tag but **skip the PyPI publish step**, so a pre-release
never lands a wheel on PyPI. The CLI wheel (`python-cli-wheels.yml`)
skips publish for *any* hyphenated suffix (`!contains(github.ref,
'-')`, matching `release.yml`'s `*-*` prerelease rule exactly); the
library wheel (`python-wheels.yml`) skips the recognised `-rc` /
`-beta` / `-alpha` suffixes. For the suffixes this project actually
uses (above), all three pipelines stay aligned: one tag cannot publish
a prerelease to one registry while skipping another.

## Post-release verification

The pipeline's own `verify` job downloads the musl tarball from the
published Release and re-runs minisign + SLSA verification. That
covers the critical path automatically.

Verify manually if you want extra assurance:

```bash
# From a fresh directory:
TAG=v0.1.0
VERSION=0.1.0
TARBALL="big-code-analysis-${VERSION}-x86_64-unknown-linux-musl.tar.gz"
gh release download "$TAG" -R dekobon/big-code-analysis \
  -p "$TARBALL" -p SHA256SUMS -p SHA256SUMS.minisig

# Fetch minisign.pub from the tag, not main — if the key was rotated
# after this release, main has a different key and verification fails.
RAW_BASE="https://raw.githubusercontent.com/dekobon/big-code-analysis"
curl -fsSLO "${RAW_BASE}/${TAG}/minisign.pub"
minisign -Vm SHA256SUMS -p minisign.pub
grep "${TARBALL}" SHA256SUMS | sha256sum -c
gh attestation verify "${TARBALL}" -R dekobon/big-code-analysis
```

Check that the downstream package managers updated (only applicable
once the corresponding gating variable is on):

- Homebrew tap: new commit on `dekobon/homebrew-tap` touching
  `Formula/big-code-analysis.rb`.
- Scoop bucket: new commit on `dekobon/scoop-bucket` touching
  `bucket/big-code-analysis.json`.

Confirm both PyPI wheels published at the new version (these ship on
every tag once their Trusted Publishers are registered). Either check
the project pages (<https://pypi.org/project/big-code-analysis/> and
<https://pypi.org/project/big-code-analysis-cli/>) or verify the CLI
end-to-end from a clean environment:

```bash
VERSION=0.1.0
python -m venv /tmp/bca-rel && . /tmp/bca-rel/bin/activate
# Library bindings (importable module):
pip install "big-code-analysis==${VERSION}"
python -c "import big_code_analysis as bca; print(bca.__version__)"
# CLI tool (drops `bca` on PATH):
pip install "big-code-analysis-cli==${VERSION}"
bca --version   # must print the tagged version
deactivate
```

Finally, land the documentation follow-up commit: update every pin
in `big-code-analysis-book/src/recipes/ci.md` (the post-publish
category of
[Version strings in documentation](#version-strings-in-documentation))
to the new version, with the `BCA_SHA256` values taken from the
release's `SHA256SUMS` asset.

## Post-public-release checklist

The first time the repository goes public and a stable release is
cut, complete the items below in order. None of them belongs in the
per-release flow, but skipping any of them on the cutover release
turns into a foot-gun on the *next* release.

- [ ] **crates.io ownership and Trusted Publisher.** For each of
      the eight publishable crates (the five `bca-tree-sitter-*`
      leaves, `big-code-analysis`, `big-code-analysis-cli`,
      `big-code-analysis-web`): claim the name with a manual
      `cargo publish` (leaf-first, retry on the new-crate rate
      limit; see [crates.io ownership](#cratesio-ownership) for
      the loop), add at least one co-owner via `cargo owner
      --add`, and register a Trusted Publisher (repo owner
      `dekobon`, repo `big-code-analysis`, workflow `release.yml`,
      environment `release`).
- [ ] **PyPI Trusted Publisher and `pypi` GH environment.** Claim
      `big-code-analysis` on PyPI via the pending-publisher flow at
      <https://pypi.org/manage/account/publishing/> (registers the
      TP and reserves the name in one step), and create the `pypi`
      GitHub environment so protection rules can attach before the
      first wheel publish. See [Python wheels
      (PyPI)](#python-wheels-pypi).
- [ ] **`python-wheels` PR label.** Create the label (see the
      Python wheels section) so contributors can opt PRs into the
      wheel matrix.
- [ ] **PyPI Trusted Publisher and `pypi-cli` GH environment (CLI
      wheel).** Claim `big-code-analysis-cli` on PyPI via the
      pending-publisher flow, registering a TP for workflow
      `python-cli-wheels.yml` + environment `pypi-cli`, and create the
      `pypi-cli` GitHub environment. See [CLI wheels
      (PyPI)](#cli-wheels-pypi).
- [ ] **`python-cli-wheels` PR label.** Create the label (see the CLI
      wheels section) so contributors can opt PRs into the CLI wheel
      matrix.
- [ ] **Shared Homebrew tap reachable.** Confirm
      `dekobon/homebrew-tap` exists and the configured PAT can push to
      it. The release workflow appends `Formula/big-code-analysis.rb`
      to that tap alongside the other formulae; no dedicated tap repo
      is required.
- [ ] **Shared Scoop bucket reachable.** Confirm
      `dekobon/scoop-bucket` exists and the configured PAT can push
      to it. The release workflow appends
      `bucket/big-code-analysis.json` alongside the other manifests;
      no dedicated bucket repo is required.
- [ ] **Fine-grained PATs minted and stored.** Generate
      `HOMEBREW_TAP_TOKEN` and `SCOOP_BUCKET_TOKEN` as fine-grained
      PATs scoped to the tap and bucket repos respectively, with
      write access only. Store under Settings → Secrets and
      variables → Actions.
- [ ] **Repo secrets and variables wired.** Confirm
      `MINISIGN_SECRET_KEY`, `MINISIGN_PASSWORD`, the Alpine abuild
      pair (if Alpine ships), `HOMEBREW_TAP_TOKEN`, and
      `SCOOP_BUCKET_TOKEN` are all present. Confirm the
      defer-and-gate variables (`ENABLE_CRATES_PUBLISH`,
      `ENABLE_HOMEBREW_TAP`, `ENABLE_SCOOP_BUCKET`) are set to
      `true` for the cutover release.
- [ ] **First release tag.** Cut the first stable tag with all gates
      on. Watch the `publish-crates`, `homebrew-tap-push`, and
      `scoop-bucket-push` jobs end-to-end. The `verify` job's
      success on the published tarball is the canonical "release
      is done" signal.
- [ ] **Delete any stray `CARGO_REGISTRY_TOKEN` secret** after the
      first successful TP-authenticated release. Leaving it around
      is not actively harmful (nothing references it), but deleting
      it removes a tempting footgun for a future contributor.

## Python wheels (PyPI)

Python bindings ship via `.github/workflows/python-wheels.yml`, not
`release.yml`. The two workflows trigger on the same `v*` tag push
but run in parallel; a crates.io publish failure does not block the
PyPI upload, and vice versa.

What the python-wheels pipeline does:

1. **build**: `PyO3/maturin-action@v1.51.0` builds a manylinux_2_28
   abi3 wheel on `ubuntu-latest` (x86_64) and `ubuntu-24.04-arm`
   (aarch64). `[tool.maturin].features` in
   `big-code-analysis-py/pyproject.toml` pins
   `pyo3/extension-module` + `pyo3/abi3-py312` so the wheel uses
   the limited (stable) Python C API and targets CPython 3.12+
   forward-compatibly. One wheel per architecture covers every
   future 3.12+ minor release.
2. **sdist**: `maturin sdist` produces a source distribution as
   the PyPI fallback for niche architectures and a
   reproducibility anchor for the wheels.
3. **smoke-test**: pulls each wheel onto a clean runner of the
   matching architecture, installs it with
   `pip install --no-index --find-links=dist big-code-analysis`,
   and asserts that the public API surface
   (`analyze_source`, `flatten_spaces`, `to_sarif`,
   `language_for_file`) loads and produces the documented dict
   shape under both Python 3.12 and 3.13. An abi3 wheel that
   loaded on 3.12 but failed on 3.13 (the most plausible silent
   forward-compat regression) trips here.
4. **publish**: gated on a `v*` tag and the `pypi` deployment
   environment. Authentication is via PyPI Trusted Publishing
   (OIDC); the workflow has no `PYPI_API_TOKEN` secret to leak.
   PEP 740 Sigstore attestations are generated automatically by
   `pypa/gh-action-pypi-publish@v1.14.0`.

### One-time PyPI setup

Before the first `v*` tag is cut after the cutover, complete these
on PyPI as the maintainer:

1. **Claim the project name.** Open
   `https://pypi.org/project/big-code-analysis/`. If the name is
   taken by another project, pick a different name and bump
   `[project] name` in `big-code-analysis-py/pyproject.toml`
   before tagging.

2. **Register a Trusted Publisher.** Under
   `https://pypi.org/manage/account/publishing/` (for a brand new
   project, the *pending* publisher flow at the same URL works
   the same way), add a GitHub publisher with:

   - PyPI Project Name: `big-code-analysis`.
   - Owner: `dekobon`.
   - Repository name: `big-code-analysis`.
   - Workflow filename: `python-wheels.yml` (basename only).
   - Environment name: `pypi`.

   The environment name is intentionally distinct from the
   `release` environment used by the crates.io trusted publisher
   in `release.yml`; keeping them separate prevents the OIDC
   `environment` claim from accidentally satisfying the wrong
   registry's TP entry.

3. **Create the `pypi` GitHub Environment.** Settings →
   Environments → New environment → `pypi`. The publish job
   references this environment; protection rules (required
   reviewers, branch / tag filters) attached here are the right
   place to add a manual approval gate on every wheel publish.

   ⚠️ GitHub will auto-create a referenced-but-undefined
   environment with **no protection rules** the first time the
   workflow runs. Create the environment manually *before* the
   first `v*` tag if you want the approval gate to apply on the
   first publish; otherwise the cutover release goes through
   immediately with no manual checkpoint.

4. **Create the `python-wheels` PR label.** The wheel build /
   sdist / smoke-test jobs are gated by a `python-wheels` label
   on PRs so Rust-only PRs that happen to share a path-filter
   neighbour (e.g. `Cargo.lock`) do not pay the wheel-matrix
   cost. GitHub does not auto-create custom labels; until the
   label exists, contributors cannot opt PRs into wheel
   verification. One-off via the `gh` CLI:

   ```bash
   gh label create python-wheels \
     --color 1d76db \
     --description "PR opts in to the manylinux wheel CI matrix"
   ```

   Tag pushes and `workflow_dispatch` runs ignore the label;
   they always build the full matrix.

5. **First tagged release validates the path.** Trusted
   Publishing cannot be rehearsed via `workflow_dispatch` (the
   environment claim mismatches). The first non-prerelease `v*`
   tag after registration is the canonical end-to-end test;
   watch the `publish` job's log for the OIDC exchange and the
   attestation upload.

### Version coupling

`big-code-analysis-py` inherits its version from
`[workspace.package] version` via `version.workspace = true` in its
`Cargo.toml`, and `pyproject.toml` reads the same value at build
time (`dynamic = ["version"]`). The "Bumping the version" steps
above are therefore sufficient: there is no separate
`big-code-analysis-py/pyproject.toml` version field to keep in sync.

### Testing a release candidate without uploading

For the two PyPI wheel workflows, `workflow_dispatch` from the
**Actions** tab runs the full build + smoke-test matrix without
invoking the publish job (the `if:` guard requires a `v*` tag push).
Use this to validate a release-prep branch before tagging. This does
not apply to `release.yml`, whose preflight refuses a branch dispatch
— rehearse it from a pre-release tag as described above.

To exercise the PyPI side end-to-end against
`https://test.pypi.org/`, temporarily change the
`pypa/gh-action-pypi-publish` step's `repository-url` input to
`https://test.pypi.org/legacy/` and register a matching TP entry
on TestPyPI; keep this off `main` to avoid leaking a real upload
into a production-shaped flow.

### Out of scope

The wheel pipeline ships Linux only (x86_64 + aarch64). macOS and
Windows wheels are tracked separately under
[#103](https://github.com/dekobon/big-code-analysis/issues/103)'s
"Out of scope" section. (This Linux-only scope is for the *library*
bindings wheel above; the **CLI** `bca` wheel, see below, does ship
macOS and Windows.)

## CLI wheels (PyPI)

The `bca` command-line tool ships as its own pip-installable
distribution via `.github/workflows/python-cli-wheels.yml`, separate
from both `release.yml` (crates.io / native packages) and
`python-wheels.yml` (the library bindings). All three trigger on the
same `v[0-9]*` tag push and run in parallel; a failure in one does not
block the others.

This is a `maturin -b bin` wheel: the compiled `bca` binary is packaged
as a console script, so `pip install big-code-analysis-cli` drops `bca`
onto the user's `PATH`. Key differences from the library wheel:

- **No abi3 / no per-Python matrix.** A bin wheel is tagged
  `py3-none-<platform>`; one wheel per (OS, arch) covers every CPython
  3.x and PyPy. The matrix is per-platform, not per-Python-version.
- **Distribution name `big-code-analysis-cli`, command `bca`.** The
  installed command intentionally differs from the dist name (the `bca`
  name on PyPI is taken; `big-code-analysis` is the library bindings).
- **Full grammar set is inherited from the crate** (`all-languages`, via
  #252); no `[tool.maturin] features` wiring.
- **Wider platform matrix:** Linux `manylinux_2_28` (`x86_64` /
  `aarch64`), macOS (`x86_64` / `arm64`), Windows (`x86_64`).
- **Compliance artefacts ride in the wheel:** the per-binary
  `THIRD-PARTY-LICENSES-bca.md` (cargo-about) and `LICENSE` land in
  `.dist-info/licenses/`; the `bca` man pages are bundled; maturin emits
  a CycloneDX SBOM into `.dist-info/sboms/`.

### One-time PyPI setup

Mirror the library-wheel setup above, with CLI-specific values:

1. **Claim the project name** at
   `https://pypi.org/project/big-code-analysis-cli/` (the
   pending-publisher flow reserves the name in the same step as the TP
   registration). The name was confirmed available when #408 was filed.

2. **Register a Trusted Publisher** at
   `https://pypi.org/manage/account/publishing/` with:

   - PyPI Project Name: `big-code-analysis-cli`.
   - Owner: `dekobon`.
   - Repository name: `big-code-analysis`.
   - Workflow filename: `python-cli-wheels.yml` (basename only).
   - Environment name: `pypi-cli`.

   `pypi-cli` is intentionally distinct from the library's `pypi`
   environment and the crates.io `release` environment so each
   registry/project's OIDC `environment` claim is unambiguous.

3. **Create the `pypi-cli` GitHub Environment** (Settings →
   Environments → New environment → `pypi-cli`) before the first `v*`
   tag if you want a manual approval gate on the first publish; GitHub
   auto-creates a referenced-but-undefined environment with no
   protection rules otherwise.

4. **Create the `python-cli-wheels` PR label** so contributors can opt a
   PR into the wheel matrix (Rust-only PRs that merely brush a
   path-filter neighbour skip it):

   ```bash
   gh label create python-cli-wheels \
     --color 1d76db \
     --description "PR opts in to the bca CLI wheel CI matrix"
   ```

   Tag pushes and `workflow_dispatch` runs ignore the label.

5. **First tagged release validates the path**, exactly as for the
   library wheel: Trusted Publishing cannot be rehearsed via
   `workflow_dispatch`.

### Version coupling

`big-code-analysis-cli` inherits its version from
`[workspace.package] version` (`version.workspace = true`), and its
`pyproject.toml` reads the same value at build time (`dynamic =
["version"]`). No separate version field to maintain.

## Rotating the minisign key

1. Generate a new keypair:
   `minisign -G -p minisign.pub.new -s minisign.key.new`.
2. Replace `minisign.pub` with the new public key and commit it.
3. Update `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD` secrets with
   the new values. Use stdin redirection (`gh secret set
   MINISIGN_SECRET_KEY -R dekobon/big-code-analysis < minisign.key.new`)
   to preserve the trailing newline of the key file; see
   [Minisign key](#minisign-key) for why paste-via-UI bites.
4. Cut a new release; its `SHA256SUMS.minisig` will be signed with
   the new key, self-documenting the rotation.

Users verifying an older release still need the old `minisign.pub`
from that release's tagged commit.

## Fixing a broken release

The pipeline fails *before* `publish` on any preflight, build,
package, smoke, or `sign-attest` error, so a broken release almost
never reaches users. `sign-attest` is the latest hard-gate before
external state changes; it is the right place to expect a noisy red
if `MINISIGN_SECRET_KEY` is missing, corrupted, or doesn't pair with
`MINISIGN_PASSWORD`.

`post-publish verify` runs *after* `publish` and is an internal
sanity check; its failure does not invalidate the published
artefacts and does not roll back any external state. Treat a `verify`
red as a CI bug to triage, not as a botched release.

If publish itself partially succeeds (e.g. GitHub Release created but
tap push failed), the fix is usually to re-run the workflow against
the same tag: **Actions** tab → open the failed run → **Re-run
failed jobs** (top-right of the run page). The pipeline is designed
to be idempotent on re-run, and re-runs pick up freshly-set repo
secrets without needing a force-retag.

If you need to pull a release entirely:

```bash
gh release delete vX.Y.Z --cleanup-tag --yes
```

Then fix the underlying issue, bump to `vX.Y.(Z+1)`, and re-tag.
**Do not re-use a published version number**: Homebrew/Scoop and
crates.io users may have already cached the old artefacts.

### Cutover-only escape hatch: force-moving the tag

The recovery rule above (bump to the next patch version) is correct
for any release that already produced external state. On the *very
first* tag for a brand-new repo, before `publish` has touched
crates.io / Homebrew / Scoop, no downstream state exists yet to
poison, and bumping the version mid-cutover adds churn (workspace
version, man pages, SARIF snapshots, CHANGELOG section). In that
narrow window, force-moving the tag is the cheaper recovery:

```bash
# Fix and push the underlying issue first
git push origin main

# Move the tag to point at the fix
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

This is **only safe** while:

- The GitHub Release object does not yet exist (or contains nothing
  irrevocable).
- `python-wheels.yml` has not yet uploaded to PyPI (PyPI versions are
  immutable; a re-fire of the tag will trip the publish step but
  won't roll back). Accept that single noisy red if the wheels are
  already correctly on PyPI.
- crates.io has not yet been told about this version. ANY publish
  for the version (workflow-driven *or* manual `cargo publish` from
  the maintainer's workstation) makes a force-retag inappropriate,
  because the published version is irrevocable (yank-able, not
  delete-able).

Outside that window, never force-move; use `vX.Y.(Z+1)`.

### Known pipeline limitations

The one standing limitation a maintainer should know about:

- `x86_64-unknown-freebsd` is absent from the binary matrix
  ([#346](https://github.com/dekobon/big-code-analysis/issues/346),
  closed): cross v0.2.5 + the vendored grammars' C++ scanners cannot
  link against `libcxxrt` without a deeper toolchain change. While
  the target is absent, FreeBSD users install from source.
