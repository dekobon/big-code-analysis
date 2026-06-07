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
