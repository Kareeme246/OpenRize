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
2. Push the release commit to `main`. In GitHub Actions, run the `Release`
   workflow on that commit with `version` set to `X.Y.Z` (without `v`). It must
   match `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
   `src-tauri/cli/Cargo.toml`, and `src-tauri/core/Cargo.toml`
   (`./scripts/bump-version.sh` updates them all). The workflow creates the tag
   or verifies an existing tag points at that commit, and refuses an existing
   GitHub release to avoid mixing asset sets.
3. Select `macos_arm` (default on), `windows` (default off), and `scope`
   (`standard` by default; `comprehensive` also includes the zipped macOS
   `.app`). At least one platform must be selected. Leave `r2` off for the
   existing GitHub-releases-only path, or turn it on to also update the
   website's installers after the GitHub release is published (setup below).
4. Watch the `Release` workflow. It uses `cliff.toml` for release notes and
   publishes precisely the selected platforms' assets and `latest.json`.
   macOS includes the signed, notarized `.dmg` and updater's
   `openrize.app.tar.gz`; Windows includes the unsigned NSIS installer. Both
   updater artifacts have minisign signatures. `install.sh` installs the CLI
   from that same `openrize.app.tar.gz`, so there is no separate CLI download.

Publishing a release ships it to every installed copy: the app checks
`releases/latest/download/latest.json` hourly (see `src-tauri/src/updater.rs`)
and offers the update in the sidebar and Settings. Drafts and pre-releases are
never offered, because `releases/latest` skips them.

## Optional website publishing (R2)

The `r2` dispatch checkbox defaults to **false**. Turning it on publishes to
**GitHub Releases and R2**, using the same platform selection. Select both
platforms if both should be available on the website: the new website manifest
lists only the selected platforms, not installers from an older version.
The app's in-app updater still uses GitHub, regardless of this checkbox.

Both entry points use the single `publish-r2.yml` workflow. During a normal
release it runs only after GitHub publishing succeeds. To publish an existing
release without rebuilding, run **Publish website installers (R2)** in GitHub
Actions, enter its version (for example `0.9.20`), select the platforms, and
explicitly turn `r2` on. This standalone dispatch does not create a tag, build
installers, or change the GitHub release. Leaving `r2` off does nothing.

Both entry points require a published stable release and every selected
installer. They download the installers into an isolated directory and fail
on missing assets, HTTP errors, or truncated downloads before invoking the
publisher. This is important because the publisher's own
default GitHub-download mode permits missing platforms; the workflow instead
passes the complete staged directory with `--dir`.

Uploads use the publisher in `scripts/publish-r2/` (`pnpm publish-r2`, or
`node scripts/publish-r2/cli.ts`), which has no dependencies beyond Node. It
validates installer formats, uploads under `openrize/releases/<version>/`, and
reads every installer back to verify its size and SHA-256 before publishing the
version manifest and finally `openrize/current.json`. A failed installer upload
or verification cannot advance the current pointer. R2 jobs serialize across
versions, and the publisher refuses accidental downgrades. No Worker deployment
or deletion of previous releases is involved by default. Flags: `--version`,
`--dir`, `--remote` or `--local` (plain files under `--persist-to`, default
`.r2-local`), `--no-current` (stage without promoting), `--promote` (make an
already published version current, for rollback), `--purge` (delete the
previous release once the new one is current) and `--bucket`.

The manifest format is byte-compatible with what the website already parses
(`manifest.ts` is a straight port of the website's reader); do not change it
without changing the website too.

Add these two **OpenRize repository Actions secrets** in Settings > Secrets and
variables > Actions before opting in:

| Secret | Value and minimum access |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | Cloudflare API token with **Account > Workers R2 Storage > Edit** (`Workers R2 Storage Write`), scoped to only the account owning `offline-studios-downloads`. This includes object read-back and write access. No Workers Scripts, DNS, or zone permissions are needed. |
| `CLOUDFLARE_ACCOUNT_ID` | The Cloudflare account ID owning that bucket. It is an identifier, not a credential, but this workflow reads it from an Actions secret for consistent configuration. |

The bucket name is fixed to `offline-studios-downloads`; there is no additional
bucket ID, S3 access key, or S3 secret to configure. The publisher calls
Cloudflare's REST API directly (the same object endpoints `wrangler r2 object`
uses). Its account-scoped R2 Edit permission is broader than one bucket;
R2's bucket-scoped **Object Read & Write** tokens are S3-only and do not work
with this publisher. See Cloudflare's [R2 token permissions][r2-tokens].

Nothing is installed in the R2 job, so no dependency code runs beside the
credentials, which are provided only to the publish step and are not needed
when `r2` is off.

If R2 fails, the GitHub release remains published. Failures before promotion
leave the old website pointer unchanged; an error reading the pointer back
could happen after promotion, so inspect the logs before retrying. Fix the
cause and use Actions' **Re-run failed jobs** or the standalone R2 dispatch,
not a fresh `Release` dispatch (which correctly refuses an existing GitHub
release). Versions with a published R2 manifest are immutable; if a failure
occurred after writing that manifest, deliberate promotion/recovery is an
operator action with `--promote` (or a fresh `--no-current` stage) from `scripts/publish-r2/`, not an option in this workflow.

[r2-tokens]: https://developers.cloudflare.com/r2/api/tokens/#permissions

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

`install.sh` only accepts a CLI notarized and signed by team `Z899WY5Y94`, and
the release workflow runs that same check on the bundled `rize` before publishing. If the signing
identity's team ever changes, update the requirement in `install.sh`.


The app's hardened-runtime entitlements live in `src-tauri/Entitlements.plist`
and privacy prompt strings in `src-tauri/Info.plist`; any new API that macOS
gates behind an entitlement (Apple Events, camera, etc.) must be added there or
it silently fails in signed builds.
