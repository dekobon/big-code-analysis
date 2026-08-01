# big-code-analysis

[![crates.io](https://img.shields.io/crates/v/big-code-analysis.svg)](https://crates.io/crates/big-code-analysis)
[![MSRV](https://img.shields.io/crates/msrv/big-code-analysis.svg)](Cargo.toml)
[![CI](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![codecov](https://codecov.io/gh/dekobon/big-code-analysis/graph/badge.svg)](https://codecov.io/gh/dekobon/big-code-analysis)
[![CodeQL](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml?query=branch%3Amain)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/dekobon/big-code-analysis/badge)](https://scorecard.dev/viewer/?uri=github.com/dekobon/big-code-analysis)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/13461/badge)](https://www.bestpractices.dev/projects/13461)
[![docs.rs](https://docs.rs/big-code-analysis/badge.svg)](https://docs.rs/big-code-analysis)
[![License](https://img.shields.io/crates/l/big-code-analysis.svg)](LICENSE)

[English](README.md) | 日本語

**big-code-analysis** はコードの保守性を測定するツールです。
コマンドラインツール `bca` は、[20 を超えるプログラミング言語](https://dekobon.github.io/big-code-analysis/ja/languages.html)を対象に、
関数単位のメトリクスを計算します。循環的複雑度、
[認知的複雑度](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)、
[Halstead メトリクス](https://en.wikipedia.org/wiki/Halstead_complexity_measures)、保守容易性指数、ABC、各種コード行数など、
[メトリクス一式](https://dekobon.github.io/big-code-analysis/ja/metrics.html)をサポートします。
パースには [tree-sitter](https://tree-sitter.github.io/tree-sitter/) を使うため、
コンパイラもビルドステップも言語ランタイムも不要です。ディレクトリを指定するだけで数値が出力されます。

本プロジェクトは Mozilla の [rust-code-analysis](https://github.com/mozilla/rust-code-analysis) のハードフォークで、
メトリクスエンジンをコード品質ツールチェーンへと発展させたものです。

- `bca check` — ベースライン、ソース内抑制マーカー、CI 向け終了コードを備えたしきい値ゲート。
- エージェントフィードバック — 編集のたびに違反を [Claude Code](https://code.claude.com/docs/en/overview) や
  [opencode](https://opencode.ai/) にフィードバック（[下記](#コーディングエージェントにメトリクスをフィードする)参照）。
- `bca report` — Markdown / HTML のホットスポットレポート。
- `bca vcs` — git ツリーに対する変更履歴メトリクス（チャーン、所有権の希薄化、バグ修正履歴）。
- ライブラリバインディング — 同じエンジンを Rust クレート、
  [Python パッケージ](https://pypi.org/project/big-code-analysis/)、REST サーバー（`bca-web`）として利用できます。

インストール前に出力を確認したい場合は、`bca` が `main` へのプッシュのたびに
自分自身のソースを解析して公開している結果をご覧ください。

- [**HTML ホットスポットレポート（実例）**](https://dekobon.github.io/big-code-analysis/reports/index.html)
  — ファイル単位・関数単位でブラウズできるビュー（英語）。
- [**Markdown レポート（実例）**](https://dekobon.github.io/big-code-analysis/reports/report.md)
  — 同じ実行結果をプルリクエストのコメント形式にしたもの（英語）。

完全なドキュメントは[**ドキュメントブック（日本語版）**](https://dekobon.github.io/big-code-analysis/ja/)にあります。
メトリクスの定義、コマンドリファレンス、CI レシピ、ライブラリガイドを収録しています。

## コーディングエージェントにメトリクスをフィードする

コーディングエージェントは大量のコードを書きますが、
そのループの中に「この関数は保守できないほど複雑になった」と教えてくれる仕組みはありません。
`bca check` はそのループを閉じます。エージェントが編集した各ファイルをチェックし、編集が確定した瞬間に、
問題のある関数をモデルのコンテキストへ報告します。
必要なのは `PATH` 上の `bca`（[クイックスタート](#クイックスタート)参照）と数行の設定だけです。

- **Claude Code** — `PostToolUse` フックが編集されたファイルに対して `bca check` を実行し、違反をモデルにフィードバックします。
  本リポジトリ自身がリファレンス実装のフック [`.claude/hooks/bca-check.sh`](.claude/hooks/bca-check.sh) をドッグフーディングしています。
- **opencode** — `tool.execute.after` プラグインが同じ役割を果たします。
  リファレンスコピーは [`.opencode/plugins/bca-check.js`](.opencode/plugins/bca-check.js) にあります。

[エージェントフィードバックのレシピ](https://dekobon.github.io/big-code-analysis/ja/recipes/agent-feedback.html)には、
両ツール向けのコピー＆ペーストで使える設定に加えて、
エージェントがコードを簡潔にする代わりにメトリクスの数値だけを下げる「メトリクスのゲーム化」を防ぐガイダンスブロックも掲載しています。

## クイックスタート

[リリースページ](https://github.com/dekobon/big-code-analysis/releases)からビルド済みの `bca` をインストールするか
（Linux・macOS・Windows 向けの署名付き tarball と `.deb`・`.rpm`・`.apk` パッケージ）、パッケージレジストリからインストールします。

```console
cargo install big-code-analysis-cli    # または: pip install big-code-analysis-cli
```

その後、プロジェクトのルートで次を実行します。

```console
bca metrics src/main.rs      # 1 ファイルの関数単位メトリクスツリー
bca init                     # bca.toml・.bcaignore・.bca-baseline.toml を生成
bca check                    # 関数がしきい値を超えると終了コード 2
bca report -O html -o report.html
```

全サブコマンド・フラグ・出力形式は、
ブックの [Commands](https://dekobon.github.io/big-code-analysis/ja/commands/index.html) の章に記載されています。

## CI での品質ゲートとレポート

`bca check` はしきい値・ベースライン・除外設定をコミット済みの `bca.toml` から読み込むため、CI、ローカル実行、
エージェントフックのすべてが同じシグナルでゲートされます。`bca report` は同じ実行結果を、
プルリクエスト向けの Markdown コメントや HTML のホットスポットページに変換します。
本リポジトリはプッシュのたびに自分自身をゲートし、その結果を公開しています。

- HTML ホットスポットレポート: <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR コメント: <https://dekobon.github.io/big-code-analysis/reports/report.md>

[CI 統合レシピ](https://dekobon.github.io/big-code-analysis/ja/recipes/ci.html)が導入ガイドです。
チェックサム検証付きのリリース固定インストール、そのまま使える GitHub Actions / GitLab CI ジョブに加えて、
既存コードベースを段階的に締めていくための[ベースライン](https://dekobon.github.io/big-code-analysis/ja/recipes/baselines.html)と
[ローカルしきい値ゲート](https://dekobon.github.io/big-code-analysis/ja/recipes/local-gates.html)のレシピがあります。

## ライブラリとして使う

`big-code-analysis` クレートは、明文化された安定性契約（[STABILITY.md](./STABILITY.md)、英語）のもとで
crates.io に公開されています。公開 API は `2.x` 系のパッチ・マイナーバンプの間は安定を保ち、
破壊的変更は次のメジャーバンプまで持ち越されます。ただし、
文法のバージョン固定が更新された場合やメトリクス定義が修正された場合には、
メトリクスの*値*はマイナーバンプでも変動することがあります。何が約束され、何が約束されないかは契約に明記されています。

```toml
[dependencies]
big-code-analysis = "2"
```

各文法は言語ごとの Cargo フィーチャーの背後に置かれています。デフォルトではすべて有効で、
一部だけ必要な場合はデフォルトフィーチャーを無効化して個別の言語を再度有効化できます。
ブックの[言語別 Cargo フィーチャー](https://dekobon.github.io/big-code-analysis/ja/library/cargo-features.html)と、
タスク指向のウォークスルー（クイックスタート、インメモリ解析、`FuncSpace` 結果の走査、エラーハンドリング）をまとめた
[ライブラリとして使う](https://dekobon.github.io/big-code-analysis/ja/library/index.html)の章を参照してください。
API リファレンスは [docs.rs](https://docs.rs/big-code-analysis) にあります。

Python バインディング（[PyO3](https://pyo3.rs/)）は
[`big-code-analysis-py/`](./big-code-analysis-py/README.md) にあり、
[PyPI の `big-code-analysis` パッケージ](https://pypi.org/project/big-code-analysis/)として同じメトリクスパイプラインを提供します。
ブックの [Python バインディング](https://dekobon.github.io/big-code-analysis/ja/python/index.html)の章で、
インストール、バッチ処理・非同期処理、
[SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html) 出力を解説しています。

サービスとして動かす場合は、`bca-web` がライブラリを REST API としてラップします。
[bca-web の運用](https://dekobon.github.io/big-code-analysis/ja/commands/web-server.html)を参照してください。

## ビルドと貢献

このリポジトリは Cargo ワークスペースで、よく使うタスクのための `Makefile` ラッパーを備えています。
`make help` で全タスクの一覧を確認できます。

```console
make build        # ワークスペース全体のデバッグビルド
make test         # 完全なテストスイート（ワークスペース、全フィーチャー）
make pre-commit   # CI と同等のローカルゲート一式
```

貢献のワークフローは [CONTRIBUTING.md](./CONTRIBUTING.md)（英語）に、
内部構造（言語の追加、メトリクスの実装、文法の更新）はブックの
[開発者ガイド](https://dekobon.github.io/big-code-analysis/ja/developers/index.html)にまとまっています。

## ライセンス

- 同梱の文法クレート（`tree-sitter-ccomment`、`tree-sitter-mozcpp`、`tree-sitter-mozjs`、
  `tree-sitter-preproc`、`tree-sitter-tcl`）は MIT ライセンスで公開されています。

- **big-code-analysis**、**big-code-analysis-cli**、**big-code-analysis-web**、
  **big-code-analysis-py** は [Mozilla Public License v2.0](https://www.mozilla.org/MPL/2.0/)
  のもとで公開されています。
