"""Change-history (VCS) surface for the big-code-analysis bindings.

This is the public facade over the compiled ``_native.vcs`` submodule
(issue #612). The flat ``vcs_metrics`` / ``vcs_trend`` / ``vcs_jit``
functions of 1.x were namespaced here as ``rank`` / ``trend`` / ``commit``
/ ``score_diff`` plus a shared :class:`Options` object — the entry-point
names mirror the ``bca vcs`` CLI subcommands (``bca vcs`` → ``vcs.rank``,
``vcs trend`` → ``vcs.trend``, ``vcs commit`` → ``vcs.commit``,
``vcs commit --diff`` → ``vcs.score_diff``).

Usage::

    from big_code_analysis import vcs

    report = vcs.rank("path/to/repo", top=20)
    trend = vcs.trend("path/to/repo", points=6)
    commit = vcs.commit("path/to/repo", commit="HEAD")
    diff = vcs.score_diff(unified_diff_text)

The shared :class:`Options` object carries every knob the three
repository-walking entry points have in common (windows, risk formula, bot
/ merge / rename / history toggles, the ``as_of`` snapshot pin, the
file-type scope, the bus-factor threshold), so they no longer repeat a
15-parameter signature each. Each function keeps only its genuinely
distinct parameters (``top``; ``points`` / ``span`` / ``top_deltas``;
``no_cache`` / ``cache_dir``). The static types live in the companion
``vcs.pyi`` stub; this module is the thin runtime re-export of the native
submodule, mirroring how ``__init__`` re-exports the top-level surface.
"""

# The public types live in the companion ``vcs.pyi`` stub; this module is
# the thin runtime re-export of the native ``_native.vcs`` submodule, which
# PyO3 does not stub, so its members are untyped here. Suppress the
# corresponding strict-pyright "unknown type" diagnostics for the
# re-export block — ``vcs.pyi`` is the type authority consumers see.
# pyright: reportUnknownVariableType=false, reportUnknownMemberType=false
# pyright: reportAttributeAccessIssue=false
from __future__ import annotations

from ._native import vcs as _vcs

Options = _vcs.Options
rank = _vcs.rank
trend = _vcs.trend
commit = _vcs.commit
score_diff = _vcs.score_diff

__all__ = [
    "Options",
    "commit",
    "rank",
    "score_diff",
    "trend",
]
