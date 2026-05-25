#!/usr/bin/env bash
# Build the mdBook and publish it to the `gh-pages` branch of the
# current repository.
#
# IMPORTANT: this script is for the **legacy** "Deploy from a branch"
# Pages mode only. It pushes to `gh-pages`, which GitHub Pages will
# only serve when the repository's Pages source is set to
# "Deploy from a branch" -> branch=gh-pages.
#
# The canonical publish path for this repo is the GitHub Actions
# workflow at `.github/workflows/book.yml`, which uses the official
# `actions/deploy-pages` flow. That flow requires Pages source to be
# set to "GitHub Actions". The two source modes are MUTUALLY
# EXCLUSIVE — only one can be active at a time. If the workflow is
# active, pushing to `gh-pages` does nothing visible; if this script
# is active, the workflow will fail at the `deploy-pages` step.
#
# Reach for this script only when:
#   - the GitHub Actions runner is unavailable for an extended
#     period, AND
#   - you have switched the repo's Pages source to "Deploy from a
#     branch -> gh-pages" in Settings -> Pages.
#
# Switch back to "GitHub Actions" once CI is healthy.
#
# Requirements: `git`, `mdbook` on PATH, push access to `origin`,
# a configured `git config user.name` / `user.email` (or the
# `GIT_AUTHOR_NAME` / `GIT_AUTHOR_EMAIL` env vars).
#
# Usage:
#     utils/deploy-book-to-gh-pages.sh           # build, commit, push
#     DEPLOY_REMOTE=upstream utils/deploy-book-to-gh-pages.sh
#     DEPLOY_PUSH=0 utils/deploy-book-to-gh-pages.sh  # build + commit only

set -euo pipefail

remote="${DEPLOY_REMOTE:-origin}"
branch="${DEPLOY_BRANCH:-gh-pages}"
push="${DEPLOY_PUSH:-1}"

repo_root="$(git rev-parse --show-toplevel)"
book_src="${repo_root}/big-code-analysis-book"
book_out="${book_src}/book"
worktree_dir="${repo_root}/target/gh-pages"

if ! command -v mdbook >/dev/null 2>&1; then
	echo "error: mdbook not found on PATH" >&2
	echo "  install with: cargo install --locked mdbook" >&2
	exit 1
fi

# Resolve the commit identity up-front so we fail fast with a clear
# message before doing minutes of build work, rather than dying at
# `git commit` with "empty ident name (for <>) not allowed".
commit_name="${GIT_AUTHOR_NAME:-$(git -C "$repo_root" config --default '' user.name)}"
commit_email="${GIT_AUTHOR_EMAIL:-$(git -C "$repo_root" config --default '' user.email)}"
if [ -z "$commit_name" ] || [ -z "$commit_email" ]; then
	echo "error: cannot determine git commit identity" >&2
	echo "  set GIT_AUTHOR_NAME + GIT_AUTHOR_EMAIL, or configure" >&2
	echo "  'git config user.name' and 'git config user.email'" >&2
	exit 1
fi

source_sha="$(git -C "$repo_root" rev-parse --short HEAD)"

echo "==> Building mdBook"
mdbook build "$book_src"

# Reset any stale worktree at the target location. `worktree list
# --porcelain` emits one record per worktree, with the path on a
# `worktree <path>` line — match against awk-extracted paths so a
# checkout under a directory containing regex metacharacters (`.`,
# `[`, etc.) cannot false-match or trip `grep` under `set -e`.
if git -C "$repo_root" worktree list --porcelain \
	| awk '$1 == "worktree" { print substr($0, 10) }' \
	| grep -Fxq -- "$worktree_dir"; then
	git -C "$repo_root" worktree remove --force "$worktree_dir"
fi
git -C "$repo_root" worktree prune

echo "==> Preparing ${branch} worktree at ${worktree_dir}"
if git -C "$repo_root" show-ref --verify --quiet "refs/heads/${branch}"; then
	git -C "$repo_root" worktree add "$worktree_dir" "$branch"
elif git -C "$repo_root" ls-remote --exit-code --heads "$remote" "$branch" >/dev/null 2>&1; then
	git -C "$repo_root" fetch "$remote" "${branch}:${branch}"
	git -C "$repo_root" worktree add "$worktree_dir" "$branch"
else
	# First-ever publish: build an orphan branch (no shared history
	# with main). Errors from `git rm` are fatal here — a partial
	# wipe would silently bake leftover source files into the first
	# gh-pages commit. The orphan checkout stages every tracked file
	# from HEAD, so `git rm -rf .` is expected to succeed.
	git -C "$repo_root" worktree add --detach "$worktree_dir"
	git -C "$worktree_dir" checkout --orphan "$branch"
	git -C "$worktree_dir" rm -rf --quiet . || {
		echo "error: failed to clear orphan ${branch} index" >&2
		exit 1
	}
fi

echo "==> Syncing book output into worktree"
# Wipe everything except .git so deleted pages are not left behind.
# `.nojekyll` is recreated unconditionally below.
find "$worktree_dir" -mindepth 1 -maxdepth 1 ! -name '.git' -exec rm -rf {} +
cp -R "${book_out}/." "${worktree_dir}/"
# GitHub Pages otherwise pipes the site through Jekyll, which drops
# files and directories whose names start with `_` (mdBook emits
# `FontAwesome/`, search index, etc. under that convention).
touch "${worktree_dir}/.nojekyll"

cd "$worktree_dir"
git add -A
if git diff --cached --quiet; then
	echo "==> No changes to deploy"
	exit 0
fi

git -c "user.name=${commit_name}" -c "user.email=${commit_email}" \
	commit -m "Deploy book from ${source_sha}"

if [ "$push" = "1" ]; then
	echo "==> Pushing to ${remote}/${branch}"
	git push "$remote" "$branch"
else
	echo "==> DEPLOY_PUSH=0 set; skipping push (commit is in ${worktree_dir})"
fi
