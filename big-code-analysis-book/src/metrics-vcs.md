# Supported Change-history (VCS) Metrics

This chapter is a guided tour of the **change-history metrics** —
the signals big-code-analysis derives from version-control history
rather than from the source AST. Where the
[Supported Code Metrics](./metrics.md) chapter measures the *shape*
of the code as it exists today, these metrics measure how that code
*got there*: how often it changes, how much, by how many people, and
in what kind of commits. Each section starts from the empirical paper
that first connected the signal to defects, walks through how
big-code-analysis computes it, and explains how to read the number in
practice.

The whole family is computed by the `bca vcs` command and attached to
`bca metrics --vcs` output; the [Commands → Change-history (VCS)
metrics](./commands/vcs.md) page documents the flags, windows, output
formats, and caching. This chapter is the *why*; that page is the
*how*.

A few framing notes before we start:

- **These metrics predict where defects cluster, not whether code is
  correct.** The defect- and vulnerability-prediction literature
  consistently finds that *process* signals — how a file has been
  edited over time — out-predict *product* signals like size or
  complexity. Graves *et al.* put it bluntly in 2000: the number of
  times a module has been changed is a better predictor of its fault
  count than its length. None of these numbers measures correctness;
  they rank files by the probability that a bug is hiding in one.
- **Everything is measured over two windows.** A single history walk
  per invocation produces every signal over a **long** window
  (default `12mo` ≈ 365 days) and a **recent** window (default
  `90d`). Recent activity is weighted more heavily than old activity
  throughout, because recency is the strongest single signal in the
  just-in-time defect-prediction line.
- **The composite scores are ordinal, not cardinal.** The
  `risk_score`, `hotspot_score`, and commit-level JIT score are
  rulers, not thermometers: only relative ranks carry meaning. Rank a
  repository's files (or commits) against each other, or against the
  repository's own distribution over time. Do not read an absolute
  magnitude as a probability or compare a raw score across unrelated
  projects.
- **An absent record is not a zero.** An untracked file has no record
  at all, which is distinct from a tracked file with zero in-window
  activity. A computed `0.0` (for example, a file that only ever
  changed alone has zero co-change entropy) is a real measurement.

## Index

