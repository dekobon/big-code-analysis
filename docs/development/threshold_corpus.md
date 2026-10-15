# Threshold calibration corpus

This file records the corpus behind the `halstead.effort` default in
[`big-code-analysis-cli/src/default_thresholds.rs`](../../big-code-analysis-cli/src/default_thresholds.rs)
and the `halstead.effort` column of the book's per-language table
([Choosing thresholds](../../big-code-analysis-book/src/recipes/thresholds.md#per-language)).
The other defaults and columns still rest on
[#1140](https://github.com/dekobon/big-code-analysis/issues/1140)'s 2026-07-31 corpus: 43 repositories
across 20 languages, whose repository list was never recorded.

The corpus below is a **rebuilt** one, made for
[#1569](https://github.com/dekobon/big-code-analysis/issues/1569) after the Halstead vocabulary
changes (#1486 through #1573). It covers the same 20 languages with the same sizing rule and the same kinds of
exclusions, so it is comparable to #1140's, but it is not the same set of repositories.

## Method

- Measured 2026-10-10 with a release `bca` built from `fix/batch-2026-10-10` at `18c28cac`, with
  every vocabulary change applied. The same corpus was also measured with the `v2.3.0` release, which
  predates all of them, to separate the metric change from the corpus change.
- Each repository was cloned with `git clone --depth 1` at its default branch.
- `bca metrics --no-config -O json -p <repo>` ran once per repository. The `-I` globs restricted it to
  the language's own extensions: `*.sh` (Bash-it also `*.bash`), `*.c`, `*.cpp *.cc *.h *.hpp`, `*.cs`,
  `*.ex`, `*.go`, `*.groovy`, `*.java`, `*.js *.mjs`, `*.kt`, `*.lua`, `*.m`, `*.pm *.pl`, `*.php`,
  `*.py`, `*.rb`, `*.rs`, `*.tcl`, `*.ts`, `*.tsx`. C counts `.c` only, because `bca` parses `.h` with
  the C++ grammar.
- The run excluded test, vendored, generated, and documentation-demo trees with these `-X` globs:
  `**/test/**`, `**/tests/**`, `**/testing/**`, `**/__tests__/**`, `**/spec/**`, `**/specs/**`,
  `**/t/**`, `**/testdata/**`, `**/fixtures/**`, `**/*_test.*`, `**/*_tests.*`, `**/test_*.py`,
  `**/*Test.java`, `**/*Tests.java`, `**/*Test.kt`, `**/*Test.groovy`, `**/*Spec.groovy`,
  `**/*.test.*`, `**/*.spec.*`, `**/*_spec.rb`, `**/*Tests/**`, `**/*.Tests/**`, `**/*.Test/**`,
  `**/*.Tests.*/**`, `**/benches/**`, `**/benchmark/**`, `**/benchmarks/**`, `**/vendor/**`,
  `**/third_party/**`, `**/thirdparty/**`, `**/third-party/**`, `**/node_modules/**`, `**/deps/**`,
  `**/dist/**`, `**/build/**`, `**/generated/**`, `**/*.min.js`, `**/*.d.ts`, `**/*.generated.*`,
  `**/*.demo.*`, `**/*.story.*`, `**/*.stories.*`, `**/demos/**`, `**/*-tests/**`, `**/*.pb.go`,
  `**/*_string.go`, `**/*.Designer.cs`, `**/*.designer.cs`.
  The per-repository extras in the corpus table exclude a duplicate source tree (Guava's `android/`
  backport), vendored libraries (three.js `libs/`), generated validators (webpack), and sample apps.
  Files `bca` recognises as generated are skipped on top of this.
- Values are each `function`-kind space's own `metrics.halstead.effort`, walked through nested spaces.
  That is the space kind `halstead.effort`'s `Function` scope admits in `src/metric_catalog.rs`.
  Percentiles use linear interpolation between closest ranks.

## Result

| Language | Functions | p50 | p90 | p97.5 | % > 50,000 | p97.5 at v2.3.0 | % > 50,000 at v2.3.0 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Bash | 1,803 | 803 | 8,699 | 44,195 | 2.1 | 44,195 | 2.1 |
| C | 6,361 | 5,582 | 60,359 | 182,534 | 11.8 | 173,763 | 11.3 |
| C# | 6,214 | 375 | 6,914 | 43,715 | 2.3 | 43,957 | 2.3 |
| C++ | 2,491 | 600 | 12,005 | 49,665 | 2.5 | 49,790 | 2.4 |
| Elixir | 4,535 | 460 | 4,398 | 15,121 | 0.4 | 15,121 | 0.4 |
| Go | 9,454 | 1,310 | 21,802 | 89,276 | 4.7 | 89,276 | 4.7 |
| Groovy | 12,529 | 221 | 5,275 | 25,353 | 1.4 | 23,038 | 1.3 |
| Java | 18,198 | 523 | 5,166 | 19,870 | 0.8 | 16,936 | 0.6 |
| JavaScript | 31,030 | 1,191 | 32,475 | 174,060 | 7.2 | 174,108 | 7.3 |
| Kotlin | 6,682 | 428 | 4,945 | 18,334 | 0.6 | 17,413 | 0.6 |
| Lua | 4,562 | 2,673 | 27,416 | 81,544 | 5.1 | 81,544 | 5.1 |
| Objective-C | 1,209 | 1,215 | 15,441 | 72,897 | 4.1 | 75,726 | 4.1 |
| Perl | 1,760 | 640 | 12,537 | 41,205 | 2.0 | 43,862 | 2.2 |
| PHP | 20,296 | 625 | 6,857 | 26,333 | 1.2 | 26,333 | 1.2 |
| Python | 10,264 | 1,030 | 10,904 | 41,983 | 2.0 | 28,535 | 1.2 |
| Ruby | 11,837 | 453 | 2,774 | 4,974 | 0.0 | 4,974 | 0.0 |
| Rust | 8,778 | 998 | 10,472 | 33,894 | 1.4 | 33,309 | 1.3 |
| Tcl | 10,907 | 804 | 13,412 | 45,543 | 2.3 | 45,537 | 2.3 |
| TSX | 6,456 | 774 | 26,726 | 134,616 | 6.0 | 131,020 | 6.0 |
| TypeScript | 6,945 | 830 | 14,724 | 60,339 | 3.1 | 60,126 | 3.0 |

At the shipped limit of 50,000, the median language flags 2.20% of its functions, inside the 1–3%
band the sizing rule asks for, so the default stands. Candidate limits for comparison: 40,000 gives
2.78% and 60,000 gives 1.76%. The `v2.3.0` binary gives 2.23% on the same corpus. The vocabulary
changes therefore barely moved the default's footing. Their largest per-language effect is Python
(p97.5 28,535 → 41,983), followed by Java (+17%) and Groovy (+10%). Bash, Elixir, Go, Lua, PHP, and
Ruby did not move at all, and Tcl moved by under 0.1%.

Ruby's low figures are a property of these two repositories, which hold their own methods short
with [RuboCop](https://docs.rubocop.org/rubocop/cops_metrics.html)'s method-length and ABC cops.
They are not a measurement gap.

## Corpus

| Language | Repository | Commit | Files | Functions | Extra excludes |
| --- | --- | --- | --- | --- | --- |
| Bash | [nvm-sh/nvm](https://github.com/nvm-sh/nvm) | `b1b3a0038c0666c7436cb5d383608b4b72c8300b` | 4 | 154 |  |
| Bash | [ohmybash/oh-my-bash](https://github.com/ohmybash/oh-my-bash) | `abf846186ab0a8a41ec5888e827ece6277dfe446` | 218 | 765 |  |
| Bash | [Bash-it/bash-it](https://github.com/Bash-it/bash-it) | `81929690975f9a7ea3c50b87265f2959327bb0a5` | 334 | 884 |  |
| C | [curl/curl](https://github.com/curl/curl) | `b8f71d6e1199701352c498467c17847237d1c43e` | 372 | 4,718 |  |
| C | [libuv/libuv](https://github.com/libuv/libuv) | `4080d7d965fdf291f37ec72677fc5a2ace8fa361` | 120 | 1,643 |  |
| C++ | [fmtlib/fmt](https://github.com/fmtlib/fmt) | `35c58f084e7cd79b51997439cff68346f4c724c0` | 20 | 1,426 |  |
| C++ | [google/leveldb](https://github.com/google/leveldb) | `bb74ef739973a70ca9f0d90788a1da89b788c0ad` | 98 | 1,065 |  |
| C# | [JamesNK/Newtonsoft.Json](https://github.com/JamesNK/Newtonsoft.Json) | `52fa3aef1f2cadcd3a3f874251eddc98d3efbbaa` | 243 | 4,129 |  |
| C# | [AutoMapper/AutoMapper](https://github.com/AutoMapper/AutoMapper) | `6e8697bc44f02fb54ef6a556a7d134e63485299e` | 92 | 2,085 |  |
| Elixir | [phoenixframework/phoenix](https://github.com/phoenixframework/phoenix) | `9f2c1409ca8bb9d55ca5b9cb7b4244ff645fc2d3` | 85 | 1,688 |  |
| Elixir | [elixir-ecto/ecto](https://github.com/elixir-ecto/ecto) | `94d69279c517347ff0962b138f4ccd0556486ae2` | 60 | 2,847 |  |
| Go | [caddyserver/caddy](https://github.com/caddyserver/caddy) | `d4165a4eb1fdb129e8ce8390099021a00239ecf7` | 225 | 2,603 |  |
| Go | [gohugoio/hugo](https://github.com/gohugoio/hugo) | `0732eadd5aead94d4bd4674d87e9bfc79aaf10a7` | 511 | 6,851 |  |
| Groovy | [nextflow-io/nextflow](https://github.com/nextflow-io/nextflow) | `ca258a4e9bce2763001dc67770e2bd957ccf1034` | 967 | 9,815 |  |
| Groovy | [apache/groovy](https://github.com/apache/groovy) | `000449404bb0fa255c8bd55cbb35e1c3c7edd379` | 399 | 2,714 |  |
| Java | [apache/commons-lang](https://github.com/apache/commons-lang) | `dd2c9e85784b288c7507c3736818040b3b457842` | 264 | 4,550 |  |
| Java | [google/guava](https://github.com/google/guava) | `6b3a8d7498a7fde122967ba37a023ba3dcb47792` | 666 | 13,648 | `**/android/**` |
| JavaScript | [mrdoob/three.js](https://github.com/mrdoob/three.js) | `11738a9d55e502fb484cefffcea3a66dc44e9fe9` | 1,403 | 14,891 | `**/libs/**` |
| JavaScript | [webpack/webpack](https://github.com/webpack/webpack) | `5ba5dba8b82a0ebd339a5c2be56166400de3460d` | 1,297 | 16,139 | `**/*.check.js` |
| Kotlin | [Kotlin/kotlinx.coroutines](https://github.com/Kotlin/kotlinx.coroutines) | `bd2e9a1b90400fb7b2fa4f8731e7d8148799ab73` | 342 | 3,029 |  |
| Kotlin | [square/okhttp](https://github.com/square/okhttp) | `42e5888d45623662c138ed27e80ea28bbe0aa5cc` | 367 | 3,653 |  |
| Lua | [Kong/kong](https://github.com/Kong/kong) | `2d2e67ea7a90cc9d81c53177e9fdbe61bd142236` | 609 | 3,722 |  |
| Lua | [luarocks/luarocks](https://github.com/luarocks/luarocks) | `a422d213b38c488472ec3bec70f372bb63784cdb` | 96 | 840 |  |
| Objective-C | [SDWebImage/SDWebImage](https://github.com/SDWebImage/SDWebImage) | `c3ad5e1a9bf55c9b76d4c362430b5fcded96c502` | 73 | 889 | `**/Examples/**` |
| Objective-C | [CocoaLumberjack/CocoaLumberjack](https://github.com/CocoaLumberjack/CocoaLumberjack) | `b75d3c3198a2930b4491b04fd47cb504b359c750` | 22 | 320 | `**/Demos/**` |
| Perl | [mojolicious/mojo](https://github.com/mojolicious/mojo) | `da40b9c04e31a530b05ce285fca322e135b3b780` | 124 | 1,569 |  |
| Perl | [beyondgrep/ack3](https://github.com/beyondgrep/ack3) | `11d698f9407c6bd5dfcff1c264bf0aa60abacc5d` | 27 | 191 |  |
| PHP | [laravel/framework](https://github.com/laravel/framework) | `73a25a4ad3d1da83c3c62a100b8902da380015ef` | 1,820 | 17,242 |  |
| PHP | [composer/composer](https://github.com/composer/composer) | `43f74517021a59ed4054a801c1472ef66355c49a` | 315 | 3,054 |  |
| Python | [django/django](https://github.com/django/django) | `dab0a5c47f159eb58472110dcbc10bd4a9b01adb` | 763 | 9,324 |  |
| Python | [Textualize/rich](https://github.com/Textualize/rich) | `9d8f9a372cc5916fd4781fec207ced7ddac2f08f` | 143 | 940 |  |
| Ruby | [rubocop/rubocop](https://github.com/rubocop/rubocop) | `6fa180158e70aaa390fd9f8990b273c5bd38f9b8` | 958 | 10,548 |  |
| Ruby | [jekyll/jekyll](https://github.com/jekyll/jekyll) | `541d8b2ee75c8907de4744d4b87a7a6f1f997cae` | 97 | 1,289 |  |
| Rust | [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) | `3fce3b5bb0236da2df6d99672afb8a719642eca7` | 91 | 3,219 |  |
| Rust | [tokio-rs/tokio](https://github.com/tokio-rs/tokio) | `09a57c27ea64bb546fe16822b9d2e65c599f0fb4` | 488 | 5,559 |  |
| Tcl | [tcltk/tcllib](https://github.com/tcltk/tcllib) | `48b8b5dbdf23ced85530a8a2fe8aab1f4bf09f34` | 770 | 7,510 |  |
| Tcl | [tcltk/tklib](https://github.com/tcltk/tklib) | `b9f737163cd02f5b243df6c3342fa26e710ab898` | 445 | 3,397 |  |
| TypeScript | [vuejs/core](https://github.com/vuejs/core) | `4ab865a848a1da3d10fb674f857e5fff13094644` | 267 | 2,650 |  |
| TypeScript | [nestjs/nest](https://github.com/nestjs/nest) | `2731c18054fb348779311d81e4e20949e313be78` | 1,071 | 4,295 | `**/sample/**` |
| TSX | [excalidraw/excalidraw](https://github.com/excalidraw/excalidraw) | `e65eca999e071503efb893d3379a29fb96ff37c4` | 233 | 2,575 |  |
| TSX | [mantinedev/mantine](https://github.com/mantinedev/mantine) | `f7ab1ef52579366e2310fc12b1a12e88a821053a` | 759 | 3,881 |  |
