# macOS

Livecap supports macOS 13 or later on Apple Silicon. Windows continues to use WASAPI; macOS uses Apple's [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration) to capture system audio without a virtual audio driver. Audio is converted locally to the provider's mono PCM16 format. No screen images are recorded or transmitted.

## Build and run

Install Node.js 24, stable Rust, and Xcode Command Line Tools:

```sh
xcode-select --install
npm ci
npm run tauri dev
```

Build a local application and disk image with:

```sh
npm run tauri -- build --bundles app,dmg -- --locked
```

Find the output under `src-tauri/target/release/bundle/`. The release workflow builds explicitly with `--target aarch64-apple-darwin`; those packages are under `src-tauri/target/aarch64-apple-darwin/release/bundle/`.

## Install a release

Download the Apple Silicon `.dmg` from [GitHub Releases](https://github.com/MiaoForge/livecap/releases), open it, and drag **Livecap** to **Applications**. Quit any existing instance before replacing it, then launch Livecap from Applications and eject the disk image. The `.app.tar.gz` and `.sig` assets are for the updater, not the initial installation.

Release and local packages use an ad-hoc signature without an Apple Developer ID certificate. The complete app bundle is sealed for integrity, but the publisher is not verified and the app is not notarized. If macOS blocks first launch, confirm you downloaded the official release, try opening the installed app, then use **System Settings → Privacy & Security → Open Anyway** if available. Follow [Apple's instructions](https://support.apple.com/en-us/102445); do not disable Gatekeeper globally. A signature verification failure requires a rebuilt package, rather than bypassing verification.

## Permissions and credentials

Starting a session explicitly checks and requests system capture permission before connecting to a provider. If the permission check disagrees with the system, Livecap tries a short ScreenCaptureKit audio stream before connecting; it discards the probe audio locally. Grant the system capture prompt. If access is denied, Livecap shows instructions and does not start a session. In **System Settings → Privacy & Security → Screen & System Audio Recording** (called **Screen Recording** on older versions), allow Livecap and restart the app if requested. During development, the permission may be associated with the terminal or development executable; rebuilding or replacing an ad-hoc-signed application can require renewed permission. Meetings with **Record microphone** enabled also request **Privacy & Security → Microphone** access. Subtitle sessions and meetings with that option off do not request or open the microphone. The app includes a microphone usage description and the audio-input signing entitlement; Tauri embeds the usage description for development runs too. Denied microphone access stops startup with instructions; you can grant access and relaunch, or turn the microphone option off.

If Livecap remains blocked after an update even though the permission switch is on, macOS may still be checking an older ad-hoc signature. Quit Livecap, remove its entry from **Screen & System Audio Recording**, and add `/Applications/Livecap.app` again before relaunching. As a targeted troubleshooting alternative, `tccutil reset ScreenCapture com.chenmiaoi.livecap` clears only Livecap's screen capture approval; you must then grant access again. Repeated upgrades can require renewed authorization while releases use ad-hoc signing. The capture probe cannot repair a stale system authorization record.

API keys use macOS Keychain. Settings and meeting data live in `~/Library/Application Support/com.chenmiaoi.livecap/`. Changing output devices or displays during capture requires restarting the session. ScreenCaptureKit requires an available display and an interactive desktop session. Protected media may not permit audio capture.

## Verification

Run `npm run check`, `sh scripts/test-macos-audio.sh`, and a packaged build. The Swift checks exercise PCM resampling at both provider rates, mono/stereo input, and consumer failure. CI checks Apple Silicon macOS; these checks do not exercise privacy prompts or live capture. Before distributing a macOS build, verify on a real desktop:

1. Deny capture permission and confirm the session reports a helpful error; allow permission, relaunch, and retry.
2. Play system audio and test both OpenAI (24 kHz) and Soniox (16 kHz), including the level meter and translated captions.
3. Speak into the default microphone with no system playback, then test both sources together. Confirm your voice appears in captions and replay. Deny microphone access and verify the error and the system-only option. Pause, speak, continue, and verify the paused speech is absent from the recording. Leave a silent gap in a meeting, stop, and confirm replay duration and transcript alignment. Repeat start/stop and cancel during startup.
4. Save a provider key, relaunch, and confirm retrieval from Keychain. Delete it and verify it is no longer configured.
5. Move and resize the transparent caption window; open a knowledge source and the local recordings folder.

To verify a packaged build locally:

```sh
node scripts/verify-macos-bundles.mjs src-tauri/target/release/bundle
```

For a build with `--target aarch64-apple-darwin`, use `src-tauri/target/aarch64-apple-darwin/release/bundle`. Add `--updater` when the build includes an updater archive. The verifier checks the standalone app, the copy inside the DMG, and (when requested) the extracted updater app. CI exercises this check and a regression test that rejects an incomplete signature and a tampered updater resource.

Updater signatures authenticate update archives separately from ad-hoc application signing. No Apple certificate or notarization secrets are needed for the current release process.

## Diagnose missing audio

Run this from the same terminal used for development:

```sh
sh scripts/diagnose-macos-audio.sh --tone
```

The probe uses the same native capture bridge, requests permission, plays a quiet built-in test sound, and reports packet counts and peak PCM amplitude for eight seconds. It does not connect to a provider or save captured audio. A nonzero peak verifies audible capture; no nonzero samples or a capture error produces a nonzero exit status. Without `--tone`, play audio yourself during the test. The terminal's permission identity can differ from the installed application's identity.

Add `--microphone` to request microphone access and inspect that source too:

```sh
sh scripts/diagnose-macos-audio.sh --microphone --tone
```

Speak into the default microphone during the eight-second probe. It reports microphone packet count and peak separately and fails if no nonzero microphone samples arrive. The Swift probe examines the two raw sources; Rust mixer tests verify the combined 20 ms output clock and pause boundaries.

If capture works in the probe but the app stays at “waiting for speech”, inspect the development terminal for errors. A rustls message about ambiguous `ring` and `aws-lc-rs` providers indicates failure before capture starts. Livecap initializes its TLS provider at process startup; completely restart an older running development process after updating. In development builds, unwinding session task panics are converted to errors so normal record persistence and session cleanup can still run.

## Keychain authorization

macOS may ask for permission when Livecap first reads an existing provider key. Choose **Always Allow** for a trusted installed copy if you want macOS to remember access for that application; **Allow Once** grants only that read. See [Apple's Keychain access guidance](https://support.apple.com/en-gb/guide/mac-help/kychn002/26/mac/26).

Livecap reads each provider key at most once after a successful read during one app process, then reuses it in native memory for captions, meetings, knowledge preparation, and record analysis. Concurrent requests share the same read. Saving or deleting a key updates the cache only after the system credential store succeeds. Cached secret allocations are zeroed when replaced or dropped; keys are never returned to the frontend or stored in settings. If access is denied, a subsequent attempt can ask again.

This cache does not replace macOS authorization. A newly rebuilt or updated ad-hoc application can require authorization again, even after **Always Allow** on the previous build. Changing a key directly in Keychain Access requires restarting Livecap to refresh its in-memory copy. Livecap does not modify the item's access-control policy or grant access to other applications.
