# Livecap v0.1.2-rc1

This release candidate adds macOS support and improves meeting recording and translation.

- Support Apple Silicon Macs running macOS 13 or later, with native system audio capture and a floating caption window.
- Record the default microphone alongside system audio in meetings. Turn off **Record microphone** before starting to capture system audio only; subtitle sessions continue to use system audio only.
- Pause and continue meetings from the main window or caption window, keeping audio and transcripts in the same record and excluding paused audio.
- Detect input languages automatically and translate speech into the selected target language.
- Reuse API keys during the app's lifetime to avoid repeated Keychain authorization prompts in the same run.
- Fix TLS initialization failures and ensure failed sessions can finish cleanup and saving.
- Provide macOS permission guidance, audio diagnostics, and verified application bundles and updater archives.

## Installation notes

Windows packages support x64. macOS packages support Apple Silicon only; Intel Macs are not supported.

macOS packages use ad-hoc signing and are not notarized. First launch may require approval in **System Settings → Privacy & Security**. System audio capture requires screen/system audio recording permission; meetings with microphone recording enabled also require microphone permission. Replacing an ad-hoc-signed build can require renewed permission.

See the [macOS guide](https://github.com/MiaoForge/livecap/blob/v0.1.2-rc1/docs/macos.md) for installation and troubleshooting.
