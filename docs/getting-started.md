# Getting started

Build and launch Livecap using the [quick start](../README.md#quick-start). Complete onboarding or configure a provider later under **Settings → Connection**. Livecap stores provider API keys in Windows Credential Manager or macOS Keychain.

## Start a session

1. Choose **Subtitle** for live captions or **Meeting** for a replayable record.
2. Check the source and target languages in Settings.
3. Start the session and review the audio transmission notice. Livecap sends audio from the system audio capture stream to the selected provider. Restart the session after changing playback devices.
4. Stop the session from the app or the caption window. Closing the caption window hides it.

Subtitle mode clears audio and text when the session ends and retains usage totals. Meeting mode saves audio and transcript data locally. The record view supports replay, correction, export, and source-linked notes. The OpenAI route can generate meeting notes automatically when enabled.

## Knowledge and usage

Use **Knowledge Space** to prepare domain terminology. Preparation reads public sources and can incur model and search charges. The local cache stores entries, source links, and usage. A failed refresh preserves the existing cache.

The **Usage** page shows estimates based on recorded audio and provider-reported usage. Provider invoices determine actual charges.

## Local data

Livecap stores settings, terminology, meeting records, and usage in `%LOCALAPPDATA%\com.chenmiaoi.livecap\livecap.sqlite3`. Meeting audio is stored in the adjacent `recordings` directory.

On macOS, local data is stored in `~/Library/Application Support/com.chenmiaoi.livecap/`, with the same database and recordings layout. See [macOS setup](macos.md) for capture permission instructions.
