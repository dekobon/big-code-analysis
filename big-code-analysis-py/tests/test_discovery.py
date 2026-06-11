"""Tests for the discovery / lookup entry points.

Covers ``language_for_extension`` (#682, the filesystem-free extension →
language lookup and the ``language_for_file(read=False)`` variant) and
``analyze_paths`` (#658, the directory-walk entry point that mirrors the
CLI walker and returns the batch shape).
"""

from __future__ import annotations

from pathlib import Path

import big_code_analysis as bca
import pytest
from big_code_analysis import FuncSpaceDict

# ── #682: filesystem-free extension lookup ──────────────────────────


def test_language_for_extension_bare_and_dotted() -> None:
    """Accepts both ``"tsx"`` and ``".tsx"`` (the dot is normalised)."""
    assert bca.language_for_extension("tsx") == "tsx"
    assert bca.language_for_extension(".tsx") == "tsx"
    assert bca.language_for_extension("py") == "python"
    assert bca.language_for_extension(".py") == "python"


def test_language_for_extension_is_case_insensitive() -> None:
    assert bca.language_for_extension("PY") == "python"
    assert bca.language_for_extension(".RS") == "rust"


def test_language_for_extension_unknown_returns_none() -> None:
    """Unknown extension → ``None``, never a raise (a pure table lookup)."""
    assert bca.language_for_extension("xyz") is None
    assert bca.language_for_extension("") is None


def test_language_for_extension_returns_lang_enum_member() -> None:
    """The facade lifts the slug into the ``Lang`` StrEnum (#682)."""
    result = bca.language_for_extension("py")
    assert isinstance(result, bca.Lang)
    assert result == bca.Lang.PYTHON


def test_language_for_file_read_false_never_reads_or_raises(tmp_path: Path) -> None:
    """#682: ``read=False`` resolves by extension alone for a path that does
    not exist — no file read, no ``OSError``."""
    missing = tmp_path / "nope.py"
    assert not missing.exists()
    assert bca.language_for_file(missing, read=False) == "python"
    # An extension-less absent path resolves to None (not a raise).
    assert bca.language_for_file(tmp_path / "noext", read=False) is None


def test_language_for_file_read_true_still_sniffs_and_raises(tmp_path: Path) -> None:
    """#682: ``read=True`` (the default) keeps the content-sniffing,
    raising contract — a missing file is an ``OSError``."""
    missing = tmp_path / "nope.py"
    with pytest.raises(FileNotFoundError):
        bca.language_for_file(missing)
    with pytest.raises(FileNotFoundError):
        bca.language_for_file(missing, read=True)


def test_language_for_file_read_false_skips_shebang(tmp_path: Path) -> None:
    """With ``read=False`` an extension-less shebang script resolves to
    ``None`` (the content is never inspected), whereas ``read=True`` sniffs
    the shebang. Pins that the kwarg is load-bearing."""
    script = tmp_path / "install"
    script.write_text("#!/usr/bin/env python\nprint('ok')\n")
    assert bca.language_for_file(script, read=False) is None
    assert bca.language_for_file(script, read=True) == "python"


# ── #658: analyze_paths directory walk ──────────────────────────────


