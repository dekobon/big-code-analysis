# Cutting a release

This document is the step-by-step procedure for releasing
`big-code-analysis`. It describes what to do, in what order, and what
to check when something looks wrong.

> **Status.** The release pipeline described here is being built up in
> stages (`S1`–`S8` of the public-release roadmap). The repository
> currently ships with a Cargo workspace, the MSRV declaration, the
> CHANGELOG, and the contributor docs. The signed-artefact pipeline,
> minisign key, packaging matrix, and external taps/buckets land in
> the remaining stages. Sections below that describe in-flight pieces
> say so explicitly.

The pipeline, once landed, is defined in
`.github/workflows/release.yml`. Everything downstream of `git push
--tags` is automated.

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

1. **preflight** — validates the tag, checks `Cargo.toml` version
   parity against `[workspace.package] version`, confirms
   `minisign.pub` is not a placeholder, and extracts the matching
   `CHANGELOG.md` section as release notes.
2. **build** — cross-compiles `bca` and `bca-web` for the target
   matrix (Linux gnu/musl x86_64+aarch64, macOS x86_64+aarch64,
   Windows x86_64+aarch64; FreeBSD x86_64 if added). Strips binaries,
   captures debug symbols, and produces per-target `.tar.gz` / `.zip`
   archives.
3. **package-*** — builds `.deb`, `.rpm`, `.apk`, and any other OS
   packages from the staged binaries.
4. **smoke-*** — installs each package inside the appropriate
   container/VM and asserts `bca --version` and `bca-web --version`
   match the tag.
5. **sign-attest** — flattens every artefact into `release/`,
   generates CycloneDX SBOMs, computes `SHA256SUMS`, signs it with
   minisign, and attaches SLSA build provenance.
6. **publish** — creates/updates the GitHub Release, attaches every
   artefact + `SHA256SUMS` + `SHA256SUMS.minisig`, and (for non
   pre-releases, **subject to the gating variables below**) pushes the
   Homebrew formula and Scoop manifest.
7. **publish-crates** — for non pre-releases, **subject to the gating
   variables below**, runs `cargo publish` for each publishable
   workspace crate in dependency order: `big-code-analysis` (library)
   first, then `big-code-analysis-cli` and `big-code-analysis-web`.
   Skips idempotently if the version is already on crates.io.
8. **verify** — downloads the published musl tarball back out of the
   release, verifies the minisign signature, checksum, and SLSA
   provenance.

If any stage fails, nothing downstream runs. `publish` and
`publish-crates` are the only jobs that mutate anything outside this
repo; they run in parallel so a crates.io failure does not block the
GitHub Release's `verify` step (and vice versa).

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
dependencies:

- `tree-sitter-ccomment`.
- `tree-sitter-mozcpp`.
- `tree-sitter-mozjs`.
- `tree-sitter-preproc`.
- `tree-sitter-tcl`.

These crates currently set `publish = false`. They are **not
publishable to crates.io as-is** because their crate names collide
with upstream tree-sitter grammar crates that we have diverged from,
and crates.io does not allow re-publishing under a name owned by a
different account. Three options exist for unblocking publication of
`big-code-analysis` itself:

1. **Rename the vendored forks** to a project-namespaced prefix —
   e.g. `bca-tree-sitter-ccomment`, `bca-tree-sitter-mozcpp`,
   `bca-tree-sitter-mozjs`, `bca-tree-sitter-preproc`,
   `bca-tree-sitter-tcl` — and publish them as separate crates.
   Cleanest from a downstream-consumer perspective; requires renaming
   the path dependencies in the root `Cargo.toml` and the inline
   `name = "tree-sitter-*"` in each crate.
2. **Inline the C sources** into the parent crate. Removes the
   separate-crate problem entirely but inflates the parent crate
   tarball and the build graph; only viable if the C grammar sources
   are stable enough that we never need to re-sync them from upstream
   tree-sitter.
3. **Skip crates.io for the workspace entirely** and ship only via
   tagged GitHub releases / homebrew / scoop. Lowest blast radius,
   highest friction for downstream Rust users (no `cargo add
   big-code-analysis`).

