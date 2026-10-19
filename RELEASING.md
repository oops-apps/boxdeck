# Releasing

1. Make sure CI is green on `main` and `CHANGELOG.md` lists the changes under `## [Unreleased]`.
2. In GitHub, open **Actions → Release → Run workflow**, keep the branch on `main`, enter the version (for example `0.2.0`) and tick **Publish** if it should go live without a review.
3. The workflow then:
   - sets the version in `Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json` and `src-tauri/tauri.conf.json`;
   - turns `## [Unreleased]` into `## [0.2.0] - <date>` in `CHANGELOG.md`, commits `chore(release): v0.2.0` to `main` and pushes the `v0.2.0` tag;
   - creates a draft release whose notes are that changelog section;
   - builds macOS (Apple Silicon and Intel), Windows and Linux bundles into it, each app with its `boxdeck-mcp` inside, plus the signed update files and `latest.json`;
   - publishes the release once every build succeeds, if you ticked **Publish**. Otherwise check the draft and publish it yourself.

Releases are never marked pre-release, betas included: the in-app updater reads `releases/latest/download/latest.json`, and GitHub never treats a pre-release as the latest. When you publish a draft by hand, keep **Set as the latest release** ticked and don't delete `latest.json`. Windows MSI versions must be numeric, so its `.msi` is built as `0.2.0.1`, taking the suffix's trailing number. rpm versions can't contain `-`, so versions with a suffix ship no `.rpm`; the `.deb` and AppImage are still built.

The workflow pushes to `main` with the built-in `GITHUB_TOKEN`. If `main` is protected, allow GitHub Actions to bypass the rule, or bump the versions in a pull request and push the tag by hand instead: `git tag v0.2.0 && git push origin v0.2.0` builds the same draft release, after checking the tag matches the versions in the code.

## Auto-update

Release builds sign the update bundles with the key pair whose public key is under `plugins.updater` in `src-tauri/tauri.conf.json`. Only the maintainers hold the private key, as the repository secrets `TAURI_SIGNING_PRIVATE_KEY` (the contents of the key file) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Without them every release build fails.

Each release carries `latest.json`, the feed the app reads, and the `.app.tar.gz` files the macOS updater downloads. The `.sig` files aren't uploaded: their signatures are already inside `latest.json`.

If the key is ever lost, generate a new pair with `npx tauri signer generate -w ~/.tauri/boxdeck.key`, put the new public key in `tauri.conf.json` and replace both secrets. Installed copies only trust the old key, so they can't update past that release; their users have to download it once by hand.

## Code signing (later)

macOS notarization needs an Apple Developer account; the `boxdeck-mcp` sidecar must be signed along with the app. Windows signing can use Azure Trusted Signing.
