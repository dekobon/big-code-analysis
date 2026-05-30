#!/usr/bin/env python3
"""check-manpage-assets

Guard that every ``man/bca-*.1`` page is referenced in the deb *and*
rpm asset lists of its owning crate's ``Cargo.toml``.

The asset tables in ``big-code-analysis-cli/Cargo.toml`` and
``big-code-analysis-web/Cargo.toml`` are hand-maintained (see #444,
where ``bca-diff-baseline.1`` had silently dropped out of the CLI
lists). Any future ``bca`` subcommand that ships a man page can
regress the same way. This gate fails loud, naming the offending
file(s).

Partitioning rule:

* ``bca-web.1`` is owned by ``big-code-analysis-web``.
* every other ``man/bca-*.1`` is a CLI subcommand page owned by
  ``big-code-analysis-cli``.

For each owning crate, the page must appear in BOTH
``[package.metadata.deb].assets`` and
``[package.metadata.generate-rpm].assets``. The two table shapes
differ (deb uses ``["src", "dest", "mode"]`` arrays; rpm uses
``{ source = "src", … }`` tables), so the check matches on the
basename of the asset source path, which is robust against
formatting drift and dest-path style differences.

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import pathlib
import sys
import tomllib

REPO_ROOT = pathlib.Path(__file__).resolve().parent
MAN_DIR = REPO_ROOT / "man"
CLI_MANIFEST = REPO_ROOT / "big-code-analysis-cli" / "Cargo.toml"
WEB_MANIFEST = REPO_ROOT / "big-code-analysis-web" / "Cargo.toml"

# Page owned by the web crate; every other man/bca-*.1 is a CLI page.
WEB_PAGE = "bca-web.1"


def _is_subcommand_man(basename: str) -> bool:
    """True for ``bca-*.1`` man-page basenames.

    Scopes the reverse-direction guards (#447) to man pages only, so
    binaries, completions, the top-level ``bca.1`` (no hyphen), and
    licence files are not swept into the wrong-crate / stale checks.
    """
    return basename.startswith("bca-") and basename.endswith(".1")


def asset_basenames(manifest: pathlib.Path) -> dict[str, set[str]]:
    """Return ``{"deb": {basenames…}, "rpm": {basenames…}}``.

    Reads both metadata asset tables and reduces each source path to
    its basename. deb assets are ``[src, dest, mode]`` arrays; rpm
    assets are ``{source, dest, …}`` tables.
    """
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    metadata = data.get("package", {}).get("metadata", {})

    deb_assets = metadata.get("deb", {}).get("assets", [])
    rpm_assets = metadata.get("generate-rpm", {}).get("assets", [])

    deb = {pathlib.PurePosixPath(entry[0]).name for entry in deb_assets}
    rpm = {pathlib.PurePosixPath(entry["source"]).name for entry in rpm_assets}
    return {"deb": deb, "rpm": rpm}


def main() -> int:
    pages = sorted(p.name for p in MAN_DIR.glob("bca-*.1"))
    if not pages:
        sys.stderr.write(
            f"error: no man/bca-*.1 pages found under {MAN_DIR}\n"
            "       (expected at least the CLI subcommand pages)\n"
        )
        return 2

    cli_pages = [p for p in pages if p != WEB_PAGE]
    web_pages = [p for p in pages if p == WEB_PAGE]

    cli_assets = asset_basenames(CLI_MANIFEST)
    web_assets = asset_basenames(WEB_MANIFEST)

    # (page, owning crate label, deb set, rpm set)
    checks: list[tuple[str, str, set[str], set[str]]] = []
    checks += [(p, "big-code-analysis-cli", cli_assets["deb"], cli_assets["rpm"]) for p in cli_pages]
    checks += [(p, "big-code-analysis-web", web_assets["deb"], web_assets["rpm"]) for p in web_pages]

    missing: list[str] = []
    for page, crate, deb, rpm in checks:
        if page not in deb:
            missing.append(f"  {page}: missing from {crate} deb assets")
        if page not in rpm:
            missing.append(f"  {page}: missing from {crate} rpm assets")

    if missing:
        sys.stderr.write(
            "error: man page(s) not referenced in packaging asset lists\n"
        )
        sys.stderr.write("\n".join(missing) + "\n")
        sys.stderr.write(
            "\nEvery man/bca-*.1 page must be listed in BOTH the deb\n"
            "([package.metadata.deb].assets) and rpm\n"
            "([package.metadata.generate-rpm].assets) tables of its\n"
            "owning crate's Cargo.toml. bca-web.1 lives in\n"
            "big-code-analysis-web; all other pages live in\n"
            "big-code-analysis-cli. See #444 for the bug class this guards.\n"
        )
        return 1

    # Reverse-direction guards (#447). The forward check above only
    # proves each page is present in its owner; it cannot see a page
    # listed in the WRONG crate, nor an asset entry whose source no
    # longer exists. Both are scoped to bca-*.1 basenames so shared
    # assets (binaries, completions, the top-level bca.1, licences)
    # are not swept in.
    cli_man = {b for b in cli_assets["deb"] | cli_assets["rpm"] if _is_subcommand_man(b)}
    web_man = {b for b in web_assets["deb"] | web_assets["rpm"] if _is_subcommand_man(b)}

    crossed: list[str] = []
    # The web page must not appear in the CLI tables, and no CLI
    # subcommand page may appear in the web tables. (bca.1 is owned by
    # the CLI crate, so it is excluded from both directions.)
    for table_label, basenames in (
        ("big-code-analysis-cli deb", cli_assets["deb"]),
        ("big-code-analysis-cli rpm", cli_assets["rpm"]),
    ):
        if WEB_PAGE in basenames:
            crossed.append(f"  {WEB_PAGE}: wrongly listed in {table_label} (owned by big-code-analysis-web)")
    for table_label, basenames in (
        ("big-code-analysis-web deb", web_assets["deb"]),
        ("big-code-analysis-web rpm", web_assets["rpm"]),
    ):
        for page in sorted(basenames):
            if _is_subcommand_man(page) and page != WEB_PAGE:
                crossed.append(f"  {page}: wrongly listed in {table_label} (owned by big-code-analysis-cli)")

    if crossed:
        sys.stderr.write(
            "error: man page(s) listed in the wrong crate's asset lists\n"
        )
        sys.stderr.write("\n".join(crossed) + "\n")
        sys.stderr.write(
            "\nbca-web.1 belongs to big-code-analysis-web; every other\n"
            "bca-*.1 subcommand page belongs to big-code-analysis-cli.\n"
            "Move the offending entry to its owning crate's Cargo.toml.\n"
        )
        return 1

    stale: list[str] = []
    for crate, basenames in (
        ("big-code-analysis-cli", cli_man),
        ("big-code-analysis-web", web_man),
    ):
        for page in sorted(basenames):
            if not (MAN_DIR / page).is_file():
                stale.append(f"  {page}: listed in {crate} assets but missing from man/")

    if stale:
        sys.stderr.write(
            "error: asset entry points at a man page that does not exist\n"
        )
        sys.stderr.write("\n".join(stale) + "\n")
        sys.stderr.write(
            "\nEvery bca-*.1 asset source must resolve to a file under\n"
            "man/. Remove the stale entry or restore the missing page.\n"
        )
        return 1

    print(f"manpage-assets: OK ({len(checks)} page(s) checked)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
