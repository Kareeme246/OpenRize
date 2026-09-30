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
   the same `cliff.toml` to generate GitHub Release notes and builds a signed,
   notarized macOS `.dmg`/`.app` with `pnpm tauri build`.
3. Watch the `Release` workflow. When it finishes, it publishes the GitHub
   Release with the `.dmg` and zipped `.app` attached, plus the in-app
   updater's `openrize.app.tar.gz` and `latest.json`.

Publishing a release ships it to every installed copy: the app checks
`releases/latest/download/latest.json` hourly (see `src-tauri/src/updater.rs`)
and offers the update in the sidebar and Settings. Drafts and pre-releases are
never offered, because `releases/latest` skips them.

## Signing and notarization secrets

The release workflow signs with a Developer ID Application certificate and
notarizes with an App Store Connect API key. It needs these repository secrets
(Settings > Secrets and variables > Actions):

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | base64 of the exported Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password chosen when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_API_KEY` | App Store Connect API key ID |
| `APPLE_API_ISSUER` | App Store Connect issuer ID |
| `APPLE_API_PRIVATE_KEY` | full contents of the `AuthKey_<KEYID>.p8` file |
| `TAURI_SIGNING_PRIVATE_KEY` | full contents of the updater's minisign private key |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | password chosen when generating that key |


The app's hardened-runtime entitlements live in `src-tauri/Entitlements.plist`
and privacy prompt strings in `src-tauri/Info.plist`; any new API that macOS
gates behind an entitlement (Apple Events, camera, etc.) must be added there or
it silently fails in signed builds.
