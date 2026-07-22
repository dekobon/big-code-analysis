# Documentation translations

The mdBook under `big-code-analysis-book/` is translated with the
gettext workflow from
[mdbook-i18n-helpers](https://github.com/google/mdbook-i18n-helpers).
The English sources under `src/` are the single source of truth;
translations live in one PO file per language under
`big-code-analysis-book/po/` (currently `ja.po`). The deployed site
serves English at the root and Japanese at
[`/ja/`](https://dekobon.github.io/big-code-analysis/ja/).
`README.ja.md` at the repository root is a hand-maintained sibling of
`README.md` and is not part of the gettext pipeline.

Untranslated and stale entries **fall back to English** at build time,
so a doc PR never blocks on translation and readers never see stale
Japanese for changed English text. That fallback is the reason this
workflow was chosen over a copied `src/ja/` tree: prose is translated
per-paragraph in one file, code blocks stay untranslated automatically
(only comments and string literals are extracted), and `msgmerge`
marks every paragraph the English edit touched.

## Toolchain

```console
cargo install mdbook-i18n-helpers --version '=0.3.6' --locked
```

The 0.3.x line pairs with the mdbook 0.4.x pinned in
`.github/workflows/pages.yml` (`MDBOOK_VERSION` /
`MDBOOK_I18N_HELPERS_VERSION`); mdbook-i18n-helpers 0.4.0+ targets
mdbook 0.5 and the two pins must move together. The preprocessor is
registered in `book.toml` with `optional = true`, so a plain
`mdbook build` keeps working without it (English output only).
GNU gettext (`msgmerge`, `msgfmt`, `msgattrib`) comes from the system
package manager.

## Editing English docs

Nothing extra is required. Changed paragraphs simply render in English
on the Japanese site until someone refreshes the translation. When a
doc PR wants to keep `ja.po` current in the same change, run the
refresh workflow below; otherwise refreshing periodically is fine.

One rule does apply to English edits: headings that are the target of
a fragment link (`[…](#anchor)` or `[…](page.md#anchor)`) carry an
explicit `{#anchor}` attribute so the anchor survives heading
translation (a translated heading otherwise derives a different HTML
id and the link breaks only on the Japanese site). When you add a new
fragment link, pin the target heading's id the same way.

## Refreshing the Japanese translation

```console
make book-po-update   # regenerate messages.pot + msgmerge into ja.po
```

`msgmerge` fills `po/ja.po` with new empty entries and marks edited
ones fuzzy. List what needs attention:

```console
msgattrib --untranslated big-code-analysis-book/po/ja.po
msgattrib --only-fuzzy   big-code-analysis-book/po/ja.po
```

Translate those entries (drop the `#, fuzzy` marker once an entry is
verified — fuzzy entries still render as English), then validate and
preview:

```console
msgfmt --check -o /dev/null big-code-analysis-book/po/ja.po
make book-ja          # builds into big-code-analysis-book/book/ja
```

Terminology: keep the glossary already established in `ja.po`
consistent (しきい値 for threshold, ベースライン for baseline,
抑制マーカー for suppression marker, 循環的複雑度 / 認知的複雑度 for
cyclomatic / cognitive complexity, 保守容易性指数 for maintainability
index). Prose style is です・ます調. Inline code, flags, paths, link
targets, and product names stay verbatim.

`messages.pot` is generated on demand (`make book-pot`) and is
gitignored — only `ja.po` is tracked.

## Adding another language

1. `make book-pot`, then
   `msginit -i big-code-analysis-book/po/messages.pot -l <lang> -o big-code-analysis-book/po/<lang>.po`.
2. Translate as above.
3. Add a build step for the language in the `build-book` job of
   `.github/workflows/pages.yml` (mirror the Japanese step, output
   directory `book/<lang>`), and extend the language-switch line in
   `big-code-analysis-book/src/README.md`.
4. Mirror `README.ja.md` with a hand-translated `README.<lang>.md`
   and add it to the language links at the top of each README.
