# packaging/

Templates consumed by `.github/workflows/release.yml` on every `v*`
tag. Every template uses `@@TOKEN@@` placeholders that the release
workflow substitutes at build time:

| Token                  | Meaning                                   |
| ---------------------- | ----------------------------------------- |
| `@@VERSION@@`          | `${GITHUB_REF_NAME#v}` (e.g. `0.1.0`)     |
| `@@APK_PKGVER@@`       | `@@VERSION@@` with `-` replaced by `_`    |
| `@@TARGET@@`           | Rust target triple                        |
| `@@ARCH@@`             | Alpine arch list (`x86_64` / `aarch64`)   |
| `@@SHA256_*@@`         | SHA-256 of the matching release tarball   |
| `@@SHA512@@`           | SHA-512 (Alpine)                          |

| File                                          | Consumer                              |
| --------------------------------------------- | ------------------------------------- |
| `alpine/APKBUILD.in`                          | `abuild -r` in Stage 2                |
| `freebsd/+MANIFEST.in`                        | `pkg create -M` in Stage 2            |
| `freebsd/port/`                               | Published as-is for ports-tree PRs    |
| `homebrew/big-code-analysis.rb.tmpl`          | Pushed to `dekobon/homebrew-tap` (shared tap) |
| `scoop/big-code-analysis.json.in`             | Pushed to `dekobon/scoop-bucket` (shared bucket) |

Each format installs **both** `bca` (CLI) and `bca-web` (REST server)
side-by-side. The deb and rpm jobs in `release.yml` split into two
per-binary packages (`big-code-analysis-cli` / `big-code-analysis-web`)
so users can opt in to only one; the apk, FreeBSD, Homebrew, and Scoop
flows ship a single combined package because their ecosystems make
multi-binary packages the idiomatic choice.

Homebrew tap and Scoop bucket pushes are gated by repo variables
(`ENABLE_HOMEBREW_TAP`, `ENABLE_SCOOP_BUCKET`); the templates are
committed so a future maintainer can flip the gate without a code
change.