The decision has not been made yet. Until it is, `publish-crates`
should remain gated by `ENABLE_CRATES_PUBLISH` as described above.

## Prerequisites (one-time setup)

You only need to do this once per project, but verify each item
before the first real release.

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
`dekobon/homebrew-big-code-analysis` and
`dekobon/scoop-big-code-analysis` respectively.

crates.io authentication uses
[Trusted Publishing](https://crates.io/docs/trusted-publishing) — no
long-lived `CARGO_REGISTRY_TOKEN` is stored as a secret. The
`publish-crates` job mints a GitHub OIDC ID token and exchanges it for
a short-lived registry token scoped to that run.

If `HOMEBREW_TAP_TOKEN` or `SCOOP_BUCKET_TOKEN` is missing — or if the
target tap/bucket repo is unreachable (deleted, renamed, or the PAT
cannot see it) — the corresponding step emits a GitHub Actions
warning and skips without failing the release.

### Minisign key

`minisign.pub` at the repo root must be a real public key, not a
committed placeholder. The preflight job greps for the placeholder
comment and aborts if it is still present.

To create a fresh key:

```bash
minisign -G -p minisign.pub -s minisign.key
```

Commit `minisign.pub`. Paste the contents of `minisign.key` into
`MINISIGN_SECRET_KEY` and its password into `MINISIGN_PASSWORD`. Keep
`minisign.key` out of the repo.

### External repos

Stable releases push to (subject to the gating variables above):

- `dekobon/homebrew-big-code-analysis` — Homebrew tap.
- `dekobon/scoop-big-code-analysis` — Scoop bucket.
- crates.io — `big-code-analysis` (library) followed by
  `big-code-analysis-cli` and `big-code-analysis-web`.

Both tap and bucket repos must exist and accept the configured PAT.

### crates.io ownership

Before the first automated publish:

1. **Check name availability.** Open
   `https://crates.io/crates/big-code-analysis`,
   `https://crates.io/crates/big-code-analysis-cli`, and
   `https://crates.io/crates/big-code-analysis-web`. If any returns a
   crate owned by someone else, pick a different name and update the
   crate `Cargo.toml` before tagging — the `cargo owner --add` step
   below only works on crates you already own.
2. **Publish each crate manually once to claim the name.** `cargo
   owner --add` requires the crate to exist, so the owner-management
   step comes *after* the first publish, not before. From a clean
   checkout at the release-prep commit:

   ```bash
   cargo login <your-token>
   cargo publish -p big-code-analysis --locked
   cargo publish -p big-code-analysis-cli --locked
   cargo publish -p big-code-analysis-web --locked
   ```

   After that, the `publish-crates` job takes over for every
   subsequent release (and is a no-op for this same version because
   of the idempotency check).
3. **Add additional owners.** `cargo owner --add <github-handle>
   big-code-analysis` (and the same for the two binary crates). A
   single-owner crate is one forgotten password away from being
   orphaned. If you have a GitHub team, use
   `github:<org>:<team>`.
4. **Register a Trusted Publisher for each crate** (see below). This
   replaces any long-lived API token a future contributor might
   otherwise wire into the workflow.

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
   deployment branch filters) act as a manual gate on every publish —
   the environment is the right place to add them, not the workflow.
   The name must match the TP registration exactly; a typo here is
   the most common self-inflicted failure mode.

2. **Register a Trusted Publisher for each crate.** On crates.io,
   open the settings page for each of `big-code-analysis`,
   `big-code-analysis-cli`, and `big-code-analysis-web`. In the
   **Trusted Publishing** section, add a GitHub publisher with:

   - Repository owner: `dekobon`.
   - Repository name: `big-code-analysis`.
   - Workflow filename: `release.yml` (basename only, not a path).
   - Environment: `release`.

   Every publishable crate needs its own trusted-publisher entry — a
   TP registered on `big-code-analysis` does not cover the CLI or
   the web crate. The workflow still performs a single `auth`
   exchange for all publishes because crates.io issues one token
   covering every crate whose TP config matches the JWT claims.

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

1. Root `Cargo.toml`, `[workspace.package] version = "x.y.z"` — the
   canonical version that every member crate picks up via
   `version.workspace = true`.
