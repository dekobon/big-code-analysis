# Recipes

Task-oriented examples for getting work done with `bca` and `bca-web`.
Each recipe assumes you have built the binaries (`cargo build
--release`) and that `bca` is on your `PATH`.

The recipes are grouped by goal:

- [Quality reports](quality-reports.md) — generate Markdown reports
  suitable for pull requests, dashboards, or wikis, including the
  C/C++ preprocessor-aware workflow.
- [CI integration](ci.md) — wire `bca check` and `bca report` into
  GitHub Actions and GitLab CI, including the baseline / ratchet
  pattern and the Code Quality widget path.
- [AST queries](ast-queries.md) — search for syntactic constructs,
  count node types, dump trees, and detect parse errors.
- [Exporting metric data](exporting-data.md) — emit structured output
  (JSON / YAML / TOML / CBOR) and consume it from shell pipelines.
- [Driving the REST API](rest-api.md) — run the HTTP server and call
  every endpoint with `curl`.

If you want a deeper look at any flag the recipes use, see the
per-command pages under [Commands](../commands/README.md). For the
full list of metrics that show up in these recipes, see
[Supported Metrics](../metrics.md).

> **Upstream reference.** `big-code-analysis` is a fork of Mozilla's
> [`rust-code-analysis`](https://github.com/mozilla/rust-code-analysis).
> Recipes that work for the upstream `rust-code-analysis-cli` binary
> usually translate directly — replace the binary name and adjust for
> the subcommand restructure documented in the
> [migration guide](../migration.md).
