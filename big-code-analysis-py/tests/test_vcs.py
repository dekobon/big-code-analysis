"""Tests for the change-history (VCS) bindings (issue #328).

Each builds a throwaway git repo with a fixed author and a commit dated
relative to wall-clock now (both ``vcs_metrics`` and ``analyze(vcs=True)``
default to wall-clock time), so per-window counts are deterministic.
"""

from __future__ import annotations

import os
import subprocess
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, cast

import big_code_analysis as bca
import pytest
from big_code_analysis import FuncSpaceDict


def _build_repo(root: Path) -> Path:
    """Init a repo under *root* with ``work.rs`` committed ~5 days ago."""
    now = int(time.time())
    date = f"@{now - 5 * 86_400} +0000"
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "Ada",
        "GIT_AUTHOR_EMAIL": "ada@example.com",
        "GIT_AUTHOR_DATE": date,
        "GIT_COMMITTER_NAME": "Ada",
        "GIT_COMMITTER_EMAIL": "ada@example.com",
        "GIT_COMMITTER_DATE": date,
    }

    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=root, env=env, check=True)

    git("init", "-q", "-b", "main")
    git("config", "commit.gpgsign", "false")
    (root / "work.rs").write_text("fn a() {}\n")
    git("add", ".")
    git("commit", "-q", "-m", "fix bug in work")
    return root


