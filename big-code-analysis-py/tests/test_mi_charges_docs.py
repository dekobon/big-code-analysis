"""The ``mi_charges_docs`` kwarg (#1597) on every analysis entry point.

Every ``mi.*`` variant sizes a space on ``loc.ploc`` (#1607), so the
mode reaches only ``mi.sei``'s comment share: by default documentation
rows leave both sides of the ratio, and ``mi_charges_docs=True`` counts
every comment row over every physical row.
Each of the five entry points carries the kwarg, and the stub (gated by
``make py-stubtest``) only proves the signature, so every one is driven
here to prove the value reaches MI.
"""

from __future__ import annotations

import copy
from pathlib import Path
from typing import Any

import big_code_analysis as bca

DOCUMENTED = (
    "def clamp(value, low, high):\n"
    '    """Return ``value`` held within ``[low, high]``.\n'
    "\n"
    "    The bounds are inclusive and are not checked against each\n"
    "    other, so ``low > high`` returns ``high`` for every input.\n"
    '    """\n'
    "    if value < low:\n"
    "        return low\n"
    "    if value > high:\n"
    "        return high\n"
    "    return value\n"
)


def _mi(result: Any) -> tuple[float, float, float]:
    mi = result["metrics"]["mi"]
    return (mi["original"], mi["sei"], mi["visual_studio"])


def _without_mi(result: Any) -> Any:
    stripped = copy.deepcopy(result)
    stripped["metrics"].pop("mi")
    for space in stripped["spaces"]:
        space["metrics"].pop("mi")
    return stripped


def test_charging_docs_moves_mi_and_nothing_else_through_analyze_source() -> None:
    default = bca.analyze_source(DOCUMENTED, "python")
    explicit = bca.analyze_source(DOCUMENTED, "python", mi_charges_docs=False)
    charged = bca.analyze_source(DOCUMENTED, "python", mi_charges_docs=True)

    assert default == explicit, "False is the default"
    assert default["metrics"]["loc"]["dloc"] == 5, "the fixture is documented"
    # The function space holds the docstring too, so it moves as well:
    # both finalisation paths carry the mode.
    for d, c in [
        (_mi(default), _mi(charged)),
        (_mi(default["spaces"][0]), _mi(charged["spaces"][0])),
    ]:
        # `original` and `visual_studio` size on ploc in both modes.
        assert (d[0], d[2]) == (c[0], c[2])
        # The fixture has no working comment, so the default share is 0
        # and charging the docstring can only raise `sei`.
        assert d[1] == d[0]
        assert c[1] > d[1]
    assert _without_mi(default) == _without_mi(charged), "only mi moves"


def test_every_entry_point_carries_the_kwarg(tmp_path: Path) -> None:
    path = tmp_path / "doc.py"
    path.write_text(DOCUMENTED)
    source = bca.analyze_source(DOCUMENTED, "python", mi_charges_docs=True)
    expected = _mi(source)

    analyzed = bca.analyze(path, mi_charges_docs=True)
    assert analyzed is not None
    assert _mi(analyzed) == expected

    (batch,) = bca.analyze_batch([path], mi_charges_docs=True)
    assert isinstance(batch, dict)
    assert _mi(batch) == expected

    (walked,) = bca.analyze_paths(tmp_path, mi_charges_docs=True)
    assert isinstance(walked, dict)
    assert _mi(walked) == expected

    ast = bca.Ast.parse(DOCUMENTED, "python")
    assert _mi(ast.metrics(mi_charges_docs=True)) == expected
    # One parse, both modes: the kwarg is read per call.
    assert _mi(ast.metrics()) == _mi(bca.analyze_source(DOCUMENTED, "python"))
    assert _mi(ast.metrics()) != expected


def test_charging_docs_leaves_an_undocumented_file_unmoved() -> None:
    bare = "def f(n):\n    # a working comment\n    if n:\n        return 1\n    return 0\n"
    default = bca.analyze_source(bare, "python")
    charged = bca.analyze_source(bare, "python", mi_charges_docs=True)
    assert default["metrics"]["loc"]["dloc"] == 0
    assert default == charged
