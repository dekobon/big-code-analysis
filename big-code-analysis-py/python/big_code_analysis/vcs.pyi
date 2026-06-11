"""Type stubs for the public ``big_code_analysis.vcs`` facade (issue #612).

The runtime ``vcs.py`` thinly re-exports the native ``_native.vcs``
submodule; this stub carries the full typed signatures (PyO3 does not
generate stubs) so ``mypy --strict`` / ``pyright`` see ``rank`` / ``trend``
/ ``commit`` / ``score_diff`` and the shared :class:`Options` object with
precise return types.
"""

from __future__ import annotations

import os
from collections.abc import Sequence
from datetime import datetime

from ._types import (
    JitCommitReportDict,
    JitDiffReportDict,
    VcsReportDict,
    VcsTrendDict,
)

class Options:
    """Shared change-history options for :func:`rank`, :func:`trend`, and
    :func:`commit`.

    Every field is optional and defaults match the corresponding ``bca
    vcs`` CLI flags, so ``Options()`` reproduces the default ranking. The
    distinct ``vcs.*`` functions accept the same object, so one ``Options``
    can drive a rank + trend pass without restating the common knobs.

    Parameters
    ----------
    long_window, recent_window
        History window lengths (``"12mo"`` / ``"2y"`` / ``"8w"`` /
        ``"90d"`` or ISO 8601 ``"P1Y"``).
    reference
        Git revision the walk is anchored at (default ``HEAD``).
    risk_formula
        ``"weighted"`` (default) or ``"percentile"``.
    file_types
        ``"metrics"`` (default), ``"all"``, a comma-separated extension
        allow-list (``"rs,py"``), or a sequence of extensions.
    full_history, include_merges, follow_renames, exclude_bots
        History-walk toggles.
    bot_pattern
        Override the bot-author regular expression.
    as_of
        Pin the reference "now" for reproducible snapshots; a ``datetime``
        or a string (RFC 3339 / ``@unix`` / git date).
    emit_author_details
        Include SHA-256-hashed canonical author identities.
    include_deleted
        Rank files deleted at the reference too.
    bus_factor_threshold
        Coverage/abandonment fraction for the bus-factor flag (default
        ``0.5``).
    """

    def __init__(
        self,
        *,
        long_window: str | None = None,
        recent_window: str | None = None,
        reference: str | None = None,
        risk_formula: str | None = None,
        file_types: Sequence[str] | str | None = None,
        full_history: bool = False,
        include_merges: bool = False,
        follow_renames: bool = True,
        exclude_bots: bool = True,
        bot_pattern: str | None = None,
        as_of: datetime | str | None = None,
        emit_author_details: bool = False,
        include_deleted: bool = False,
        bus_factor_threshold: float | None = None,
    ) -> None: ...

def rank(
    repo_path: str | os.PathLike[str],
    /,
    *,
    options: Options | None = None,
    top: int | None = None,
    no_cache: bool = False,
    cache_dir: str | os.PathLike[str] | None = None,
) -> VcsReportDict:
    """Rank the files in a git repository by change-history risk.

    The programmatic analogue of ``bca vcs`` (issue #328). Returns a dict
    with the window lengths, version stamps, a ``truncated_shallow_clone``
    flag, a ``vcs_aggregate`` bus-factor object (issue #332), and a
    ``files`` list ranked by descending ``vcs.risk_score``. ``top`` caps
    how many files the ranking keeps (``0`` / ``None`` keeps all). The
    persistent cache (issue #334) is on by default; ``no_cache`` /
    ``cache_dir`` control it. The GIL is released across the walk (#620).

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not inside a git working tree.
    VcsError
        For a malformed window / timestamp / formula / file-type scope /
        bus-factor threshold.
    VcsEnvironmentError
        When walking history, diffing, or cache I/O fails.
    """

def trend(
    repo_path: str | os.PathLike[str],
    /,
    *,
    options: Options | None = None,
    points: int = 12,
    span: str | None = None,
    top: int | None = None,
    top_deltas: int | None = None,
) -> VcsTrendDict:
    """Sample change-history metrics over time as a per-file trend.

    The programmatic analogue of ``bca vcs trend`` (issue #333).
    ``points`` (>= 2) samples cover ``span`` (default ``12mo``), ending at
    ``options.as_of``. Returns a dict with ``as_of_points``, a ``files``
    map aligned to it (``None`` marks an absent point), and an
    ``improved`` / ``regressed`` ``deltas`` summary. ``top`` caps files
    kept; ``top_deltas`` trims each delta list. Each point re-anchors at
    that moment's mainline tip. The GIL is released across the walks
    (#620).

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not inside a git working tree.
    VcsError
        For a malformed option, or a point count below 2 or above the
        supported maximum.
    VcsEnvironmentError
        When walking history, diffing, or cache I/O fails.
    """

def commit(
    repo_path: str | os.PathLike[str],
    /,
    *,
    commit: str = "HEAD",
    options: Options | None = None,
) -> JitCommitReportDict:
    """Score a single commit for just-in-time (commit-level) risk.

    The programmatic analogue of ``bca vcs commit`` (issue #331).
    ``commit`` is any git revision spelling (default ``"HEAD"``), scored
    against its first parent; only the window / history / rename / as-of
    knobs of ``options`` apply. Returns a dict with ``source == "commit"``,
    the size / diffusion / history / experience features, their per-group
    contributions, the ordinal ``risk_score``, and the ``commit`` block.
    To score an arbitrary diff use :func:`score_diff` (issue #667 split the
    two modes so neither silently discards the other's input).

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not a git working tree.
    InvalidRevisionError
        When ``commit`` cannot be resolved to a revision.
    VcsError
        For a malformed window / timestamp.
    """

def score_diff(diff: str, /) -> JitDiffReportDict:
    """Score an arbitrary unified diff for partial just-in-time risk.

    The programmatic analogue of ``bca vcs jit --diff`` (issue #580). A
    bare diff carries no author / parent / history, so the returned dict
    has ``source == "diff"``, a ``partial_risk_score`` **not comparable**
    to a commit ``risk_score``, and no history / experience / purpose
    groups (absent, not zero). The diff half of the former dual-mode
    ``vcs_jit`` (issue #667); the commit half is :func:`commit`.

    Raises
    ------
    InvalidDiffError
        When ``diff`` cannot be parsed as a unified diff.
    """

__all__ = [
    "Options",
    "commit",
    "rank",
    "score_diff",
    "trend",
]