def test_vcs_metrics_ranks_the_tracked_file(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    report = bca.vcs_metrics(repo)
    assert report["long_window_days"] == 365
    assert report["recent_window_days"] == 90
    files = {f["path"]: f for f in report["files"]}
    assert "work.rs" in files
    assert files["work.rs"]["commits_long"] == 1
    assert files["work.rs"]["bug_fix_commits"] == 1


def test_vcs_metrics_top_limits_results(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    (repo / "other.rs").write_text("fn z() {}\n")
    subprocess.run(["git", "add", "."], cwd=repo, check=True, env={**os.environ})
    subprocess.run(
        ["git", "commit", "-q", "-m", "add other"],
        cwd=repo,
        check=True,
        env={
            **os.environ,
            "GIT_AUTHOR_NAME": "Ada",
            "GIT_AUTHOR_EMAIL": "ada@example.com",
            "GIT_COMMITTER_NAME": "Ada",
            "GIT_COMMITTER_EMAIL": "ada@example.com",
        },
    )
    report = bca.vcs_metrics(repo, top=1)
    assert len(report["files"]) == 1


def test_analyze_vcs_true_attaches_block(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    result = bca.analyze(repo / "work.rs", vcs=True)
    assert result is not None
    vcs = result["metrics"]["vcs"]
    assert vcs["commits_long"] == 1
    # hotspot = cyclomatic_sum x churn_recent; both positive here.
    # `hotspot_score` is NotRequired float | None (#623): present and
    # finite when cyclomatic was computed alongside, as it is here.
    hotspot = vcs["hotspot_score"]
    assert hotspot is not None
    assert hotspot > 0.0


def test_analyze_without_vcs_has_no_block(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    result = bca.analyze(repo / "work.rs")
    assert result is not None
    assert "vcs" not in result["metrics"]


def test_vcs_metrics_outside_repo_raises(tmp_path: Path) -> None:
    # The typed not-a-repo exception (#624) is the variant a caller
    # branches on; it stays catchable as a plain ValueError.
    with pytest.raises(bca.NotARepositoryError, match="version-control") as exc:
        bca.vcs_metrics(tmp_path)
    assert isinstance(exc.value, bca.VcsError)
    assert isinstance(exc.value, ValueError)


def test_vcs_metrics_bad_window_raises(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    # A bad option is a client-input error but not one of the named
    # subclasses, so it surfaces as the VcsError base (#624) — still a
    # ValueError, and NOT a NotARepositoryError.
    with pytest.raises(bca.VcsError, match="time window") as exc:
        bca.vcs_metrics(repo, long_window="nonsense")
    assert isinstance(exc.value, ValueError)
    assert not isinstance(exc.value, bca.NotARepositoryError)


def _build_multifn_repo(root: Path) -> Path:
    """Init a repo whose ``work.rs`` holds two distinct functions.

    Lines are deliberately unique — identical lines trigger a
    non-deterministic ``gix`` blame failure (project memory: gix-blame
    repetitive-content bug), so each function body carries different
    content.
    """
    now = int(time.time())
    date = f"@{now - 5 * 86_400} +0000"
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "Ada",
        "GIT_AUTHOR_EMAIL": "ada@example.com",
        "GIT_AUTHOR_DATE": date,
        "GIT_COMMITTER_NAME": "Ada",
        "GIT_COMMITTER_EMAIL": "ada@example.com",
        "GIT_COMMITTER_DATE": date,
    }

    def git(*args: str) -> None:
        subprocess.run(["git", *args], cwd=root, env=env, check=True)

    git("init", "-q", "-b", "main")
    git("config", "commit.gpgsign", "false")
    # Two functions with branching so each space has a non-trivial
    # cyclomatic sum (a positive hotspot_score). Every line is distinct.
    (root / "work.rs").write_text(
        "fn alpha(value: i32) -> i32 {\n"
        "    if value > 0 {\n"
        "        return value + 1;\n"
        "    }\n"
        "    value - 7\n"
        "}\n"
        "\n"
        "fn beta(flag: bool) -> u8 {\n"
        "    match flag {\n"
        "        true => 200,\n"
        "        false => 13,\n"
        "    }\n"
        "}\n"
    )
    git("add", ".")
    git("commit", "-q", "-m", "add alpha and beta")
    return root


def _func_spaces(result: FuncSpaceDict) -> list[FuncSpaceDict]:
    """Flatten every nested function space (the root file space excluded)."""
    out: list[FuncSpaceDict] = []
    stack: list[FuncSpaceDict] = list(result.get("spaces", []))
    while stack:
        space = stack.pop()
        out.append(space)
        stack.extend(space.get("spaces", []))
    return out


def test_analyze_vcs_per_function_attaches_block_to_each_space(
    tmp_path: Path,
) -> None:
    repo = _build_multifn_repo(tmp_path)
    result = bca.analyze(repo / "work.rs", vcs_per_function=True)
    assert result is not None
    spaces = _func_spaces(result)
    # alpha and beta both parse to their own function space.
    assert len(spaces) == 2
    for space in spaces:
        vcs = space["metrics"]["vcs"]
        # Shape parity with the file-level block (same keys the CLI emits).
        assert vcs["commits_long"] == 1
        assert vcs["churn_recent"] >= 1
        assert "hotspot_score" in vcs

    # Prove genuine per-function attribution, not a broadcast of the
    # file-level stats onto every space. alpha and beta have distinct
    # cyclomatic sums, so their hotspot scores (complexity x churn) must
    # differ; if production were changed to copy the file-level block to
    # each space, every score would be identical and this would fail.
    hotspots = [space["metrics"]["vcs"]["hotspot_score"] for space in spaces]
    assert len(set(hotspots)) > 1, hotspots

    # A per-function churn must also differ from the file-level churn: each
    # function spans only its own lines, so its recent churn is a strict
    # subset of the whole file's. Broadcasting the file-level block would
    # make these equal.
    file_level = bca.analyze(repo / "work.rs", vcs=True)
    assert file_level is not None
    file_churn = file_level["metrics"]["vcs"]["churn_recent"]
    per_fn_churns = [space["metrics"]["vcs"]["churn_recent"] for space in spaces]
    assert any(churn != file_churn for churn in per_fn_churns), (
        per_fn_churns,
        file_churn,
    )


# Fields the per-function and file-level blocks must agree on: the
# output-shape / formula versions and window lengths are repo-config
# constants, and in a single-commit fixture every function shares the
# file's commit/author totals. Churn and the hotspot score deliberately
# differ per function (see the distinctiveness test above), so they are
# excluded from this parity check. Comparing this required-key subset is
# robust to the optional ``hotspot_score`` key dropping out.
_SHARED_VCS_FIELDS = (
    "vcs_schema_version",
    "risk_score_version",
    "long_window_days",
    "recent_window_days",
    "commits_long",
    "authors_long",
)


def test_analyze_vcs_per_function_matches_file_level_block(tmp_path: Path) -> None:
    """The per-function block agrees with the file-level block that
    ``vcs=True`` attaches on the shared required fields — not just key
    presence, but their values.

    This is Python file-level-vs-per-function parity; CLI parity is
    transitive, since both front ends serialize the same ``wire::Vcs``.
    """
    repo = _build_multifn_repo(tmp_path)
    file_level = bca.analyze(repo / "work.rs", vcs=True)
    per_fn = bca.analyze(repo / "work.rs", vcs_per_function=True)
    assert file_level is not None
    assert per_fn is not None
    file_block = file_level["metrics"]["vcs"]
    # Guard against a future field rename silently emptying the subset.
    for field in _SHARED_VCS_FIELDS:
        assert field in file_block, field

    spaces = _func_spaces(per_fn)
    assert len(spaces) == 2
    # The field-name comparison below indexes by a loop variable, which a
    # TypedDict rejects (literal-required keys). Cast to plain mappings for
    # the dynamic-key reads — the VcsDict shape is asserted elsewhere.
    file_block_map = cast("dict[str, Any]", file_block)
    for space in spaces:
        block = cast("dict[str, Any]", space["metrics"]["vcs"])
        # Every required key the file-level block carries is present.
        assert set(file_block_map).issubset(block.keys()), set(file_block_map) - block.keys()
        # Value-level agreement on the fields that must match.
        for field in _SHARED_VCS_FIELDS:
            assert block[field] == file_block_map[field], field


def test_analyze_without_vcs_per_function_has_no_nested_block(
    tmp_path: Path,
) -> None:
    repo = _build_multifn_repo(tmp_path)
    result = bca.analyze(repo / "work.rs")
    assert result is not None
    for space in _func_spaces(result):
        assert "vcs" not in space["metrics"]


def test_analyze_vcs_per_function_outside_repo_degrades(tmp_path: Path) -> None:
    """A file with no enclosing git repo yields no per-function block and
    no error (graceful degradation matching the CLI)."""
    (tmp_path / "loose.rs").write_text(
        "fn one() -> i32 {\n    42\n}\n\nfn two() -> i32 {\n    99\n}\n"
    )
    result = bca.analyze(tmp_path / "loose.rs", vcs_per_function=True)
    assert result is not None
    for space in _func_spaces(result):
        assert "vcs" not in space["metrics"]


def _build_staged_repo(root: Path) -> Path:
    """Init a repo with ``early.rs`` at now-300d and ``late.rs`` at now-100d,
    pinned to a fixed clock so a 3-point / 300d trend is reproducible."""
    fixed_now = 1_700_000_000
    day = 86_400

    def commit(secs: int, message: str) -> None:
        date = f"@{secs} +0000"
        env = {
            **os.environ,
            "GIT_AUTHOR_NAME": "Ada",
            "GIT_AUTHOR_EMAIL": "ada@example.com",
            "GIT_AUTHOR_DATE": date,
            "GIT_COMMITTER_NAME": "Ada",
            "GIT_COMMITTER_EMAIL": "ada@example.com",
            "GIT_COMMITTER_DATE": date,
        }
        subprocess.run(["git", "add", "."], cwd=root, check=True, env=env)
        subprocess.run(
            ["git", "commit", "-q", "--no-verify", "-m", message],
            cwd=root,
            check=True,
            env=env,
        )

    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    subprocess.run(["git", "config", "commit.gpgsign", "false"], cwd=root, check=True)
    (root / "early.rs").write_text("fn a() {}\n")
    commit(fixed_now - 300 * day, "init early")
    (root / "late.rs").write_text("fn b() {}\n")
    commit(fixed_now - 100 * day, "add late")
    return root


def test_vcs_trend_shape(tmp_path: Path) -> None:
    repo = _build_staged_repo(tmp_path)
    trend = bca.vcs_trend(repo, points=3, span="300d", as_of="@1700000000")
    assert trend["trend_schema_version"] == 1
    assert len(trend["as_of_points"]) == 3
    assert trend["as_of_points"][2] == 1_700_000_000
    # late.rs was added at the middle commit, so it is absent (None) at the
    # oldest point and present at the newest.
    late = trend["files"]["late.rs"]
    assert late[0] is None
    assert isinstance(late[2], dict)
    assert late[2]["as_of"] == 1_700_000_000
    assert isinstance(trend["deltas"]["improved"], list)
    assert isinstance(trend["deltas"]["regressed"], list)


def test_vcs_trend_too_few_points_raises(tmp_path: Path) -> None:
    repo = _build_staged_repo(tmp_path)
    with pytest.raises(ValueError, match="at least 2 points"):
        bca.vcs_trend(repo, points=1, span="300d")


def test_vcs_trend_outside_repo_raises(tmp_path: Path) -> None:
    with pytest.raises(bca.NotARepositoryError, match="version-control") as exc:
        bca.vcs_trend(tmp_path, points=3, span="300d")
    assert isinstance(exc.value, ValueError)


def test_vcs_metrics_cache_dir_replays_identically(tmp_path: Path) -> None:
    """A cache hit (issue #334) reproduces the first run's report exactly."""
    repo_dir = tmp_path / "repo"
    repo_dir.mkdir()
    repo = _build_repo(repo_dir)
    cache_dir = tmp_path / "cache"

    first = bca.vcs_metrics(repo, as_of="@1700000000", cache_dir=str(cache_dir))
    # An entry was persisted under the cache directory.
    assert any(cache_dir.rglob("*.json"))
    second = bca.vcs_metrics(repo, as_of="@1700000000", cache_dir=str(cache_dir))
    assert first == second


def test_vcs_metrics_no_cache_writes_nothing(tmp_path: Path) -> None:
    repo_dir = tmp_path / "repo"
    repo_dir.mkdir()
    repo = _build_repo(repo_dir)
    cache_dir = tmp_path / "cache"
    bca.vcs_metrics(repo, no_cache=True, cache_dir=str(cache_dir))
    assert not any(cache_dir.rglob("*.json"))


def test_vcs_jit_commit_returns_report(tmp_path: Path) -> None:
    """vcs_jit(repo, commit) returns the full commit JIT report (issue
    #331). The commit-mode report carries the score plus every feature
    group, mirroring ``bca vcs jit``."""
    repo = _build_repo(tmp_path)
    report = bca.vcs_jit(repo, commit="HEAD")
    assert report["jit_schema_version"] == 3
    assert report["jit_score_version"] == 1
    # Commit-mode reports self-identify via ``source`` (issue #642).
    assert report["source"] == "commit"
    assert isinstance(report["risk_score"], (int, float))
    assert isinstance(report["commit"]["id"], str)
    assert len(report["commit"]["id"]) == 40
    for group in ("size", "diffusion", "history", "experience"):
        assert isinstance(report["features"][group], dict), group
    # A bug-fix message ("fix bug in work") is classified.
    assert report["commit"]["purpose"]["is_fix"] is True


_SAMPLE_DIFF = (
    "diff --git a/src/a.rs b/src/a.rs\n"
    "--- a/src/a.rs\n"
    "+++ b/src/a.rs\n"
    "@@ -1,1 +1,3 @@\n"
    " keep\n"
    "+added1\n"
    "+added2\n"
    "diff --git a/docs/b.md b/docs/b.md\n"
    "--- a/docs/b.md\n"
    "+++ b/docs/b.md\n"
    "@@ -1,1 +1,2 @@\n"
    " title\n"
    "+body\n"
)


def test_vcs_jit_diff_mode_marks_unavailable_groups() -> None:
    """vcs_jit(diff=...) scores an arbitrary diff (issue #580). Only size and
    diffusion are computable, so the report is marked ``source == "diff"``
    and the unavailable groups are ABSENT (not present as zero) — a consumer
    cannot read a missing group as "low risk"."""
    report = bca.vcs_jit(diff=_SAMPLE_DIFF)
    assert report["source"] == "diff"
    assert isinstance(report["partial_risk_score"], (int, float))
    assert report["size"]["files_touched"] == 2
    assert report["size"]["lines_added"] == 3
    assert report["diffusion"]["subsystems"] == 2  # src + docs
    # The whole point of #580: the unavailable groups have no key at all.
    for absent in ("history", "experience", "purpose", "commit", "risk_score"):
        assert absent not in report, absent


def test_vcs_jit_malformed_diff_raises() -> None:
    with pytest.raises(bca.InvalidDiffError, match="diff") as exc:
        bca.vcs_jit(diff="diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n")
    assert isinstance(exc.value, bca.VcsError)
    assert isinstance(exc.value, ValueError)


def test_vcs_jit_outside_repo_raises(tmp_path: Path) -> None:
    with pytest.raises(bca.NotARepositoryError, match="version-control") as exc:
        bca.vcs_jit(tmp_path, commit="HEAD")
    assert isinstance(exc.value, ValueError)


def test_vcs_jit_unresolvable_commit_raises(tmp_path: Path) -> None:
    """An unresolvable revision surfaces as InvalidRevisionError (#624),
    catchable as VcsError / ValueError, distinct from the not-a-repo and
    diff cases."""
    repo = _build_repo(tmp_path)
    with pytest.raises(bca.InvalidRevisionError, match="resolve") as exc:
        bca.vcs_jit(repo, commit="no-such-rev-xyzzy")
    assert isinstance(exc.value, bca.VcsError)
    assert isinstance(exc.value, ValueError)
    assert not isinstance(exc.value, bca.NotARepositoryError)


def test_vcs_exception_hierarchy() -> None:
    """The VCS exception taxonomy is additive over ValueError (#624):
    every typed class is a VcsError, and VcsError is a ValueError, so a
    single ``except ValueError`` covers all of them."""
    assert issubclass(bca.VcsError, ValueError)
    for cls in (
        bca.NotARepositoryError,
        bca.InvalidRevisionError,
        bca.InvalidDiffError,
        bca.VcsEnvironmentError,
    ):
        assert issubclass(cls, bca.VcsError), cls
        assert issubclass(cls, ValueError), cls


# --- Python-native kwarg widenings (issue #619) -----------------------------
#
# Each widened kwarg is asserted to produce output IDENTICAL to its
# pre-existing string form (a parity test), so the native form is proven a
# pure spelling alternative rather than a separate code path. The string
# form is kept as regression cover that the CLI-ism still works.

# A fixed epoch reused so the as-of parity tests are deterministic.
_FIXED_EPOCH = 1_700_000_000


def test_vcs_metrics_file_types_sequence_matches_comma_string(tmp_path: Path) -> None:
    """``file_types=["rs", "py"]`` ranks the same files as ``"rs,py"``."""
    repo = _build_repo(tmp_path)
    (repo / "note.py").write_text("def a():\n    pass\n")
    subprocess.run(["git", "add", "."], cwd=repo, check=True, env={**os.environ})
    subprocess.run(
        ["git", "commit", "-q", "-m", "add py"],
        cwd=repo,
        check=True,
        env={
            **os.environ,
            "GIT_AUTHOR_NAME": "Ada",
            "GIT_AUTHOR_EMAIL": "ada@example.com",
            "GIT_COMMITTER_NAME": "Ada",
            "GIT_COMMITTER_EMAIL": "ada@example.com",
        },
    )
    from_seq = bca.vcs_metrics(repo, file_types=["rs", "py"])
    from_str = bca.vcs_metrics(repo, file_types="rs,py")
    assert from_seq == from_str
    paths = {f["path"] for f in from_seq["files"]}
    assert {"work.rs", "note.py"} <= paths


def test_vcs_metrics_file_types_keyword_string_still_works(tmp_path: Path) -> None:
    """The ``"metrics"`` / ``"all"`` literal strings keep their meaning."""
    repo = _build_repo(tmp_path)
    metrics_scope = bca.vcs_metrics(repo, file_types="metrics")
    assert any(f["path"] == "work.rs" for f in metrics_scope["files"])


def test_vcs_metrics_file_types_empty_sequence_raises(tmp_path: Path) -> None:
    """A sequence that normalises to no usable extensions raises VcsError,
    exactly as the equivalent empty comma-string does (#624 taxonomy)."""
    repo = _build_repo(tmp_path)
    with pytest.raises(bca.VcsError):
        bca.vcs_metrics(repo, file_types=[])


def test_vcs_metrics_file_types_invalid_type_raises(tmp_path: Path) -> None:
    """A non-str / non-sequence value is rejected up front."""
    repo = _build_repo(tmp_path)
    with pytest.raises((TypeError, ValueError)):
        bca.vcs_metrics(repo, file_types=42)  # type: ignore[arg-type]


def test_vcs_metrics_as_of_datetime_matches_epoch_string(tmp_path: Path) -> None:
    """An aware ``datetime`` pins the same "now" as its ``@unix`` string."""
    repo = _build_repo(tmp_path)
    aware = datetime.fromtimestamp(_FIXED_EPOCH, tz=UTC)
    from_dt = bca.vcs_metrics(repo, as_of=aware)
    from_str = bca.vcs_metrics(repo, as_of=f"@{_FIXED_EPOCH}")
    assert from_dt == from_str


def test_vcs_metrics_as_of_naive_datetime_accepted(tmp_path: Path) -> None:
    """A naive ``datetime`` is accepted (rendered offset-less, the same as
    passing that bare ISO string), and returns a normal report."""
    repo = _build_repo(tmp_path)
    naive = datetime(2023, 11, 14, 22, 13, 20)
    report = bca.vcs_metrics(repo, as_of=naive)
    assert report["long_window_days"] == 365


def test_vcs_metrics_as_of_invalid_type_raises(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    with pytest.raises((TypeError, ValueError)):
        bca.vcs_metrics(repo, as_of=object())  # type: ignore[arg-type]


def test_vcs_metrics_cache_dir_path_matches_str(tmp_path: Path) -> None:
    """``cache_dir=Path(...)`` writes the same cache a ``str`` does."""
    repo_dir = tmp_path / "repo"
    repo_dir.mkdir()
    repo = _build_repo(repo_dir)
    path_cache = tmp_path / "by_path"
    str_cache = tmp_path / "by_str"

    via_path = bca.vcs_metrics(repo, as_of=f"@{_FIXED_EPOCH}", cache_dir=path_cache)
    via_str = bca.vcs_metrics(repo, as_of=f"@{_FIXED_EPOCH}", cache_dir=str(str_cache))
    assert via_path == via_str
    assert any(path_cache.rglob("*.json"))


def test_vcs_trend_as_of_datetime_matches_epoch_string(tmp_path: Path) -> None:
    repo = _build_staged_repo(tmp_path)
    aware = datetime.fromtimestamp(_FIXED_EPOCH, tz=UTC)
    from_dt = bca.vcs_trend(repo, points=3, span="300d", as_of=aware)
    from_str = bca.vcs_trend(repo, points=3, span="300d", as_of=f"@{_FIXED_EPOCH}")
    assert from_dt == from_str


def test_vcs_jit_as_of_datetime_matches_epoch_string(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    aware = datetime.fromtimestamp(_FIXED_EPOCH, tz=UTC)
    from_dt = bca.vcs_jit(repo, commit="HEAD", as_of=aware)
    from_str = bca.vcs_jit(repo, commit="HEAD", as_of=f"@{_FIXED_EPOCH}")
    assert from_dt == from_str
