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
