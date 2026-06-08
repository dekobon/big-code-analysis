# Documentation: prose, markdown, docstrings

## Audience separation

Keep these channels distinct. Mixing them creates documents that age
badly:

- `README.md`: the problem this project solves and enough detail to
  decide whether to use it. Not a changelog.
- `CHANGELOG.md`: what changed between releases (see
  [Changelog](#changelog)).
- `STABILITY.md`: the public-API stability contract covering what is
  stable, what an additive change is, and what is reserved for a major
  bump.
- Doc comments (`///`, `//!`): how to use a specific function, type,
  or module. Not a spec.
- `docs/conventions/`: how contributors and agents work in this
  repository.
- `big-code-analysis-book/`: the mdBook: end-user and library
  documentation (metrics, commands, languages, recipes).

## Prose documents

These rules govern human-consumed prose: `README.md`,
`STABILITY.md`, everything under `docs/`, and the mdBook source under
`big-code-analysis-book/src/`. They do not govern doc comments (see
[Doc comments](#doc-comments)) or changelog and lessons entries, which
have their own sections below. All writing here is
engineer-to-engineer: write for a peer who may lack this domain's
context, not for an end customer and not as a beginner tutorial.

### Whether a doc should exist

A prose doc is for durable knowledge with no natural home in code, a
commit message, or an existing doc. Before adding a top-level file,
check for an existing home and extend it. Point-in-time artifacts
(review rebuttals, migration-day runbooks, status snapshots, email
drafts) are not docs.

| Put it in                       | When                                          |
| ------------------------------- | --------------------------------------------- |
| Commit message                  | Why this specific change was made             |
| Doc comment (`///`)             | How to call an API; why local to one item     |
| An existing doc (extend it)     | The topic already has a home                  |
| `docs/development/`             | Contributor-facing design and process notes   |
| `big-code-analysis-book/src/`   | End-user or library reference documentation   |
| A new prose doc                 | Durable knowledge with no existing home       |

### Structure and altitude

Open by orienting the reader: what this is, who it is for, and where
it fits, before any detail. Order content by decreasing relevance to
the common reader, putting most readers' question first and deep
internals last. One doc answers one question; split it if it answers
several. Keep headings shallow. Do not add a section because a
template "should" have one; an "Overview" that restates the title is
slop.

### Link to external concepts

On first mention, link any external tool, standard, RFC, or paper to
its authoritative source, so the reader never has to guess a search
term. Link the canonical home (project site, spec, RFC, paper), not a
blog.

- Bad: "We use tree-sitter to build the AST and compute Halstead
  metrics." (two external concepts, no link)
- Good: "We use
  [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to build
  the AST and compute
  [Halstead](https://en.wikipedia.org/wiki/Halstead_complexity_measures)
  metrics."

### Explain assumed background

State the knowledge a doc assumes and give a one- or two-sentence
primer, or a link, before relying on it. This is
engineer-to-engineer, not a tutorial: orient a peer who lacks this
domain's context with a link or a single sentence, not a lesson.

### Plain, honest sentences

Write the plainest sentence that is still precise. The bans that bite
most in docs:

- No marketing adjectives on internal tooling (robust, powerful,
  seamless, cutting-edge), no superlatives, no validation phrases.
- No throat-clearing ("It's important to note that..."), no
  "leverage" or "utilize" where "use" works, no "in order to" where
  "to" works.
- No em dashes or other generative-AI tells. No AI sentence
  structures: correlative conjunctions ("not only... but also",
  "whether... or"), contrastive-emphasis prepositional phrases,
  participial-phrase pile-ups.
- No hedging: state the fact, or name the uncertainty and give a
  concrete next step.
- No empty intros or conclusions that restate the title.
- No invented numbers presented as measurements (see
  [No stale counts](#no-stale-counts)); illustrative worked-example
  numbers are fine.

Aim for short sentences and paragraphs (about 20 words for a task
sentence, 25 for a conceptual one, 6 sentences per paragraph) and
noun clusters of at most three words, broken up with prepositions
("the location of the config file", not "the config file location
setting"). These are targets, not hard limits; do not reflow existing
prose just to hit a count.

- Bad: "This robust, cutting-edge module leverages a powerful
  graph-traversal engine to seamlessly deliver best-in-class results."
- Good: "This module walks the AST to count decision points."

### Parallel structure

List items, series, and heading sets share one grammatical form (all
imperative verbs, or all nouns).

- Bad: "Parse the source / The AST is traversed / Computing metrics"
- Good: "Parse the source / Traverse the AST / Compute metrics"

### Voice and tense

Active voice. Present tense for current state and system behavior.
Imperative for requirements and instructions ("Run `make
pre-commit`"). Past tense only in the changelog and lessons-learned,
which are point-in-time records. Be consistent within a doc.

### Dates

Use ISO 8601 (`YYYY-MM-DD`) for all dates, matching existing project
practice.

### Terminology and word choice

Keep general-English word distinctions consistent: update vs. upgrade,
configure vs. set up, ensure vs. make sure, and spell out Latin
abbreviations rather than writing "e.g." or "i.e." in prose. Use the
project's established names for metrics, crates, and binaries (`bca`,
`bca-web`, `FuncSpace`, `CodeMetrics`) rather than inventing synonyms.

### Revise in passes

Drafting and editing are different acts; do not polish while drafting.
After a first draft, revise in separate passes, each with one focus.
Re-read the file at the start of each pass rather than editing from
memory, since a draft reads differently once it is on the page:

1. Structure: check the opening orients and the order matches what the
   reader needs first.
2. Completeness: confirm every external concept is linked and every
   assumed term explained.
3. Line edit: tighten each sentence (see
   [Plain, honest sentences](#plain-honest-sentences)).
4. Proofread: run the self-review checklist below, then lint.

Stop when a pass turns up nothing new, not after a fixed number of
rounds. A second reader catches what the author cannot; when the doc
matters, have someone (or a fresh-context agent) review it before it
lands.

### Self-review checklist

Before committing a prose doc, confirm:

- The opening says what this is, who it is for, and where it fits.
- Every external tool, standard, or paper is linked on first mention.
- Assumed background is stated or linked before it is relied on.
- Each sentence is true and necessary. No hedging, em dashes,
  superlatives, AI sentence structures, or noun stacks. Lists and
  headings are parallel.
- No invented counts. Dates are ISO 8601. Register is
  engineer-to-engineer.
- Present tense, active voice, consistent throughout.
- This belongs as a doc, not a commit message, comment, or archive
  item.
- `rm -rf .rumdl_cache && make markdown-lint` passes.

## Doc comments

- Public functions, types, and modules need doc comments (`///`,
  `//!`) covering what they do, their parameters, return values, and
  the errors they can return.
- Include a usage example for non-obvious APIs; doc examples are
  compiled and run by `cargo test`, so keep them correct.
- Don't write comments that restate what the code does; do write
  comments that explain *why* when the reason isn't local. Default to
  no comment unless the *why* is non-obvious.

Example:

```rust
/// Compute maintainability metrics for a parsed source file.
///
/// Walks the [`FuncSpace`] tree produced by the parser and folds each
/// per-function metric into the enclosing space, returning the root
/// space for the whole file.
///
/// # Errors
///
/// Returns [`MetricsError`] if the source cannot be parsed into a tree
/// for the requested language.
pub fn metrics(&self, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
    // ...
}
```

## No stale counts

Never hardcode specific counts in documentation, comments, or
specifications. They go stale immediately.

- **Bad**: "25 tests passing", "6 supported languages", "71% pass
  rate".
- **Good**: "all tests passing", "every supported language",
  "majority of tests pass".
- Use approximate language when scale matters: "hundreds of",
  "dozens of".
- **Exceptions**: `CHANGELOG.md` entries (point-in-time snapshots)
  and code (compiler- or test-verified).

## Markdown

- Line length under around 100 characters. The linter
  (`.rumdl.toml`) hard-fails at 120, with tables and code exempt, so
  120 is a ceiling, not the target.
- ATX-style headings (`# Heading`), not Setext (`Heading\n=====`).
- Fenced code blocks with language identifiers (`rust`, `bash`,
  `toml`, `json`).
- No trailing whitespace.
- One blank line between a heading and the content above it.
- Inline code for file paths and identifiers (`` `FuncSpace` ``), not
  bold or italics.

## Changelog

`CHANGELOG.md` follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). This is a
published `1.x` library with a written stability contract in
[`STABILITY.md`](../../STABILITY.md); the detailed entry, sectioning,
and SemVer discipline lives in [`AGENTS.md`](../../AGENTS.md). The
rules that matter most for prose:

- New entries go under `## [Unreleased]` at the top; never invent a
  version number. Release prep is the only step that moves accumulated
  entries into a dated, versioned section.
- Add the entry in the same commit as the change, not as a later
  sweep. A changelog assembled after the fact misses the *why* and
  drifts from what shipped.
- One line per change, factual, terse, past tense, written for a user
  of the library. Describe the observable change, not the
  implementation.
- Mark a source-level break **(breaking)**; per `STABILITY.md`, such
  changes are deferred to the next major bump.

## Lessons learned

Hard-won project lessons live in
[`docs/development/lessons_learned.md`](../development/lessons_learned.md).
Keep the list small and actionable. Only document lessons that are
genuinely hard (cost real debugging time or caused real bugs) and
important (likely to recur). Err on the side of *not* adding entries.
This is not a changelog or a diary.

The `lessons-learned` skill is an editor helper only;
`docs/development/lessons_learned.md` remains the source of truth.

Shape of an entry:

- A short title describing the failure mode, not the fix.
- The observed bug or surprising behaviour.
- The underlying cause.
- The prevention rule that future code should follow.
