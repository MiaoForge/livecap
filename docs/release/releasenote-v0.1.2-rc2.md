# Livecap v0.1.2-rc2

This release candidate improves the live meeting workspace, adds multiple knowledge libraries, and makes transcription failures easier to handle.

## Meetings and records

- Show live original speech on the right and translation or an automatically updated summary on the left. Summaries can include diagrams and links back to the original speech.
- Reuse the OpenAI key saved in the desktop client for live summaries, include summary usage in meeting costs, and preserve the last successful summary when later analysis fails.
- Follow new speech independently in each transcript pane. Scrolling up pauses following; **Back to latest** resumes it.
- Delete saved meeting records, including their audio, transcript, translation, and notes, while retaining usage accounting.
- Keep microphone recording enabled by default for meetings and move its control into settings.

## Knowledge Space

- Save multiple domain libraries and enable them together. Existing single-library data migrates automatically.
- Prepare another domain without replacing existing libraries. Refreshing the same domain and focus updates only that library.
- Combine enabled libraries for new sessions and share the ASR keyword budget across domains.
- Allow knowledge preparation during a meeting. The active session retains its original knowledge snapshot; changes apply to the next session.
- Exclude command switches, code expressions, and underscore-style identifiers from automatically collected vocabulary while preserving spoken concepts and explicitly saved personal vocabulary.

## Reliability and interface

- Show errors, warnings, and operation notices in a dismissible bottom-right stack without shifting page content.
- Continue a meeting when OpenAI reports an unintelligible audio segment. Distinguish authentication, quota, rate limit, configuration, and service failures when error codes are available.
- Fix narrow-screen layout problems, including the 30-day usage chart. Daily chart buttons remain readable inside a horizontally scrollable region.
- Synchronize page navigation with URL fragments, refresh, and browser history.
- Improve text contrast, touch targets, navigation landmarks, control labels, selected-state announcements, and keyboard access to caption regions.
- Disable API key editing in browser previews, where keys cannot be saved.

## Installation notes

Windows packages support x64. macOS packages support Apple Silicon running macOS 13 or later; Intel Macs are not supported.

macOS packages use ad-hoc signing and are not notarized. First launch may require approval in **System Settings → Privacy & Security**. System audio capture requires screen/system audio recording permission; meetings with microphone recording enabled also require microphone permission. Replacing an ad-hoc-signed build can require renewed permission.

Live summaries and knowledge preparation can incur additional API charges. Live summaries use confirmed original speech and remain provisional until the meeting is archived.

See the [macOS guide](https://github.com/MiaoForge/livecap/blob/v0.1.2-rc2/docs/macos.md) for installation and troubleshooting.