2. Any `[workspace.dependencies]` entries that pin an internal crate
   (e.g. `big-code-analysis = { path = "...", version = "x.y.z", ...
   }`). Must match the workspace version, otherwise `cargo publish`
   on the dependent crate will reject the dependency.
3. The `enums/` helper workspace (excluded from the root workspace).
   Its own `Cargo.toml` carries an independent version; bump only if
   the enum-generation surface changes.

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

Pick the version using semver. While the workspace is in `0.x`, the
public Rust API surface (`big-code-analysis` library re-exports, the
`bca` CLI argument grammar, and the `bca-web` REST schema) may change
between minor versions; mark breaking changes with **(breaking)** in
the CHANGELOG entry.

Commit the version bump together with the changelog move (see below)
so the release-prep commit is a single, self-contained change:

```text
chore(release): prepare v0.1.0
```

## Pre-release checklist

Before tagging, on `main`:

- [ ] All intended changes are merged and CI is green.
- [ ] Workspace version is bumped per
      [Bumping the version](#bumping-the-version) — all
      `Cargo.toml` sites, plus a refreshed `Cargo.lock`.
- [ ] `CHANGELOG.md` has a `## [x.y.z]` section with the release
      notes. The header must match the tag exactly, minus the
      leading `v`. Move entries out of `## [Unreleased]` into the
      new section.
- [ ] `cargo test --workspace --all-features` passes locally
      (including integration snapshots — initialize submodules first).
- [ ] `minisign.pub` is a real key (run
      `grep '^untrusted comment: placeholder' minisign.pub` — it
      should print nothing).
- [ ] The defer-and-gate variables (`ENABLE_CRATES_PUBLISH`,
      `ENABLE_HOMEBREW_TAP`, `ENABLE_SCOOP_BUCKET`) are set to the
      intended state for this release.

Commit and push these changes. The final commit on `main` before
tagging should be the release-prep commit.

## Cutting a stable release

Pick a semver version (e.g. `0.1.0`). The tag is the version prefixed
with `v`.

```bash
# From a clean main checkout at the release-prep commit:
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

That's it — the push of the tag triggers `release.yml`. Watch it in
the Actions tab:

```bash
gh run watch
# or
gh run list --workflow=Release
```

## Cutting a pre-release

Pre-release tags match `vX.Y.Z-<suffix>` where `<suffix>` is
`[A-Za-z][0-9]*` — e.g. `v0.1.0-rc1`, `v0.1.0-beta2`,
`v0.1.0-alpha3`. **Do not use dotted forms like `v0.1.0-rc.1`**:
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

- Homebrew tap: new commit on `dekobon/homebrew-big-code-analysis`.
- Scoop bucket: new commit on `dekobon/scoop-big-code-analysis`.

## Post-public-release checklist

The first time the repository goes public and a stable release is
cut, complete the items below in order. None of them belongs in the
per-release flow, but skipping any of them on the cutover release
turns into a foot-gun on the *next* release.

- [ ] **crates.io ownership and Trusted Publisher.** For each of
      `big-code-analysis`, `big-code-analysis-cli`,
      `big-code-analysis-web`: claim the name with a manual
      `cargo publish`, add at least one co-owner via `cargo owner
      --add`, and register a Trusted Publisher (repo owner
      `dekobon`, repo `big-code-analysis`, workflow `release.yml`,
      environment `release`).
- [ ] **Homebrew tap repo created.** Create
      `dekobon/homebrew-big-code-analysis`. The release workflow's
      tap-push step expects this exact name.
- [ ] **Scoop bucket repo created.** Create
      `dekobon/scoop-big-code-analysis`. Same caveat.
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
but run in parallel — a crates.io publish failure does not block the
PyPI upload, and vice versa.

What the python-wheels pipeline does:

1. **build** — `PyO3/maturin-action@v1.51.0` builds a manylinux_2_28
   abi3 wheel on `ubuntu-latest` (x86_64) and `ubuntu-24.04-arm`
   (aarch64). `[tool.maturin].features` in
   `big-code-analysis-py/pyproject.toml` pins
   `pyo3/extension-module` + `pyo3/abi3-py312` so the wheel uses
   the limited (stable) Python C API and targets CPython 3.12+
   forward-compatibly. One wheel per architecture covers every
   future 3.12+ minor release.
2. **sdist** — `maturin sdist` produces a source distribution as
   the PyPI fallback for niche architectures and a
   reproducibility anchor for the wheels.
3. **smoke-test** — pulls each wheel onto a clean runner of the
   matching architecture, installs it with
   `pip install --no-index --find-links=dist big-code-analysis`,
   and asserts that the public API surface
   (`analyze_source`, `flatten_spaces`, `to_sarif`,
   `language_for_file`) loads and produces the documented dict
   shape under both Python 3.12 and 3.13. An abi3 wheel that
   loaded on 3.12 but failed on 3.13 (the most plausible silent
   forward-compat regression) trips here.
4. **publish** — gated on a `v*` tag and the `pypi` deployment
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
   in `release.yml` — keeping them separate prevents the OIDC
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
   first publish — otherwise the cutover release goes through
   immediately with no manual checkpoint.

4. **Create the `python-wheels` PR label.** The wheel build /
   sdist / smoke-test jobs are gated by a `python-wheels` label
   on PRs so Rust-only PRs that happen to share a path-filter
   neighbour (e.g. `Cargo.lock`) do not pay the wheel-matrix
   cost. GitHub does not auto-create custom labels — until the
   label exists, contributors cannot opt PRs into wheel
   verification. One-off via the `gh` CLI:

   ```bash
   gh label create python-wheels \
     --color 1d76db \
     --description "PR opts in to the manylinux wheel CI matrix"
   ```

   Tag pushes and `workflow_dispatch` runs ignore the label —
   they always build the full matrix.

5. **First tagged release validates the path.** Trusted
   Publishing cannot be rehearsed via `workflow_dispatch` (the
   environment claim mismatches). The first non-prerelease `v*`
   tag after registration is the canonical end-to-end test —
   watch the `publish` job's log for the OIDC exchange and the
   attestation upload.

### Version coupling

`big-code-analysis-py` inherits its version from
`[workspace.package] version` via `version.workspace = true` in its
`Cargo.toml`, and `pyproject.toml` reads the same value at build
time (`dynamic = ["version"]`). The "Bumping the version" steps
above are therefore sufficient — there is no separate
`big-code-analysis-py/pyproject.toml` version field to keep in sync.

### Testing a release candidate without uploading

`workflow_dispatch` from the **Actions** tab runs the full build +
smoke-test matrix without invoking the publish job (the `if:`
guard requires a `v*` tag push). Use this to validate a
release-prep branch before tagging.

To exercise the PyPI side end-to-end against
`https://test.pypi.org/`, temporarily change the
`pypa/gh-action-pypi-publish` step's `repository-url` input to
`https://test.pypi.org/legacy/` and register a matching TP entry
on TestPyPI — keep this off `main` to avoid leaking a real upload
into a production-shaped flow.

### Out of scope

The wheel pipeline ships Linux only (x86_64 + aarch64). macOS and
Windows wheels are tracked separately under
[#103](https://github.com/dekobon/big-code-analysis/issues/103)'s
"Out of scope" section.

## Rotating the minisign key

1. Generate a new keypair:
   `minisign -G -p minisign.pub.new -s minisign.key.new`.
2. Replace `minisign.pub` with the new public key and commit it.
3. Update `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD` secrets with
   the new values.
4. Cut a new release — its `SHA256SUMS.minisig` will be signed with
   the new key, self-documenting the rotation.

Users verifying an older release still need the old `minisign.pub`
from that release's tagged commit.

## Fixing a broken release

The pipeline fails *before* publish on any preflight, build, package,
or smoke error, so a broken release almost never reaches users.

If publish itself partially succeeds (e.g. GitHub Release created but
tap push failed), the fix is usually to re-run the workflow against
the same tag via **Actions → Run workflow**. The pipeline is designed
to be idempotent on re-run.

If you need to pull a release entirely:

```bash
gh release delete vX.Y.Z --cleanup-tag --yes
```

Then fix the underlying issue, bump to `vX.Y.(Z+1)`, and re-tag.
**Do not re-use a published version number** — Homebrew/Scoop and
crates.io users may have already cached the old artefacts.
