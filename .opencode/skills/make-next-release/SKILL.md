---
name: make-next-release
description: Make the next release for a GitHub-hosted Rust repo on the development/main split — read the current dev version(s), update the CHANGELOG, pin the clean release version (no -test suffix), merge development into main, push, tag v<version>, then roll development back to the next -test.N dev version. Self-adapts to both CI-driven repos (a .github/workflows release job publishes on push to main) and manual repos (gh release create + post-release deploy). Use when the user says "make the next release", "next release", "cut a release", "do/release vX.Y.Z", "ship/tag/publish", or wants to bump the version and run the release.
---

# Make Next Release

End-to-end release: update the CHANGELOG, promote the current `development`
state to a clean released version, merge → push → tag (publishing via the
repo's mechanism), then return `development` to a `-test.N` dev version ready
for the next iteration.

This skill is **generic across the author's Rust repos** (RustyMealie,
WeatherAnalysis, …). It decides how each repo publishes by inspecting the repo
during the READ phase, rather than assuming a single layout. Keep the
"Per-repo notes" section up to date as the facts drift.

## 0. Read the repo (always do this first)

Establish the facts below before touching anything. Do not guess from memory of
another project.

```bash
git branch --show-current           # must be: development
git status --short                  # clean except intentional work in progress
git remote get-url origin           # → owner/repo, used for merge URLs and gh
git tag --sort=-v:refname | head -5 # last released version + tag naming
git ls-files .github/workflows/     # is there a release workflow at all?
```

Then classify the repo on three axes:

1. **Version source**
   - *Single*: one root `Cargo.toml` with `[workspace.package] version = "..."`,
     and member crates inherit it (`version.workspace = true`). One number to
     pin, one `Cargo.lock` at the root.
   - *Multi-crate*: no root workspace; each crate dir has its own
     `Cargo.toml` `version` and its own `Cargo.lock`. The crate that carries a
     `-test.N` suffix is the one that defines the next release number.
2. **Publish mechanism**
   - *CI-driven*: a `.github/workflows/*.yml` release job exists that **runs on
     push to `main`**. Pushing `main` publishes; the tag only marks the commit.
     These repos usually gate on the version string: if it contains `test`,
     the build is skipped (`allow=false`), so a suffixed version pushed to
     `main` must never happen.
   - *Manual*: no release workflow. Releasing = tagging `v<version>` and
     running `gh release create` (plus any post-release deploy step).
3. **Post-release steps** (per-repo table below, e.g. WeatherAnalysis web
   deploy; RustyMealie desktop bundles only via CI + local Android step).

## Version conventions

- `development` always carries a dev version with a `-test.N` suffix, e.g.
  `0.2.1-test.1`. It must never match a released `X.Y.Z` (a plain version on
  `main` publishes accidentally in CI repos).
- **Derive the release version from the current dev version**: strip the
  `-test.N` suffix. `0.2.1-test.1` → release **`0.2.1`**. Read the number off
  the actual `version = "..."` in the crate(s) rather than guessing.
- After releasing `X.Y.Z`, the next dev version is **`X.Y.(Z+1)-test.1`**
  (patch bump + suffix), e.g. release `0.2.1` → dev `0.2.2-test.1`. Never reuse
  `X.Y.Z-test.1` for the same number being released — that collides.

## Procedure

### 1. Update the CHANGELOG (`CHANGELOG.md`)

