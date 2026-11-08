# Desktop releases

Push a tag of the form `vX.Y.Z` or `vX.Y.Z-rcN` to trigger the release workflow. RC numbering starts at 1. Commit a non-empty English `docs/release/releasenote-<tag>.md` before tagging. GitHub uses that file as the release body and marks RCs as prereleases.

## Signing configuration

Set the following in the repository's GitHub Actions settings.

| Name                                 | Type     | Value                                         |
| ------------------------------------ | -------- | --------------------------------------------- |
| `TAURI_UPDATER_PUBLIC_KEY`           | Variable | Contents of the Tauri updater public key file |
| `TAURI_SIGNING_PRIVATE_KEY`          | Secret   | Contents of the matching private key file     |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Secret   | Signing key password, if set                  |

Generate the key with `npm run tauri -- signer generate`. The workflow runs the standard checks and publishes a signed Windows x64 NSIS installer, signature, and update manifest to the tagged release.

## Update feed

The updater in **Settings → General** reads the [MiaoForge Livecap update manifest](https://github.com/MiaoForge/livecap/releases/download/updates/latest.json). Release builds set the manifest URL to the repository running the workflow, so a fork can publish its own signed updates. The manifest advertises the highest published version across stable and RC releases. Tags and the app display use `-rcN`. The installer and update manifest use `-rc.N` so RC numbers sort numerically.

Clients must be able to download the manifest and installer without authentication. Local builds without an update source show that status in Settings. See the [Tauri updater documentation](https://v2.tauri.app/plugin/updater/) for the manifest and signature format.

## macOS releases

After the Windows release is published, the macOS job checks the project and builds an Apple Silicon `.app` and `.dmg`. It uploads a signed updater archive and adds `darwin-aarch64` to the same versioned manifest and update feed. If the macOS job fails, rerun it to finish publication; the Windows release remains available.

macOS packages use an **ad-hoc signature** (`bundle.macOS.signingIdentity: "-"`), following Threadline's packaging approach. No Apple Developer ID certificate or notarization credentials are required. Tauri seals the complete application bundle before creating the DMG and updater archive. Before uploading assets, the workflow verifies the standalone `.app`, the application inside the mounted DMG, and the extracted updater application with `codesign --verify --deep --strict`. A broken or incomplete signature fails publication.

Ad-hoc signing checks bundle integrity without verifying the publisher. These builds are not notarized and can require approval in macOS Privacy & Security on first launch. See [installation and security notices](macos.md#install-a-release).

The `TAURI_SIGNING_PRIVATE_KEY`, password, and public key are still required for updater archives. This is Livecap's own update authentication key, separate from Apple code signing. See [Tauri ad-hoc signing](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing).
