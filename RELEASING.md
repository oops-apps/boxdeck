# Releasing

1. Make sure CI is green on `main` and `CHANGELOG.md` lists the changes under `## [Unreleased]`.
2. In GitHub, open **Actions → Release → Run workflow**, keep the branch on `main`, enter the version (for example `0.2.0`) and tick **Publish** if it should go live without a review.
3. The workflow then:
   - sets the version in `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json` and `src-tauri/tauri.conf.json`;
   - turns `## [Unreleased]` into `## [0.2.0] - <date>` in `CHANGELOG.md`, commits `chore(release): v0.2.0` to `main` and pushes the `v0.2.0` tag;
   - creates a draft release whose notes are that changelog section;
   - builds macOS (Apple Silicon and Intel), Windows and Linux bundles plus standalone `boxdeck-mcp` archives into it;
   - publishes the release once every build succeeds, if you ticked **Publish**. Otherwise check the draft and publish it yourself.

A version with a suffix such as `0.2.0-beta.1` becomes a pre-release.

The workflow pushes to `main` with the built-in `GITHUB_TOKEN`. If `main` is protected, allow GitHub Actions to bypass the rule, or bump the versions in a pull request and push the tag by hand instead: `git tag v0.2.0 && git push origin v0.2.0` builds the same draft release, after checking the tag matches the versions in the code.

## Auto-update (not enabled yet)

The updater needs a signing key pair that only the maintainers hold:

1. Run `npx tauri signer generate -w ~/.tauri/boxdeck.key` and keep the private key and its password safe.
2. Add them as repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Add `tauri-plugin-updater`, set `bundle.createUpdaterArtifacts` to `true`, and put the public key and `https://github.com/oops-apps/boxdeck/releases/latest/download/latest.json` under `plugins.updater` in `tauri.conf.json`.

## Code signing (later)

macOS notarization needs an Apple Developer account; the `boxdeck-mcp` sidecar must be signed along with the app. Windows signing can use Azure Trusted Signing.
