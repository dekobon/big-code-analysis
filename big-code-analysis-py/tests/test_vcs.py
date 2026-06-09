"""Tests for the change-history (VCS) bindings (issue #328).

Each builds a throwaway git repo with a fixed author and a commit dated
relative to wall-clock now (both ``vcs_metrics`` and ``analyze(vcs=True)``
default to wall-clock time), so per-window counts are deterministic.
"""

from __future__ import annotations

import os
import subprocess
import time
from pathlib import Path
from typing import Any

import big_code_analysis as bca
import pytest


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
    assert vcs["hotspot_score"] > 0.0


def test_analyze_without_vcs_has_no_block(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    result = bca.analyze(repo / "work.rs")
    assert result is not None
    assert "vcs" not in result["metrics"]


def test_vcs_metrics_outside_repo_raises(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="version-control"):
        bca.vcs_metrics(tmp_path)


def test_vcs_metrics_bad_window_raises(tmp_path: Path) -> None:
    repo = _build_repo(tmp_path)
    with pytest.raises(ValueError, match="time window"):
        bca.vcs_metrics(repo, long_window="nonsense")


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


def _func_spaces(result: dict[str, Any]) -> list[dict[str, Any]]:
    """Flatten every nested function space (the root file space excluded)."""
    out: list[dict[str, Any]] = []
    stack = list(result.get("spaces", []))
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
    for space in spaces:
        block = space["metrics"]["vcs"]
        # Every required key the file-level block carries is present.
        assert set(file_block).issubset(block.keys()), set(file_block) - block.keys()
        # Value-level agreement on the fields that must match.
        for field in _SHARED_VCS_FIELDS:
            assert block[field] == file_block[field], field


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
    with pytest.raises(ValueError, match="version-control"):
        bca.vcs_trend(tmp_path, points=3, span="300d")


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
