# Releasing

Tags are plain semver: `vX.Y.Z` (e.g. `v0.3.1`). Pushing a `v*` tag runs the
`Release` workflow (`.github/workflows/release.yml`), which builds an
**unsigned** macOS `.dmg`/`.app` with `pnpm tauri build` and attaches them to
a GitHub Release. There is no Apple Developer signing or notarization yet -
the release notes tell users to right-click and "Open Anyway" on first
launch.

To cut a release:

1. On `main`, with a clean working tree, bump the version everywhere it's
   recorded:
   ```sh
   ./scripts/bump-version.sh X.Y.Z
   ```
   This updates `package.json`, `src-tauri/tauri.conf.json`, and
   `src-tauri/Cargo.toml` together so they can't drift.
2. Run `pnpm fix && pnpm verify`, then commit:
   ```sh
   git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml
   git commit -m "Release vX.Y.Z"
   ```
3. Tag and push:
   ```sh
   git tag vX.Y.Z
   git push origin main vX.Y.Z
   ```
4. Watch the `Release` workflow run. When it finishes, it has published the
   GitHub Release for `vX.Y.Z` with the `.dmg` and zipped `.app` attached.
