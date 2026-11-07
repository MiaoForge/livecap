# Architecture

Livecap uses React and TypeScript for the interface and Rust for its Windows desktop services. The frontend invokes native commands through [clientApi.ts](../src/clientApi.ts) and receives session events from Tauri.

| Area                                                                          | Responsibility                                                                  |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| [Audio](../src-tauri/src/audio.rs)                                            | Captures audio from the default Windows playback device through WASAPI loopback |
| [Session](../src-tauri/src/session.rs)                                        | Coordinates capture, provider connections, translation, and session shutdown    |
| [OpenAI](../src-tauri/src/openai.rs) and [Soniox](../src-tauri/src/soniox.rs) | Convert provider responses into shared transcript events                        |
| [Caption state](../src/captions.ts)                                           | Applies transcript events and updates live captions                             |
| [Storage](../src-tauri/src/storage.rs)                                        | Stores settings, usage, knowledge, and meeting records in SQLite                |
| [Record analysis](../src-tauri/src/record_ai.rs)                              | Builds source-linked meeting notes, timeline entries, and diagram data          |
| [Knowledge preparation](../src-tauri/src/knowledge_generate.rs)               | Builds and validates source-backed terminology before saving the cache          |

The independent caption window receives caption state from the main window. Meeting audio is written to WAV files and replayed alongside transcript segments. The application stores API keys separately through Windows Credential Manager.

Knowledge preparation keeps its previous cache until a replacement passes validation. Usage accounting keeps knowledge preparation separate from live sessions and distinguishes known estimates from incomplete usage.
