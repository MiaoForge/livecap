# Livecap client UI proposal

Preview: http://127.0.0.1:4317/client/#captions

Keeps the approved sidebar, workspace header, colors, session controls and settings layout. The client remains visible when starting captions. Captions have two main views: transcript history and an appearance preview using the sample LLVM slide.

The independent caption surface is explicitly opened/hidden, supports bottom docking, header dragging to float, double-click docking, shared pause/resume, theme/language/font settings, and speaker rename. Hiding the surface does not end the session. Ending hides it and retains the transcript until the next start. Navigation does not stop a session. The stream uses requestAnimationFrame with incremental text updates; history does not auto-scroll when the reader has scrolled up.

This browser prototype simulates the independent surface inside the page. Actual Windows always-on-top and reserved desktop area require native integration after approval. Audio, translations, costs and knowledge generation use samples; no cloud request or credential access occurs. Knowledge is stored in browser local storage and can be exported as JSON; this does not write the app's actual knowledge cache files.

Browser checks: start with main window retained, independent window show/hide, synchronized pause, drag-to-float, settings navigation while window remains visible, light theme synchronization, speaker rename, continued transcription after hiding the window. JavaScript syntax check passes.

Navigation revision:
- First launch uses a separate four-step onboarding flow, remembered locally; Home offers reconfiguration.
- Sidebar: Home, Knowledge Space, conditional Current Session, and Settings.
- Knowledge Space separates saved records, domain examples and an empty learning inbox. No real autonomous learning or summaries are implemented in this UI prototype.
- Home starts caption focus mode; Return restores the client. This is a browser simulation of hiding the client, not OS-level minimization.
- Ended sessions can explicitly be saved in browser local storage; recording audio remains off. Saved sample records survive reload.
- Verified in browser: onboarding generation completion, Home transition, focus/return, session end, record save and reload persistence.

Record reading workspace revision:
- Adds a clearly labeled curated design example with Overview, Minutes, Transcript/translation, and Timeline tabs. Existing captured sample records remain unsummarized; no summary is fabricated for them.
- Summary references and timeline topics select and scroll to transcript paragraphs. The source pane is collapsible. Transcript language can be original, translation, or both.
- Record cards show summary excerpts and status; title/summary search is functional.
- Language settings now distinguish source/target (including source auto-detection), original/translated/bilingual display, and original-first/translation-first order. The prototype catalog is illustrative, not a claim that a provider supports all languages. Caption samples remain English/Chinese, labeled in settings.
- Adds Usage navigation, a local sample duration aggregate and a cost breakdown layout. Billing and token totals remain unconnected.
- Verified: four record views, reference selection, transcript language toggle, non-Chinese target selection, original-only captions, and Usage navigation. No cloud requests made.

Record detail polish:
- Replaces the padded content page with a full-height reading desk and compact title/actions header.
- Default Minutes document contains summary, conclusions, a responsibility comparison table, insight cards and a process diagram. Original text occupies the wider right pane with higher contrast and larger type.
- Independent scrolling; draggable and keyboard-adjustable separator; original-text focus view; source search/filter/highlight; citation jumps; plain-text copy/export.
- The sample still has no audio recording; no nonfunctional audio player was added.
