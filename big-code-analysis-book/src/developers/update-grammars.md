# Update grammars

How to bump a [tree-sitter](https://tree-sitter.github.io/tree-sitter/)
grammar, for contributors maintaining the grammar pins.
`big-code-analysis` parses each language with a tree-sitter grammar.
Grammars change over time and carry bugs of their own, so they need
updating now and then.

The update scripts are `bash`, so grammars can be regenerated natively
**only** on Linux and macOS. On Windows, run the scripts under
[Windows Subsystem for Linux](https://learn.microsoft.com/en-us/windows/wsl/)
(WSL).

Two kinds of grammar are in use: **third-party** grammars published on
`crates.io` and maintained by external developers, and **internal**
grammars defined inside the project for the variants of some languages
used in Firefox. Each has its own procedure below.

## Third-party grammars

Update the grammar version in `Cargo.toml` and `enums/Cargo.toml`.
For example, for the `tree-sitter-java` grammar:

```toml
tree-sitter-java = "=x.xx.x"
```

where `x` represents a digit. The pin must be exact (`=X.Y.Z`);
`make check-excluded-manifests` rejects a caret range. The same
version also appears as the last field of the language's `mk_langs!`
tuple in `big-code-analysis-ast/src/langs.rs`, and the
`grammar_version_matches_cargo_toml_pin` test there fails until the
two agree.

Run `./recreate-grammars.sh` to regenerate every grammar's enum:

```bash
./recreate-grammars.sh
```

Once the script finishes, fix any failed tests and problems the
grammar changes introduced. `make enums-codegen-drift` (part of `make
pre-commit`) re-runs the codegen into a temporary directory and fails
if the checked-in `language_*.rs` or `c_langs_macros/*.rs` files
differ from it.

Commit your changes and open a pull request.

## Internal grammars

Update the version of `tree-sitter-cli` in the `package.json` file of
the internal grammar, then refresh its committed `package-lock.json`
with `npm install --package-lock-only --ignore-scripts` in the same
directory and commit both files together. The regen scripts install
with `npm ci`, which fails loudly when the lockfile is missing or out
of sync with `package.json`. This keeps every regen hash-verified and
byte-reproducible ([OpenSSF Scorecard Pinned-Dependencies][ossf]).

[ossf]: https://github.com/ossf/scorecard/blob/main/docs/checks.md#pinned-dependencies

The five vendored grammars publish under the `bca-tree-sitter-*`
namespace (see `RELEASING.md` for the rename rationale), but consumer
call sites still reference them as `tree-sitter-<lang>` via Cargo's
`package = ...` alias. **A grammar refresh does not bump the leaf's
version on its own**: every crate in this repository shares one
workspace-wide version, and bumping the leaves out of step with the
parent is not allowed (see the "Lockstep version policy" in
`RELEASING.md`). Regenerate the parser tables, accept the resulting
test-snapshot drift, and ship the change under the current version.
The next workspace release picks up the new grammars at whatever
shared version the next tag declares.

If a regeneration also needs an updated `tree-sitter` *runtime*
dependency, bump the dev-dependency line inside the leaf's
`Cargo.toml`:

```toml
[dev-dependencies]
tree-sitter = "=x.x.x"
```

Leave `[package] name = "bca-tree-sitter-<lang>"`,
`[package] version`, and `[lib] name = "tree_sitter_<lang>"`
untouched; the rename trick in `[lib]` is what keeps Rust import
paths stable, and the version line is managed by the lockstep
bump at release time.

Then run the script for the grammar, which regenerates every file.

For `tree-sitter-ccomment` and `tree-sitter-preproc` run
`./generate-grammars/generate-grammar.sh` followed by the name of the
grammar, for example:

```bash
./generate-grammars/generate-grammar.sh tree-sitter-ccomment
```

`tree-sitter-mozcpp` and `tree-sitter-mozjs` have their own scripts.

For `tree-sitter-mozcpp`, run

```bash
./generate-grammars/generate-mozcpp.sh
```

For `tree-sitter-mozjs`, run

```bash
./generate-grammars/generate-mozjs.sh
```

Both of those crates carry a notification-only marker dependency in
their `Cargo.toml` (`tree-sitter-cpp` for mozcpp, `tree-sitter-javascript`
for mozjs) that Cargo never compiles. `make grammar-marker-sync`
compares each marker against `.grammar-marker-baseline.toml`, so a
[Dependabot](https://docs.github.com/en/code-security/dependabot)
bump of the marker without a regen fails the gate. After a
verified regen, refresh the baseline in the same commit with
`./utils/check-grammar-marker-sync.py --update`.

`tree-sitter-tcl`, the fifth vendored grammar, has no regeneration
script: it vendors pre-generated parser sources only (no
`grammar.js`), so updating it means re-vendoring the generated `src/`
from its upstream project rather than running `tree-sitter generate`
locally.

Once the script finishes, fix any failed tests and problems the
grammar changes introduced.

Commit your changes and open a pull request.
