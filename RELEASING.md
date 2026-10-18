# Releasing

1. Make sure `npm run check` passes on `main` and CI is green.
2. Update the version in `Cargo.toml` (`[workspace.package]`), `package.json` and `src-tauri/tauri.conf.json`, and add a section to `CHANGELOG.md`.
3. Commit, then tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
4. The **Release** workflow builds macOS (Apple Silicon and Intel), Windows and Linux bundles plus standalone `boxdeck-mcp` archives, and attaches them to a **draft** GitHub release. Check it, then publish it.

## Auto-update (not enabled yet)

The updater needs a signing key pair that only the maintainers hold:

1. Run `npx tauri signer generate -w ~/.tauri/boxdeck.key` and keep the private key and its password safe.
2. Add them as repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Add `tauri-plugin-updater`, set `bundle.createUpdaterArtifacts` to `true`, and put the public key and `https://github.com/oops-apps/boxdeck/releases/latest/download/latest.json` under `plugins.updater` in `tauri.conf.json`.

## Code signing (later)

macOS notarization needs an Apple Developer account; the `boxdeck-mcp` sidecar must be signed along with the app. Windows signing can use Azure Trusted Signing.