| Metric | Measures | First connected to defects by |
|--------|----------|-------------------------------|
| [Commit frequency](#commit-frequency) | How often a file changes | Graves *et al.*, 2000 |
| [Code churn](#code-churn) | How many lines change | Nagappan & Ball, 2005 |
| [Burst](#burst) | Concentration of change into the recent window | recency principle (Graves; JIT line) |
| [Authorship and ownership](#authorship-and-ownership) | How many hands touch a file, and how concentrated | Bird *et al.*, 2011; Meneely & Williams, 2009 |
| [Fix, security, and revert commits](#fix-security-and-revert-commits) | History of corrective change | Śliwerski, Zimmermann & Zeller, 2005 |
| [Age and last-modified](#age-and-last-modified) | When a file was born and last touched | just-in-time defect-prediction line |
| [Change entropy](#change-entropy) | How *scattered* the commits touching a file are | Hassan, 2009 |
| [Co-change entropy](#co-change-entropy) | How wide a file's change-ripple blast radius is | arXiv 2504.18511, 2025 |
| [Composite risk score](#composite-risk-score) | Weighted roll-up of every signal above | this project (formula `v2`) |
| [Hotspot score](#hotspot-score) | Complexity × recent churn | Tornhill, 2015 |
| [Bus factor](#bus-factor) | Knowledge concentration across a set of files | Avelino *et al.*, 2016 |
| [Just-in-time commit score](#just-in-time-commit-score) | Defect-induction risk of one commit | Kamei *et al.*, 2013 |

## Commit frequency

The simplest change-history signal is how many distinct commits have
touched a file. big-code-analysis records it per window as
`commits_long` and `commits_recent`.

The metric's standing as a defect predictor comes from Todd Graves,
Alan Karr, J. S. Marron, and Harvey Siy's 2000 IEEE TSE paper
[*Predicting Fault Incidence Using Software Change
History*](https://www.niss.org/publications/predicting-fault-incidence-using-software-change-history).
Studying a large telephone-switching system, they found that
process measures drawn from the change history predicted fault rates
better than any product metric of the code itself, and that the count
of prior changes was among the strongest. The intuition is direct: a
file nobody touches is a file nobody is breaking.

### Algorithm

A single history walk visits each commit reachable from the analysed
ref (first-parent only by default, the full DAG with `--full-history`)
and attributes it to every file it modifies. Merge commits are
excluded unless `--include-merges` is passed, and renames are followed
by default so a file's history survives a move. The walk counts
*distinct commits* per file per window, not diffs or hunks, so a
commit that touches a file once and a commit that touches it in three
hunks both count as one.

### How to read it

Commit frequency is a rate, so it is only meaningful relative to a
baseline: this file versus its siblings, or this file this quarter
versus last. A consistently high count flags a file that is either
under active feature development or structurally unstable; pairing it
with [code churn](#code-churn) tells the two apart, since heavy churn
on few commits is a different story from light churn on many.

## Code churn

**Code churn** is the volume of change: the sum of added and deleted
lines touching a file, recorded per window as `churn_long` and
`churn_recent`.

Churn's defect-prediction pedigree is Nachiappan Nagappan and Thomas
Ball's 2005 ICSE paper [*Use of Relative Code Churn Measures to
Predict System Defect
Density*](https://www.microsoft.com/en-us/research/publication/use-of-relative-code-churn-measures-to-predict-system-defect-density/).
Their key finding, validated on Windows Server 2003, is that
*absolute* churn is a poor predictor on its own, but churn *relative*
to file size and to the temporal spread of the changes is highly
predictive of defect density. big-code-analysis keeps the raw
windowed churn as a signal and lets the composite score combine it
with size and recency, rather than reporting a single absolute number
as a verdict.

### Algorithm

For every in-window commit touching a file, the walk adds that diff's
added-plus-deleted line count to the file's churn total for the
window. Because added and deleted lines are summed, a one-line edit
counts as two (one deletion, one addition) — churn measures *activity*,
not net growth.

### How to read it

Churn is the natural numerator for a density ratio. Churn per commit
distinguishes steady editing from a few large rewrites; churn against
file size recovers Nagappan and Ball's relative measure. A file whose
recent churn is high relative to its long-window churn is changing
faster now than its history would predict — exactly the
[burst](#burst) signal below.

## Burst

**Burst** is the share of a file's activity concentrated in the recent
window: `commits_recent / commits_long`, reported as a ratio in
`[0, 1]`. A value near `1` means almost all of the file's commits are
recent; a value near `0` means the file was active long ago and has
since gone quiet.

The signal follows directly from the recency principle that runs
through the change-history literature — Graves *et al.* found that
recent changes weigh more heavily on fault incidence than old ones,
and the just-in-time defect-prediction line (see the
[JIT score](#just-in-time-commit-score)) is built on the same
observation. A file whose change history is *front-loaded into the
present* is in active flux, and active flux is when defects enter.

### How to read it

Burst is a tie-breaker, not a standalone alarm. A high burst on a file
with little total history is a young, fast-moving file; a high burst
on a file with deep history is an old file that has suddenly woken up,
which is often the more interesting case. Read it alongside
[age](#age-and-last-modified) to tell the two apart.

## Authorship and ownership

Two related signals describe *who* changes a file. **Author counts**
(`authors_long`, `authors_recent`) count the distinct people who
touched it in each window. **Ownership** (`ownership_top_share`, a
ratio in `[0, 1]`) is the fraction of edits attributable to the single
most active author; a low share means diffuse ownership, a high share
means one person dominates.

Both trace to empirical work on ownership and security. Christian Bird
and colleagues' 2011 FSE paper [*Don't Touch My Code! Examining the
Effects of Ownership on Software
Quality*](https://www.microsoft.com/en-us/research/publication/dont-touch-my-code-examining-the-effects-of-ownership-on-software-quality/)
found, on Windows Vista and 7, that diffuse ownership — many
low-expertise contributors, a low top-owner share — predicts both
pre-release faults and post-release failures. On the security side,
Andrew Meneely and Laurie Williams' 2009 CCS paper [*Secure Open
Source Collaboration: An Empirical Study of Linus'
Law*](https://dl.acm.org/doi/10.1145/1653662.1653717) reported that
Red Hat Enterprise Linux 4 files touched by nine or more developers
were roughly sixteen times more likely to harbour a vulnerability.

### Algorithm

Author identities are canonicalised through the repository `.mailmap`
and counted by lowercased email, so a contributor who commits under
two addresses counts once. `Co-authored-by:` trailers add
participants. Bot identities (`dependabot[bot]`, `renovate[bot]`,
`github-actions[bot]`, and similar) are excluded by default so
automated churn does not inflate the count. `ownership_top_share` is
the top author's edit count over the file's total edits in the window.

### How to read it

A rising author count on a long-lived file is a knowledge-diffusion
signal: more people are now obliged to understand it. The
[composite risk score](#composite-risk-score) folds this in two ways —
it scales the author factor by an ownership-*dilution* term
`(1 - ownership_top_share)`, so the same head-count counts for more
when ownership is spread thin, and it adds categorical bumps at the
six- and nine-developer marks that encode Meneely and Williams' RHEL4
thresholds. For knowledge concentration across a *set* of files rather
than within one, see the [bus factor](#bus-factor).

## Fix, security, and revert commits

big-code-analysis classifies each long-window commit touching a file
by the intent of its message and keeps three counts: `bug_fix_commits`
(messages matching a bug-fix keyword), `security_fix_commits`
(messages matching `CVE-####`, `security`, `vuln`, `exploit`,
`sanitize`, and similar), and `revert_commits` (subjects that are a
revert or rollback).

Reading a commit's purpose from its message is the technique
introduced by Jacek Śliwerski, Thomas Zimmermann, and Andreas Zeller's
2005 MSR paper [*When Do Changes Induce
Fixes?*](https://dl.acm.org/doi/10.1145/1083142.1083147), the work now
universally abbreviated *SZZ*. The premise is that a file's history of
*corrective* change is itself predictive: a file that has needed many
fixes is a file likely to need more. A security fix is a sharper
signal than an ordinary bug fix, and a revert marks a change that had
to be undone — a localized admission that something went wrong there.

### Algorithm

Classification is keyword-based on the commit subject and body (see
`src/vcs/classify.rs`). It is deliberately simple and language- and
tracker-agnostic: there is no issue-tracker linkage and no blame
back-tracing to the fix-inducing commit, so this is the lightweight
message-classification half of SZZ, not the full algorithm. The counts
are kept over the long window only.

### How to read it

A high `bug_fix_commits` count is a file that keeps needing repair.
The composite score feeds all three through a single log-scaled term
with security fixes double-weighted, so a file with a history of
security fixes ranks above an otherwise-identical file with only
ordinary bug fixes. Because the classifier reads messages, its
accuracy tracks your project's commit hygiene: a repository with terse
or templated messages will under-count.

## Age and last-modified

Two timing signals bound a file's history. `age_days` is the number of
days since the file's *first* in-window commit (capped at the long
window); `last_modified_days` is the number of days since its *most
recent* in-window commit.

These anchor the recency reasoning the rest of the family depends on.
A small `age_days` marks a *new* file, and newly added code carries
elevated risk — an observation from the just-in-time line and from
Chromium's own defect analysis, where freshly added features were
disproportionately fault-prone. A small `last_modified_days` marks a
file edited recently, which the windows already weight.

### How to read it

`age_days` is most useful as the new-file trigger it feeds into the
[risk score](#composite-risk-score): a file first seen inside the
recent window earns a small additive bonus, reflecting that brand-new
code has had the least chance to be exercised and reviewed.
`last_modified_days` is a staleness read in the other direction: a
high-risk file that has not been touched in months is a different
maintenance proposition from one being edited today.

## Change entropy

**Change entropy** measures how *scattered* the commits touching a
file are. It is reported per window as `change_entropy_long` and
`change_entropy_recent`, in bits.

The metric is Ahmed Hassan's *History Complexity Metric* from the 2009
ICSE paper [*Predicting Faults Using the Complexity of Code
Changes*](https://dl.acm.org/doi/10.1109/ICSE.2009.5070510). The idea
adapts Shannon entropy to commits: for each commit, take the
distribution of its churn across the files it
touched and compute that distribution's entropy in bits. A commit that
touches one file scores `0`; a commit that spreads its churn evenly
across *n* files approaches log₂(*n*). A scattered, cross-cutting
commit is harder to reason about than a focused one. Hassan reported
that this change-process complexity predicts faults better than prior
code-based or change-count models; later work measured file-level
change entropy at a Pearson correlation up to 0.54 with defect counts
on eight Apache projects (see [co-change entropy](#co-change-entropy)).

### Algorithm

For each commit, big-code-analysis computes the Shannon entropy `H` of
its churn distribution across the touched files (see
`src/vcs/entropy.rs`). Each participating file is then credited its
churn share `pᵢ·H` of that commit — Hassan's History Complexity Metric
— and these shares are summed per file per window. A file repeatedly
caught up in diffuse, multi-file changes accumulates high change
entropy; a file that only ever changes in tightly focused commits
stays low.

### How to read it

Change entropy distinguishes a file that changes *as part of large,
sprawling edits* from one that changes in self-contained commits, even
when their raw churn is identical. High change entropy is a smell of
*cross-cutting concern*: edits to this file keep dragging in many
others. It enters the recent-window risk term additively, complementing
rather than restating the churn and commit counts.

## Co-change entropy

**Co-change entropy** measures how wide a file's change *blast radius*
is — how many different files it tends to change alongside. It is
reported per window as `cochange_entropy_long` and
`cochange_entropy_recent`, in bits.

The signal comes from the 2025 study [*Co-Change Graph Entropy: A New
Process Metric for Defect
Prediction*](https://arxiv.org/abs/2504.18511) (arXiv 2504.18511).
Build a weighted graph in which two files share an edge whenever they
change in the same commit, the weight being the number of shared
commits. A
file's co-change entropy is the Shannon entropy of its edge-weight
distribution: low when it always co-changes with the same one partner,
high when its changes ripple out across many different files. The study
found that adding co-change entropy to change entropy improved AUROC in
82.5% of cases over the prior signal set across eight Apache projects.

### Algorithm

The walk records, per commit, which in-scope files changed together and
accumulates the co-change graph's edge weights, then computes each
file's edge-weight entropy per window. Bulk-import commits touching more
than 1000 files are excluded from the co-change graph — its edge count
grows with the square of commit width — though they still contribute
their (linear-cost) change entropy. A computed `0.0` means the file has
no co-change neighbours in the window, which is a real measurement, not
a missing one.

### How to read it

Where change entropy asks "how scattered are the commits that touch
this file?", co-change entropy asks "how many *other* files does
touching this one drag along?". A high value flags a file whose edits
have unpredictable, far-reaching consequences — a coupling hotspot. The
two entropy signals are designed to complement each other, and the
[risk score](#composite-risk-score) `v2` adds both recent-window terms
together.

## Composite risk score

The **risk score** rolls every signal above into a single
per-file number, `risk_score`. It is the headline output of `bca vcs`
and the field its ranked tables sort on. Two formulas are offered,
both versioned together by a single `risk_score_version` (currently
`2`) so downstream consumers can detect a change.

### The weighted formula (default)

The default formula is a log-scaled weighted sum with categorical
multiplicative bumps. Counts are passed through `ln(1 + x)` so that
the difference between 10 and 20 commits matters more than the
difference between 110 and 120, matching how change activity actually
saturates:

```text
base = 0.30 · ln(1 + churn_recent)
     + 0.25 · ln(1 + commits_recent)
     + 0.15 · ln(1 + commits_long)
     + 0.15 · ln(1 + authors_long) · (1 + dilution)
     + 0.10 · ln(1 + bug_fix_commits + 2 · security_fix_commits)
     + 0.05 · ln(1 + churn_long)
     + 0.10 · change_entropy_recent + 0.05 · cochange_entropy_recent
     + ln(1 + sloc)² / 100

risk_score = base · (1 + dev_bonus + new_file_bonus)
```

where `dilution = 1 - ownership_top_share`, the `dev_bonus` is `0.35`
for nine or more long-window authors and `0.15` for six or more, and
the `new_file_bonus` is `0.15` for a file first seen inside the recent
window. The weights are grounded term by term in the literature cited
throughout this chapter: recent churn and commit frequency carry the
most weight (Nagappan & Ball; the JIT line), the author factor is
scaled by ownership dilution (Bird *et al.*) and bumped at the RHEL4
developer-count thresholds (Meneely & Williams), security fixes are
double-weighted, and the two recent-window entropy terms enter
additively (Hassan; arXiv 2504.18511). The full derivation lives in
`src/vcs/score.rs`.

The size term `ln(1 + sloc)² / 100` is a genuine contributor, not the
tiny tie-breaker its position at the end of the sum might suggest:
it reaches about `0.85` at 10k SLOC and exceeds `1.0` past roughly 50k
SLOC, comparable in magnitude to the churn terms. Large files are only
weakly correlated with defects, but the squared-log scaling keeps size
a first-class additive signal rather than letting it dominate.

### The percentile formula

`--risk-formula percentile` is the alternative: each signal is
re-ranked to its percentile within the analysed set, and the per-file
mean of those percentiles becomes the score. This trades the
literature-tuned weights for cross-project robustness — the
prediction literature generally recommends *relative* triggers over
hard absolute thresholds — at the cost of a score that is only
meaningful within a single run's file set.

### How to read it

The score is **ordinal**. Sort a repository's files by it and look at
the top of the list; that is the ranking the score exists to produce.
Do not compare a raw `risk_score` between two repositories, and do not
read its magnitude as a defect probability. To watch a single file
move over time, use [`bca vcs trend`](./commands/vcs.md#historical-trend-over-time),
which re-anchors the walk at each historical point so the series
reflects what the file actually looked like then.

## Hotspot score

The **hotspot score** is the product of a file's complexity and its
recent churn: `hotspot_score = cyclomatic_sum × churn_recent`. It is an
`Option`, present only when an AST complexity figure is computed
alongside the history (for example `bca metrics --metrics
cyclomatic --vcs`), because it needs both halves.

The metric is the central idea of Adam Tornhill's 2015 book [*Your Code
as a Crime
Scene*](https://pragprog.com/titles/atcrime/your-code-as-a-crime-scene/)
and the CodeScene tooling built on it. The argument is that complexity
*on its own* is cheap to ignore — a complex file nobody touches costs
nothing — and churn on its own is cheap too. The danger is their
intersection: code that is both complicated *and* changing often is
where defects concentrate and where developer effort is repeatedly
spent. Tornhill observes that a small fraction of a codebase typically
accounts for a large majority of its change activity, and the hotspot
score is built to find that fraction.

### How to read it

Like the risk score, the hotspot product is ordinal: rank files by it,
do not read the magnitude. The CLI uses the file-level cyclomatic sum
as the complexity axis by convention, but any AST complexity figure
serves. The score's value is its *prioritisation*: of all the complex
files, it surfaces the ones actively being edited, which are both the
likeliest to break and the cheapest to refactor while they are already
open on someone's screen.

## Bus factor

Where `ownership_top_share` measures knowledge concentration *within* a
single file, the **bus factor** (also called the *truck factor*)
measures it *across a set of files*: the minimum number of developers
whose departure would leave more than half of a directory's files
without a knowledgeable maintainer. The broader concept has a
[Wikipedia summary](https://en.wikipedia.org/wiki/Bus_factor);
big-code-analysis emits it as a `vcs_aggregate` object covering the
whole repository, each top-level directory, and each of its immediate
subdirectories.

The estimation method is Guilherme Avelino, Leonardo Passos, Andre
Hora, and Marco Tulio Valente's 2016 ICPC paper [*A Novel Approach for
Estimating Truck Factors*](https://arxiv.org/abs/1604.06766). Each
developer's authorship of each file is scored with their
*Degree-of-Authorship* heuristic:

```text
DoA(d, f) = 3.293 + 1.098 · FA + 0.164 · DL − 0.321 · ln(1 + AC)
```

where `FA` is first authorship (`1` if developer `d` created file `f`),
`DL` is `d`'s number of deliveries (changes) to `f`, and `AC` is the
changes made by *other* developers. A developer is an **author** of a
file when their DoA, normalised by the file's maximum, clears `0.75`
(the paper's threshold).

### Algorithm

The truck factor is a greedy removal (see `src/vcs/bus_factor.rs`):
repeatedly drop the developer who authors the most still-covered files,
stopping once more than `--bus-factor-threshold` (default `0.5`, per
Avelino) of the files are orphaned, and report how many developers were
removed. The aggregate covers every in-scope file in one walk, so
`by_directory` entries are computed over all files recursively beneath
each directory.

### How to read it

A bus factor of `1` means losing one person orphans the set — common,
and correct, for a repository of mostly single-author files. Treat the
number as a planning signal, not a guarantee: it is a heuristic over
*observed* authorship within the long window, so "first authorship"
means the earliest commit seen in that window, not necessarily a file's
true creation. Use it to find the directories where knowledge is
dangerously concentrated and spread review accordingly.

## Just-in-time commit score

Everything above ranks *files* at a point in time. The
**just-in-time (JIT) score** instead scores a single *commit* for its
defect-induction risk — the unit a continuous-integration gate
actually reviews at check-in. It is produced by `bca vcs commit
<commit>` and reported as an ordinal `risk_score` with a per-group
`contributions` breakdown.

The feature groups and their signs are taken from the just-in-time
defect-prediction literature, beginning with Yasutaka Kamei and
colleagues' 2013 IEEE TSE paper [*A Large-Scale Empirical Study of
Just-in-Time Quality
Assurance*](https://doi.org/10.1109/TSE.2013.2386) and confirmed by the
open replications [Commit
Guru](https://doi.org/10.1145/2786805.2803183) (FSE 2015) and Shane
McIntosh and Yasutaka Kamei's [*Are Fix-Inducing Changes a Moving
Target?*](https://doi.org/10.1109/TSE.2017.2693980) (IEEE TSE 2018).
big-code-analysis implements a static, rule-based scorer rather than a
trained model, so nothing drifts as the project ages.

### Algorithm

Five feature groups move the score, each scored against the commit's
first parent (see `src/vcs/jit.rs`):

| Group | Features | Direction |
|-------|----------|-----------|
| **Size** | lines added / deleted, files touched, diff hunks | larger ⇒ riskier |
| **Diffusion** | distinct subsystems and directories, within-commit change entropy | more scattered ⇒ riskier |
| **History** | the touched files' priors — prior changes, distinct authors, fix counts, and their composite risk — measured *before* the commit | turbulent history ⇒ riskier |
| **Experience** | the author's prior commit count (long and recent) | more experience ⇒ **less** risky (this group subtracts) |
| **Purpose** | fix / security-fix / revert classification of the message | fixes add, reverts dampen |

The `contributions` block reports each group's signed contribution, so
a consumer can see *why* a commit ranked where it did. A merge commit
is flagged and scored against its first parent; a root commit and any
new files carry zero priors by construction, so the score then leans on
size and author experience, exactly as the literature prescribes for
changes with no file history. A bare `git diff` can also be scored
(`bca vcs commit --diff`), but only the size and diffusion groups are
computable from a diff, so that path emits a deliberately partial
`partial_risk_score`.

### How to read it

Like the file-level score, the JIT score is **ordinal**: rank commits
by it, or compare a commit against the repository's own commit-score
distribution, but do not read the magnitude as a probability. Its
intended use is a check-in gate — `bca vcs commit HEAD --fail-above
<N>` exits non-zero when a commit scores at or above a threshold — with
the threshold calibrated against your own history rather than treated
as an absolute. Any change to the formula bumps a `jit_score_version`
that is independent of the file-level `risk_score_version`.

---

## Where to go next

- The [Commands → Change-history (VCS) metrics](./commands/vcs.md)
  page documents how to invoke `bca vcs`, configure the windows,
  choose an output format, and use the persistent history cache.
- The [Supported Code Metrics](./metrics.md) chapter covers the
  AST-derived metrics these change-history signals complement — the
  [hotspot score](#hotspot-score) in particular combines the two
  families.
- The [Python Bindings → Change-history (VCS)
  metrics](./python/vcs.md) page shows the same family through the
  `big_code_analysis.vcs` module.
