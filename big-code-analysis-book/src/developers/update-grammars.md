# Update grammars

Each programming language needs to be parsed in order to extract its
syntax and semantic: the so-called grammar of a language.
In `big-code-analysis`, we use
[tree-sitter](https://github.com/tree-sitter) as parsing library since
it provides a set of distinct grammars for each of our supported
programming languages. But a grammar is not a static monolith, it
changes over time, and it can also be affected by bugs, hence it is
necessary to update it every now and then.

As now, since we have used `bash` scripts to automate the operations,
grammars can be updated natively **only** on `Linux` and `MacOS`
systems, but these scripts can also run on `Windows` using `WSL`.

In `big-code-analysis` we use both **third-party** and **internal** grammars.
The first ones are published on `crates.io` and maintained by external developers,
while the second ones have been thought and defined inside the project to manage variant of some languages
used in `Firefox`.
We are going to explain how to update both of them in the following sections.

## Third-party grammars

Update the grammar version in `Cargo.toml` and `enums/Cargo.toml`.
Below an example for the `tree-sitter-java` grammar

```toml
tree-sitter-java = "x.xx.x"
```

where `x` represents a digit.

Run `./recreate-grammars.sh` to recreate and refresh all grammars
structures and data

```bash
./recreate-grammars.sh
```

Once the script above has finished its execution, you need to fix,
if there are any, all failed tests and problems introduced by changes
in the grammars.

Commit your changes and create a new pull request

## Internal grammars

Update the version of `tree-sitter-cli` in the `package.json` file of
the internal grammar and then install the updated version.

The five vendored grammars publish under the `bca-tree-sitter-*`
namespace (see `RELEASING.md` for the rename rationale), but consumer
call sites still reference them as `tree-sitter-<lang>` via Cargo's
`package = ...` alias. **A grammar refresh does not bump the leaf's
version on its own** — every crate in this repository shares one
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
untouched — the rename trick in `[lib]` is what keeps Rust import
paths stable, and the version line is managed by the lockstep
bump at release time.

Run the appropriate script to update the grammar by recreating and
refreshing every file and script.

For `tree-sitter-ccomment` and `tree-sitter-preproc` run
`./generate-grammars/generate-grammar.sh` followed by the name of the
grammar.
Below an example always using the `tree-sitter-ccomment` grammar

```bash
./generate-grammars/generate-grammar.sh tree-sitter-ccomment
```

Instead, for `tree-sitter-mozcpp` and `tree-sitter-mozjs`, use their specific scripts.

For `tree-sitter-mozcpp`, run

```bash
./generate-grammars/generate-mozcpp.sh
```

For `tree-sitter-mozjs`, run

```bash
./generate-grammars/generate-mozjs.sh
```

Once the script above has finished its execution, you need to fix,
if there are any, all failed tests and problems introduced by changes
in the grammars.

Commit your changes and create a new pull request