- Rename `## [Unreleased]` → `## [<version>] - <YYYY-MM-DD>` (today's date).
- Fold this release's user-visible changes into the `Added` / `Changed` /
  `Fixed` sections under it; remove or re-home anything that did not actually
  ship. Keep the Keep-a-Changelog + Semantic Versioning intro intact.
- Add a fresh `## [Unreleased]` heading directly above the new release section.
- Update the reference links at the bottom of the file using the owner/repo
  read in step 0:
  - `[Unreleased]` → `.../compare/v<version>...HEAD`
  - add `[<version>]: https://github.com/<owner>/<repo>/releases/tag/v<version>`
- If previous changelog entries were never tagged (common mid-migration), fold
  or leave them as-is rather than inventing tags for the past.

### 2. Pin the version for release

- *Single*: edit `[workspace.package] version` in the root `Cargo.toml`.
- *Multi-crate*: edit each crate that is part of this release. The leading
  crate gets the suffix stripped (`0.2.1-test.1` → `0.2.1`); crates sitting at
  a plain semver bump to the release number only if they actually shipped
  changes this cycle, otherwise leave them. Note any separate display channel
  (e.g. `src-tauri/tauri.conf.json` `version`) and keep it equal to the
  **release** version — the `-test.N` suffix lives only in `Cargo.toml`.
- Run `cargo check` (or `cargo build --release` where a release build is the
  norm) in every changed crate so each `Cargo.lock` records the new version.
- Confirm `git diff` shows exactly the intended version bumps + lockfiles.

### 3. Commit on `development`

`git add` only the intended files — release commit, lockfiles, `CHANGELOG.md`,
any code changes not yet committed. Round up leftover `git checkout -- <file>`
for tooling noise (`*.idea/` churn etc.) before opening the commit.

```bash
git commit -m "chore(release): v<version>"
```

Use conventional-commit style in the body if more than a version bump is
involved (`fix(...)`, `feat(...)` lines).

### 4. Merge to `main`

```bash
git checkout main
git fetch origin
git log --oneline development..main   # expect EMPTY output
```

If `main` has nothing unique, fast-forward is clean and expected:

```bash
git merge --ff-only development
```

If `development..main` is non-empty, `--ff-only` fails — stop and reconcile (a
merge commit + conflict resolution) before pushing, rather than forcing.

### 5. Push `main` and tag

Tag the merge commit, name it exactly `v<version>`:

```bash
git push origin main
git tag v<version>
git push origin v<version>
```

Then publish via the repo's mechanism:

- **CI-driven**: pushing `main` already triggered the workflow; the tag just
  marks the commit and pre-names the GitHub release. Nothing more to do.
- **Manual**: create the GitHub release from the tag (edit notes if the
  CHANGELOG diff is cleaner than auto-generated ones):

```bash
gh release create v<version> --generate-notes
```

### 6. Verify the release

- **CI-driven**:

  ```bash
  gh run list --workflow=release.yml --limit 2
  curl -s https://api.github.com/repos/<owner>/<repo>/releases/tags/v<version>
  ```

  If not up after a while, check the Actions tab for the gate output
  ("version" vs "allow").
- **Manual**:

  ```bash
  gh release view v<version>
  ```

### 7. Reset `development` to a dev version

```bash
git checkout development
```

Patch-bump the version(s) + suffix (`0.2.1` → `0.2.2-test.1`) in the same
places pinned in step 2, run `cargo check` to refresh the `Cargo.lock`s, and
commit/push:

```bash
git commit -m "chore(version): bump to 0.2.2-test.1 for development"
git push origin development
```

### 8. Post-release steps (per repo)

Run whatever the repo's per-repo notes (below) require after the tag lands —
usually a deploy. If the plan is "ship new data too", do the data-refresh
pull/combine before deploying so the server has current data.

## Per-repo notes

| Repo | Version source | Publish | Post-release |
|---|---|---|---|
| WeatherAnalysis | multi-crate: `pull_data`, `cleanup_data`, `plot_data/{data-core,server,src-tauri}` each own version + `Cargo.lock`; `pull_data` carries the `-test.N` suffix | manual — `gh release create v<version>`; no CI workflow | Web deploy of `plot_data/server`: `cargo build --release`, copy binary to `~/.local/bin/weather-plot-server`, `sudo systemctl restart weather-plot`; frontend-only changes = restart only. Serving at `http://bicknellfamily.duckdns.org/CorvallisWeather/` |
| RustyMealie | single `[workspace.package]` version in root `Cargo.toml`; display follows via `version.workspace`; `src-tauri/tauri.conf.json` version = release version | CI — `.github/workflows/release.yml` runs on push to `main`, gates on `test` in the version, builds desktop bundles (`.deb`, `.AppImage`), publishes GitHub Release `v<version>` | Desktop bundles ship via CI; Android APK is a manual local step (`cargo tauri android build --apk --debug`) |

Add rows here for any new repo this skill is used against.

## Caveats

- **In CI repos the tag does not run the workflow; pushing `main` does.** The
  tag only marks the commit and pre-names the release.
- CI gates string-search for `test`, so a dev version without a suffix pushed
  to `main` publishes. When in doubt, leave the suffix on `development`.
- No `gh` or an unauthenticated environment: fall back to the public API
  (`https://api.github.com/repos/<owner>/<repo>/releases`) for verification.
- Don't compile-release-changes after tagging: tag exactly the commit you
  pushed. If a release build fails or the merge is dirty, fix on `development`,
  re-merge, re-push, `git tag -f`/delete-remote-tag, and re-push the tag on the
  corrected commit.
- `.idea/` (JetBrains) files are tracked in these repos but should never enter
  a release commit — unstage/checkout their churn.
- Pre-push hooks in these repos run `cargo audit`, `cargo fmt --check`,
  `cargo clippy`, and `cargo test` across every crate — pushes to `main` and
  `development` will re-run the full checks, so expect them to be slow rather
  than treating a slow push as a hang.