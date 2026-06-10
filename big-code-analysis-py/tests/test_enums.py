"""Tests for the generated ``Lang`` / ``MetricName`` string enums (#542).

The enums are generated from the same upstream ``LANG`` / ``Metric``
tables the CLI and JSON output use, so their values must equal the
canonical slugs verbatim and round-trip through the typed entry points.
"""

from __future__ import annotations

from enum import StrEnum
from pathlib import Path
from typing import assert_type

import big_code_analysis as bca
import pytest
from big_code_analysis import Lang, MetricName


def test_lang_is_a_str_enum() -> None:
    assert issubclass(Lang, StrEnum)
    assert issubclass(MetricName, StrEnum)


def _eq(member: Lang | MetricName, slug: str) -> bool:
    # Indirect the right operand through a `str` parameter so the
    # comparison is StrEnum-vs-str (the real consumer pattern) rather
    # than StrEnum-vs-string-literal. mypy flags the literal form as a
    # `comparison-overlap` false positive — it does not model
    # ``StrEnum.__eq__`` against a literal — but the str-typed form is
    # exactly how callers compare in practice and is what we contract.
    return member == slug


def test_lang_members_equal_their_slug() -> None:
    # The whole point of depending on #540: the StrEnum value is the
    # canonical CLI/JSON slug, and a StrEnum member compares equal to
    # that string at runtime (``Lang.CPP == "cpp"`` is True).
    assert _eq(Lang.CPP, "cpp")
    assert _eq(Lang.CSHARP, "csharp")
    assert _eq(Lang.TSX, "tsx")
    assert _eq(Lang.TYPESCRIPT, "typescript")
    assert _eq(Lang.JAVASCRIPT, "javascript")
    assert _eq(Lang.MOZJS, "mozjs")
    assert _eq(Lang.PYTHON, "python")
    assert _eq(Lang.RUST, "rust")


def test_lang_values_exactly_match_supported_languages() -> None:
    # supported_languages() now returns Lang members; their string
    # values must be exactly the slug set, in declaration order.
    members = bca.supported_languages()
    assert all(isinstance(m, Lang) for m in members)
    assert [str(m) for m in members] == [m.value for m in members]
    # Every member is reachable through the enum, and the public set
    # equals the enum's full membership (no public slug is missing
    # from Lang and no Lang member is unsupported at runtime).
    assert set(members) == set(Lang)


def test_language_for_file_returns_lang_member(tmp_path: Path) -> None:
    # The facade lifts the native slug into the Lang enum (#625), so a
    # recognised file yields a Lang member that still equals its slug.
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn main() {}\n")
    resolved = bca.language_for_file(src)
    # Static guarantee that the facade is typed as the enum, not str
    # (#625); mypy/pyright fail here if the annotation regresses.
    assert_type(resolved, "Lang | None")
    assert isinstance(resolved, Lang)
    assert resolved is Lang.RUST
    assert _eq(resolved, "rust")


def test_language_for_file_returns_none_for_unknown(tmp_path: Path) -> None:
    # Unknown extensions still resolve to None, not a Lang member.
    bogus = tmp_path / "foo.unknownext"
    bogus.write_text("noise")
    assert bca.language_for_file(bogus) is None


def test_language_extensions_accepts_lang_member() -> None:
    # The facade widens the parameter to ``str | Lang`` (#625); passing
    # a Lang member must match the plain-slug lookup exactly.
    assert bca.language_extensions(Lang.RUST) == bca.language_extensions("rust")


def test_lang_round_trips_through_constructor() -> None:
    for member in Lang:
        assert Lang(member.value) is member
        # The slug string also constructs the member.
        assert Lang(str(member)) is member


def test_lang_unknown_slug_raises_value_error() -> None:
    with pytest.raises(ValueError, match="klingon"):
        Lang("klingon")


def test_metric_name_values_match_metric_names() -> None:
    # MetricName members are exactly bca.METRIC_NAMES, and the typed
    # constant is itself a tuple of MetricName.
    assert all(isinstance(m, MetricName) for m in bca.METRIC_NAMES)
    assert tuple(m.value for m in bca.METRIC_NAMES) == tuple(m.value for m in MetricName)
    assert set(MetricName) == set(bca.METRIC_NAMES)
    assert _eq(MetricName.COGNITIVE, "cognitive")
    assert _eq(MetricName.HALSTEAD, "halstead")


def test_metric_name_is_usable_as_metrics_selector(tmp_path: Path) -> None:
    # A MetricName member is str-compatible, so it works directly as a
    # `metrics=` selector without a manual `.value`.
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn main() { if true {} }\n")
    result = bca.analyze(src, metrics=[MetricName.CYCLOMATIC])
    assert result is not None
    spaces = result["spaces"]
    assert isinstance(spaces, list)
    assert "cyclomatic" in spaces[0]["metrics"]
