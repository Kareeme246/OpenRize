# Releasing

Tags are plain semver: `vX.Y.Z` (e.g. `v0.3.4`). Every user-facing change
bumps the patch version in the same pull request and uses a Conventional Commit
subject. See `cliff.toml` for the commit categories and `AGENTS.md` for the
project-wide rule.

To cut a release:

1. On `main`, regenerate and commit the changelog:
   ```sh
   ./scripts/changelog.sh
   git add CHANGELOG.md
   git commit -m "docs: update changelog for vX.Y.Z"
   ```
   `CHANGELOG.md` is generated from git history; do not edit it by hand.
2. Tag and push the version already recorded in `package.json`,
   `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`:
   ```sh
   git tag vX.Y.Z
   git push origin main vX.Y.Z
   ```
   The release workflow rejects a tag unless all three versions match. It uses
   the same `cliff.toml` to generate GitHub Release notes and builds an unsigned
   macOS `.dmg`/`.app` with `pnpm tauri build`.
3. Watch the `Release` workflow. When it finishes, it publishes the GitHub
   Release with the `.dmg` and zipped `.app` attached. There is no Apple
   Developer signing or notarization yet. On first launch, right-click (or
   Control-click) `openrize.app` and choose **Open**, or go to **System Settings
   > Privacy & Security** and click **Open Anyway**, then confirm.
