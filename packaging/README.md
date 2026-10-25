# packaging/

Package-manager templates that `.github/workflows/release.yml`
consumes on every `v*` tag; see [`RELEASING.md`](../RELEASING.md) for
the pipeline they feed. Every template uses `@@TOKEN@@` placeholders
that the release workflow substitutes at build time:

| Token                  | Meaning                                   |
| ---------------------- | ----------------------------------------- |
| `@@VERSION@@`          | The tag minus its `v` prefix (`0.1.0`)    |
| `@@APK_PKGVER@@`       | `@@VERSION@@` with `-` replaced by `_`    |
| `@@TARGET@@`           | Rust target triple                        |
| `@@ARCH@@`             | Alpine arch (`x86_64` / `aarch64`)        |
| `@@SHA256_*@@`         | SHA-256 of the matching release tarball or zip |
| `@@SHA512@@`           | SHA-512 of the release tarball (Alpine)   |

| File                                          | Consumer                              |
| --------------------------------------------- | ------------------------------------- |
| `alpine/APKBUILD.in`                          | [`abuild -r`](https://wiki.alpinelinux.org/wiki/Creating_an_Alpine_package) in the `package-apk` job |
| `homebrew/big-code-analysis.rb.tmpl`          | Pushed to `dekobon/homebrew-tap` (shared [Homebrew](https://brew.sh/) tap) |
| `scoop/big-code-analysis.json.in`             | Pushed to `dekobon/scoop-bucket` (shared [Scoop](https://scoop.sh/) bucket) |

Each format installs **both** `bca` (CLI) and `bca-web` (REST server)
side by side. The deb and rpm jobs in `release.yml` split into two
per-binary packages (`big-code-analysis-cli` / `big-code-analysis-web`)
so users can opt in to only one; the apk, Homebrew, and Scoop flows
ship a single combined package because multi-binary packages are the
idiomatic choice in those ecosystems. There is no FreeBSD package: the
`x86_64-unknown-freebsd` target left the build matrix in #346, and its
templates went with it.

Homebrew tap and Scoop bucket pushes are gated by repo variables
(`ENABLE_HOMEBREW_TAP`, `ENABLE_SCOOP_BUCKET`); the templates are
committed so a future maintainer can flip the gate without a code
change.
