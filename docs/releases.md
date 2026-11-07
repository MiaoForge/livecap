# Windows releases

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
