# Getting started

Build and launch Livecap using the [quick start](../README.md#quick-start). Complete onboarding or configure a provider later under **Settings → Connection**. Livecap stores provider API keys in Windows Credential Manager or macOS Keychain.

## Start a session

1. Choose **Subtitle** for live captions or **Meeting** for a replayable record.
2. Choose **Translate to** under **Settings → Language & Translation**. Input speech is detected automatically, including language changes within a session; available languages and recognition quality depend on the provider.
3. Start the session and review the audio transmission notice. Livecap sends system playback audio to the selected provider. Meetings enable **Record microphone** by default; turn it off on the meeting card to capture only system audio. Subtitle mode never opens the microphone. Restart the session after changing playback or input devices.
4. In Meeting mode, use **Pause meeting** / **Continue meeting** in the app or caption window. Pausing discards new audio before recording and upload; already submitted audio can still return final text. Continuing appends to the same meeting record, with pause time excluded from both recording and transcript positions. You can also stop a paused meeting.
5. Stop the session from the app or the caption window. Closing the caption window hides it.

With microphone capture enabled, your voice and system playback are mixed into the same mono stream for transcription and meeting replay. Use headphones to reduce sound from the speakers being picked up again by the microphone. Pause discards both sources and clears pending mixed audio; continuing stays in the same record without replaying buffered audio from the pause.

Subtitle mode clears audio and text when the session ends and retains usage totals. Meeting mode saves audio and transcript data locally. The record view supports replay, correction, export, and source-linked notes. The OpenAI route can generate meeting notes automatically when enabled.

## Live meeting workspace

During a meeting, the right pane shows the original transcript with speaker labels and timestamps. The left pane switches between translation and a provisional summary with key points, source links, and a relationship diagram when the conversation supports one. Each transcript pane follows new speech independently; scrolling up pauses following, and **Back to latest** resumes it.

Live summaries use the OpenAI key already stored in the desktop client. They are enabled by default for meetings and can be disabled under **Settings → Knowledge and Storage** before starting. Once there is enough confirmed original speech, updates are coalesced roughly every 30 seconds; shorter passages wait longer. Silence and provisional text do not trigger requests. Updates merge earlier summarized points with new speech, so they are a compact working overview rather than a verbatim record. They incur additional model charges, included in the meeting's analysis usage. Soniox meetings require a separately configured OpenAI key for summaries.

A failed update keeps the previous summary and retries with a delay. Ending the meeting stops new summary requests and lets an already submitted request complete before saving its usage. The last successful live summary is archived with its source timestamps, followed by the normal analysis of the full meeting transcript. If final analysis fails, the archived live summary remains labeled as provisional. Recordings and the full transcript are saved independently of summary success.

## Knowledge and usage

Use **Knowledge Space** to prepare domain terminology. Preparation reads public sources and can incur model and search charges. The local cache stores entries, source links, and usage. A failed refresh preserves the existing cache. Automatic vocabulary focuses on spoken concepts, product names, and acronyms. Command switches, underscore-style code identifiers, and code expressions are excluded from new entries and filtered from existing caches when loaded. Explicitly saved personal vocabulary remains available.

Save multiple domain libraries and enable them independently in **Knowledge Space**. Every enabled library contributes to the next session. Preparing a different domain or focus adds a library; refreshing the same domain and focus updates that library. Existing single-library data is migrated automatically.

Knowledge preparation can run during a meeting or alongside session startup. The active session keeps the knowledge and ASR vocabulary captured when it started; preparing or enabling another library affects the next session without restarting capture. Only one knowledge generation can run at a time.

The **Usage** page shows estimates based on recorded audio and provider-reported usage. Provider invoices determine actual charges.

Errors, warnings, and operation notices appear in the bottom-right notification stack without moving the page. Errors remain until dismissed; warnings and informational notices close automatically and pause while hovered or focused. A failed audio segment that OpenAI reports as unintelligible is skipped while the meeting continues. Other transcription failures distinguish authentication, quota, rate limits, configuration, and service errors when the service supplies a code.

## Local data

Livecap stores settings, terminology, meeting records, and usage in `%LOCALAPPDATA%\com.chenmiaoi.livecap\livecap.sqlite3`. Meeting audio is stored in the adjacent `recordings` directory.

On macOS, local data is stored in `~/Library/Application Support/com.chenmiaoi.livecap/`, with the same database and recordings layout. See [macOS setup](macos.md) for capture permission instructions.

OpenAI audio is submitted in bounded batches, but those batches are not treated as paragraph boundaries. Completed sentences are finalized separately; unfinished text carries into the next batch so its final translation can include the continuation. Live draft translations can still change. Long gaps, speaker changes, session end, or a safety limit (1,600 characters or 60 seconds) finalize an unfinished passage. Sentence timestamps within an audio batch are estimates. Existing records keep their saved segmentation and translations.

The app keeps a single target language per session. Existing target-language preferences are preserved when upgrading; the old source-language selection is discarded. OpenAI live transcription does not report detected language labels, so those labels remain unset rather than being inferred from settings. Soniox can return detected language metadata. Speech already in the target language is retained.

Pausing keeps the provider connection open to preserve the session. Soniox still charges for the full connection duration, including pauses; see its [connection keepalive documentation](https://docs.soniox.com/stt/rt/connection-keepalive). Pausing is available after a meeting starts listening; subtitle sessions do not expose meeting pause controls.
