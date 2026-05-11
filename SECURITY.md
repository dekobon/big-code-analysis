# Security Policy

## Reporting a Vulnerability

If you believe you have found a security vulnerability in
`big-code-analysis`, please report it privately. **Do not open a
public GitHub issue.**

Please report vulnerabilities via one of the following:

- **GitHub Security Advisories** (preferred, once the repository is
  public): use the
  ["Report a vulnerability"](https://github.com/dekobon/big-code-analysis/security/advisories/new)
  button on this repository.
- **Email**: <e.zupancic@f5.com>.

Please include the following in your report:

- A description of the vulnerability and its impact.
- Steps to reproduce, or a proof-of-concept.
- The affected version(s).
- Any suggested mitigation, if known.

## Response Expectations

- We will acknowledge receipt within **3 business days**.
- We aim to provide an initial assessment within **7 business days**.
- We will keep you informed of progress toward a fix and coordinate
  a disclosure timeline with you.
- Typical time-to-fix for confirmed vulnerabilities is **30–90 days**
  depending on severity and complexity.

## Disclosure Policy

We follow **coordinated disclosure**. Once a fix is available:

1. We will publish a patched release on crates.io (subject to the
   crates.io publication gate described in
   [`RELEASING.md`](RELEASING.md)).
2. We will publish a [RustSec advisory](https://rustsec.org/) with a
   CVE identifier where appropriate.
3. We will credit the reporter in the advisory unless they prefer
   to remain anonymous.

We ask that reporters give us a reasonable window (typically 90 days)
to release a fix before public disclosure.

## Verifying release artefacts

Every `v*` tag is intended to publish signed release artefacts to
[GitHub Releases](https://github.com/dekobon/big-code-analysis/releases).
Until the release pipeline lands (tracked in
[`RELEASING.md`](RELEASING.md)), the verification recipe below is the
expected post-release shape:

- **`SHA256SUMS`** — SHA-256 hashes of every artefact in the release.
- **`SHA256SUMS.minisig`** —
  [minisign](https://jedisct1.github.io/minisign/) signature over
  `SHA256SUMS`. Verify with the committed `minisign.pub`:

  ```bash
  minisign -Vm SHA256SUMS -p minisign.pub
  grep <artefact> SHA256SUMS | sha256sum -c
  ```

- **SLSA build provenance** — every artefact carries a GitHub-signed
  provenance attestation. Verify with the `gh` CLI:

  ```bash
  gh attestation verify <artefact> -R dekobon/big-code-analysis
  ```

- **CycloneDX SBOM** — `*.cdx.json` for the library and each binary
  (`bca`, `bca-web`).

If either signature check fails, do **not** install the artefact — file
a security report via the channels above.

When verifying against an older release, fetch `minisign.pub` from the
release's tagged commit (not `main`) — if the key was rotated after
that release, `main` carries a different key and verification fails.

## Scope

This policy covers vulnerabilities in the code of this workspace
(`big-code-analysis`, `big-code-analysis-cli`,
`big-code-analysis-web`, and the vendored
`tree-sitter-{ccomment,mozcpp,mozjs,preproc,tcl}` grammar crates).
Vulnerabilities in upstream tree-sitter grammars or other dependencies
should be reported to the respective upstream projects; we will update
our pinned dependency requirements promptly once upstream fixes are
available.

## Safe Harbor

We consider security research conducted in good faith under this
policy to be authorized. We will not pursue legal action against
researchers who:

- Make a good-faith effort to avoid privacy violations, data
  destruction, or service disruption.
- Report vulnerabilities promptly.
- Do not exploit the vulnerability beyond what is necessary to
  demonstrate it.
- Give us reasonable time to respond before public disclosure.