def _write(root: Path, rel: str, content: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    return path


def _names(results: list[FuncSpaceDict | bca.AnalysisFailure]) -> set[str]:
    """Repo-relative-ish basenames of the analysed (dict) results."""
    return {
        Path(name).name
        for r in results
        if not isinstance(r, bca.AnalysisFailure) and (name := r["name"]) is not None
    }


def test_analyze_paths_walks_a_directory(tmp_path: Path) -> None:
    """#658: pointing at a directory analyses every source file under it,
    returning the batch shape."""
    _write(tmp_path, "src/a.rs", "fn a() {}\n")
    _write(tmp_path, "src/b.py", "def b():\n    return 1\n")
    results = bca.analyze_paths(tmp_path)
    assert _names(results) == {"a.rs", "b.py"}


def test_analyze_paths_honours_gitignore(tmp_path: Path) -> None:
    """#658: the walk reuses the CLI's gitignore-aware walker — an ignored
    file is not analysed."""
    _write(tmp_path, "keep.rs", "fn keep() {}\n")
    _write(tmp_path, "ignored.rs", "fn ignored() {}\n")
    (tmp_path / ".gitignore").write_text("ignored.rs\n")
    results = bca.analyze_paths(tmp_path)
    assert _names(results) == {"keep.rs"}


def test_analyze_paths_respect_gitignore_false(tmp_path: Path) -> None:
    """#658: ``respect_gitignore=False`` opts back into walking ignored
    files."""
    _write(tmp_path, "keep.rs", "fn keep() {}\n")
    _write(tmp_path, "ignored.rs", "fn other() {}\n")
    (tmp_path / ".gitignore").write_text("ignored.rs\n")
    results = bca.analyze_paths(tmp_path, respect_gitignore=False)
    assert _names(results) == {"keep.rs", "ignored.rs"}


def test_analyze_paths_skips_hidden_even_without_gitignore(tmp_path: Path) -> None:
    """Hidden (dot-prefixed) entries are skipped unconditionally, matching
    the CLI walker's `.hidden(true)`. `respect_gitignore=False` opts out of
    gitignore handling but must NOT start walking dotfiles / `.git/` /
    `.venv/` that the CLI never sees — the two surfaces stay in parity."""
    _write(tmp_path, "keep.rs", "fn keep() {}\n")
    _write(tmp_path, ".hidden.rs", "fn hidden() {}\n")
    _write(tmp_path, ".cache/buried.rs", "fn buried() {}\n")
    results = bca.analyze_paths(tmp_path, respect_gitignore=False)
    assert _names(results) == {"keep.rs"}


def test_analyze_paths_include_exclude_globs(tmp_path: Path) -> None:
    """#658: include / exclude globs filter the walk (root-relative)."""
    _write(tmp_path, "a.rs", "fn a() {}\n")
    _write(tmp_path, "b.py", "def b():\n    return 1\n")
    only_rs = bca.analyze_paths(tmp_path, include="*.rs")
    assert _names(only_rs) == {"a.rs"}
    no_py = bca.analyze_paths(tmp_path, exclude="*.py")
    assert _names(no_py) == {"a.rs"}


def test_analyze_paths_multiple_seeds(tmp_path: Path) -> None:
    """#658: multiple positional seeds are each walked (file or dir)."""
    d1 = tmp_path / "d1"
    d2 = tmp_path / "d2"
    _write(d1, "a.rs", "fn a() {}\n")
    loose = _write(d2, "loose.py", "def x():\n    return 2\n")
    results = bca.analyze_paths(d1, loose)
    assert _names(results) == {"a.rs", "loose.py"}


def test_analyze_paths_skips_generated_by_default(tmp_path: Path) -> None:
    """#658: per-file analysis still applies the generated-file filter, so a
    generated file yields no element under the default ``skip_generated``."""
    _write(tmp_path, "real.rs", "fn real() {}\n")
    _write(tmp_path, "gen.rs", "// @generated DO NOT EDIT\nfn g() {}\n")
    results = bca.analyze_paths(tmp_path)
    assert _names(results) == {"real.rs"}


def test_analyze_paths_failure_is_a_failure_element_not_raise(
    tmp_path: Path,
) -> None:
    """#658: a per-file failure surfaces as an ``AnalysisFailure`` element,
    not a raise — the same never-raise batch contract."""
    # An unknown-language file (unknown extension, no shebang) discovered by
    # an explicit include glob becomes an AnalysisFailure, not a raise.
    _write(tmp_path, "weird.unknownext", "noise\n")
    results = bca.analyze_paths(tmp_path, include="*.unknownext")
    assert len(results) == 1
    assert isinstance(results[0], bca.AnalysisFailure)
    assert results[0].error_kind == "UnsupportedLanguage"


def test_analyze_paths_bad_metric_raises_before_walk(tmp_path: Path) -> None:
    """#658: ``metrics=`` validation runs before the walk (mirrors
    analyze_batch's #268 ordering)."""
    _write(tmp_path, "a.rs", "fn a() {}\n")
    with pytest.raises(ValueError, match="unknown metric"):
        bca.analyze_paths(tmp_path, metrics=["bogus"])


def test_analyze_paths_bad_glob_raises(tmp_path: Path) -> None:
    """#658: a malformed include / exclude glob fails fast with the pattern
    named."""
    _write(tmp_path, "a.rs", "fn a() {}\n")
    with pytest.raises(ValueError, match="glob"):
        bca.analyze_paths(tmp_path, include="[")
