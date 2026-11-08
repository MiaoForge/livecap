mod audio;
mod audio_feed;
mod knowledge;
mod knowledge_documents;
mod knowledge_generate;
mod language;
mod luna;
mod openai;
mod record_ai;
mod records;
mod session;
mod soniox;
mod storage;
mod terms;
mod tls;
mod transcript;
mod usage;

use knowledge::{Generation, KnowledgeCache, KnowledgeRequest};
use records::{
    AudioChunk, RecordDetail, RecordPatch, RecordSummary, RecordingsDirectory, SessionMode,
    StorageInfo, UsageEntry,
};
use session::Sessions;
use storage::{Settings, Storage, Transcript};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use transcript::Provider;

#[tauri::command]
fn app_update_info(app: AppHandle) -> serde_json::Value {
    serde_json::json!({ "version": app.package_info().version.to_string(), "configured": updater_configured(&app) })
}

fn updater_configured(app: &AppHandle) -> bool {
    app.config().plugins.0.get("updater").is_some_and(|value| {
        value
            .get("pubkey")
            .and_then(|key| key.as_str())
            .is_some_and(|key| !key.trim().is_empty())
            && value
                .get("endpoints")
                .and_then(|urls| urls.as_array())
                .is_some_and(|urls| !urls.is_empty())
    })
}

#[tauri::command]
fn load_settings(storage: State<'_, Storage>) -> Result<Settings, String> {
    storage.load_settings()
}

#[tauri::command]
fn save_settings(storage: State<'_, Storage>, settings: Settings) -> Result<(), String> {
    storage.save_settings(settings)
}

#[tauri::command]
fn has_api_key(provider: Provider) -> Result<bool, String> {
    storage::api_key(provider).map(|key| key.is_some())
}

#[tauri::command]
fn save_api_key(provider: Provider, key: String) -> Result<(), String> {
    let key = key.trim();
    session::authorization(key)?;
    storage::credential(provider)?
        .set_password(key)
        .map_err(|_| format!("无法保存 {} API Key。", provider.label()))
}

#[tauri::command]
fn delete_api_key(provider: Provider) -> Result<(), String> {
    match storage::credential(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(format!("无法删除 {} API Key。", provider.label())),
    }
}

#[tauri::command]
fn start_session(
    app: AppHandle,
    storage: State<'_, Storage>,
    consent: bool,
    provider: Provider,
    session_id: String,
    mode: Option<SessionMode>,
    meeting_topic: Option<String>,
) -> Result<(), String> {
    let mode = mode.unwrap_or_default();
    if !consent {
        return Err(match mode {
            SessionMode::Subtitle => "请确认同意将系统音频发送给所选服务。",
            SessionMode::Meeting => "请确认同意保存录音并将系统音频发送给所选服务。",
        }
        .into());
    }
    let settings = storage.load_settings()?;
    if settings.provider != provider {
        return Err("服务设置已更改，请重新确认后开始。".into());
    }
    if session_id.is_empty() || session_id.len() > 100 {
        return Err("会话标识无效。".into());
    }
    let key = storage::api_key(provider)?
        .ok_or_else(|| format!("请先保存 {} API Key。", provider.label()))?;
    let cache = if settings.target_language.supports_knowledge() {
        storage.load_knowledge()?.filter(|cache| cache.enabled)
    } else {
        None
    };
    let topic = meeting_topic.as_deref().unwrap_or_default().trim();
    if topic.chars().count() > 160 || topic.chars().any(char::is_control) {
        return Err("会议主题须为不超过 160 个字符的单行文字。".into());
    }
    let meeting_topic = (mode == SessionMode::Meeting).then_some(topic);
    audio::request_access()?;
    app.state::<Sessions>().start(
        app.clone(),
        settings,
        &key,
        session_id,
        cache,
        meeting_topic,
    )
}

#[tauri::command]
fn set_meeting_paused(app: AppHandle, paused: bool) -> Result<(), String> {
    app.state::<Sessions>().set_paused(&app, paused)
}

#[tauri::command]
fn stop_session(app: AppHandle, sessions: State<'_, Sessions>) {
    sessions.stop(&app);
}

#[tauri::command]
fn dismiss_learned_term(
    sessions: State<'_, Sessions>,
    session_id: String,
    term: String,
) -> Result<(), String> {
    sessions.dismiss_term(&session_id, &term)
}

#[tauri::command]
fn load_knowledge(storage: State<'_, Storage>) -> Result<Option<KnowledgeCache>, String> {
    storage.load_knowledge()
}

#[tauri::command]
async fn generate_knowledge(
    app: AppHandle,
    storage: State<'_, Storage>,
    generation: State<'_, Generation>,
    mut request: KnowledgeRequest,
    consent: bool,
) -> Result<KnowledgeCache, String> {
    if !consent {
        return Err("请确认同意发送领域、关注范围和参考链接以生成知识缓存。".into());
    }
    request.validate()?;
    let mut ticket = generation.begin()?;
    let key = storage::api_key(Provider::Openai)?.ok_or("请先保存 OpenAI API Key。")?;
    let authorization = session::authorization(&key)?;
    drop(key);
    let mut cache = ticket
        .wait(knowledge_generate::generate(
            luna::ENDPOINT,
            authorization,
            &request,
            |progress| {
                let _ = app.emit("knowledge-progress", progress);
            },
        ))
        .await?;
    cache.domain = request.domain;
    cache.focus = request.focus;
    let _ = app.emit(
        "knowledge-progress",
        knowledge_generate::KnowledgeProgress::new("saving", None, 0, 1),
    );
    let saved = ticket.commit(|| storage.replace_knowledge(cache))?;
    let _ = app.emit("usage-updated", ());
    Ok(saved)
}

#[tauri::command]
fn cancel_knowledge(generation: State<'_, Generation>) {
    generation.cancel();
}

#[tauri::command]
fn set_knowledge_enabled(
    storage: State<'_, Storage>,
    generation: State<'_, Generation>,
    enabled: bool,
) -> Result<KnowledgeCache, String> {
    generation.edit(|| {
        storage.edit_knowledge(|cache| cache.enabled = enabled && !cache.entries.is_empty())
    })
}

#[tauri::command]
fn remove_knowledge_entry(
    storage: State<'_, Storage>,
    generation: State<'_, Generation>,
    term: String,
) -> Result<KnowledgeCache, String> {
    let term = term.trim().to_lowercase();
    generation.edit(|| {
        storage.edit_knowledge(|cache| {
            cache
                .entries
                .retain(|entry| entry.term.trim().to_lowercase() != term);
            cache.enabled &= !cache.entries.is_empty();
        })
    })
}

#[tauri::command]
fn clear_knowledge(
    storage: State<'_, Storage>,
    generation: State<'_, Generation>,
) -> Result<(), String> {
    generation.edit(|| storage.clear_knowledge())
}

#[tauri::command]
fn open_knowledge_source(storage: State<'_, Storage>, url: String) -> Result<(), String> {
    let cache = storage.load_knowledge()?.ok_or("暂无知识缓存。")?;
    let url = cache
        .source_url(&url)
        .ok_or("该链接不是已保存的公开资料来源。")?;
    open_shell_item(std::ffi::OsStr::new(url.as_str()))
}

#[cfg(target_os = "windows")]
fn open_shell_item(item: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::{PCWSTR, w},
    };
    let item: Vec<u16> = item.encode_wide().chain(std::iter::once(0)).collect();
    // Callers supply one fixed local directory or validated HTTPS URI.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(item.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("无法打开所选位置。".into())
    }
}

#[cfg(target_os = "macos")]
fn open_shell_item(item: &std::ffi::OsStr) -> Result<(), String> {
    // Pass the validated URI or local path as one argument, without a shell.
    std::process::Command::new("/usr/bin/open")
        .arg(item)
        .status()
        .map_err(|_| "无法打开所选位置。".to_string())?
        .success()
        .then_some(())
        .ok_or_else(|| "无法打开所选位置。".into())
}

#[tauri::command]
fn save_transcript(storage: State<'_, Storage>, content: String) -> Result<i64, String> {
    storage.save_transcript(&content)
}

#[tauri::command]
fn list_transcripts(storage: State<'_, Storage>) -> Result<Vec<Transcript>, String> {
    storage.list_transcripts()
}

#[tauri::command]
fn delete_transcript(storage: State<'_, Storage>, id: i64) -> Result<(), String> {
    storage.delete_transcript(id)
}

#[tauri::command]
fn list_records(storage: State<'_, Storage>) -> Result<Vec<RecordSummary>, String> {
    storage.list_records()
}

#[tauri::command]
fn list_knowledge_usage(
    storage: State<'_, Storage>,
) -> Result<Vec<storage::KnowledgeUsageEntry>, String> {
    storage.list_knowledge_usage()
}

#[tauri::command]
fn list_usage(storage: State<'_, Storage>) -> Result<Vec<UsageEntry>, String> {
    storage.list_usage()
}

#[tauri::command]
fn storage_info(directory: State<'_, RecordingsDirectory>) -> Result<StorageInfo, String> {
    directory.info()
}

#[tauri::command]
fn open_storage_folder(directory: State<'_, RecordingsDirectory>) -> Result<(), String> {
    open_shell_item(directory.data_directory()?.as_os_str())
}

#[tauri::command]
fn get_record(storage: State<'_, Storage>, id: i64) -> Result<RecordDetail, String> {
    storage.get_record(id)
}

#[tauri::command]
fn update_record(
    storage: State<'_, Storage>,
    id: i64,
    patch: RecordPatch,
) -> Result<RecordDetail, String> {
    storage.update_record(id, patch)
}

#[tauri::command]
fn read_record_audio(
    storage: State<'_, Storage>,
    directory: State<'_, RecordingsDirectory>,
    id: i64,
    offset: u64,
    length: usize,
) -> Result<AudioChunk, String> {
    let name = storage.record_audio_file(id)?;
    records::read_audio(&directory.0, &name, offset, length)
}

#[tauri::command]
async fn generate_record_notes(
    app: AppHandle,
    storage: State<'_, Storage>,
    id: i64,
    consent: bool,
) -> Result<RecordDetail, String> {
    require_record_consent(consent)?;
    summarize_record(&app, &storage, id, true).await
}

pub(crate) async fn summarize_record(
    app: &AppHandle,
    storage: &Storage,
    id: i64,
    include_notes: bool,
) -> Result<RecordDetail, String> {
    let record = storage.get_record(id)?;
    let input = record_ai::Input::from_record(&record)?;
    let language = match record.target_language {
        Some(language) => language,
        None => storage.load_settings()?.target_language,
    };
    let auth = record_authorization()?;
    let (result, usage) =
        record_ai::summarize(luna::ENDPOINT, auth, input, language, include_notes).await;
    let generated = result
        .as_ref()
        .ok()
        .map(|insights| (&record, insights, language));
    let saved = storage.save_record_analysis(id, generated, usage);
    let _ = app.emit("records-updated", serde_json::json!({ "id": id }));
    let _ = app.emit("usage-updated", ());
    let saved = saved?;
    result.map(|_| saved)
}

#[tauri::command]
async fn ask_record(
    app: AppHandle,
    storage: State<'_, Storage>,
    id: i64,
    question: String,
    consent: bool,
) -> Result<record_ai::Answer, String> {
    require_record_consent(consent)?;
    let question = question.trim();
    if question.is_empty() || question.chars().count() > 1000 {
        return Err("问题需在 1 到 1000 字之间。".into());
    }
    let record = storage.get_record(id)?;
    let input = record_ai::Input::from_record(&record)?;
    let auth = record_authorization()?;
    let (result, usage) = record_ai::analyze(
        luna::ENDPOINT,
        auth,
        input,
        record_ai::Analysis::Question(question),
    )
    .await;
    let saved = storage.save_record_analysis(id, None, usage);
    let _ = app.emit("records-updated", serde_json::json!({ "id": id }));
    let _ = app.emit("usage-updated", ());
    saved?;
    result.map(|answer| record_ai::Answer { answer })
}

fn require_record_consent(consent: bool) -> Result<(), String> {
    if consent {
        Ok(())
    } else {
        Err("请先确认同意将记录文字发送给 OpenAI。".into())
    }
}

fn record_authorization() -> Result<tokio_tungstenite::tungstenite::http::HeaderValue, String> {
    let key = storage::api_key(Provider::Openai)?.ok_or("请先保存 OpenAI API Key。")?;
    session::authorization(&key)
}

pub fn run() {
    tls::initialize();
    tauri::Builder::default()
        .manage(Sessions::default())
        .manage(Generation::default())
        .setup(|app| {
            if updater_configured(app.handle()) {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            let directory = app.path().app_local_data_dir()?;
            let recordings = directory.join("recordings");
            std::fs::create_dir_all(&recordings)?;
            app.manage(RecordingsDirectory(recordings));
            app.manage(Storage::open(&directory.join("livecap.sqlite3"))?);
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            ("captions", WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                let _ = window.hide();
            }
            ("main", WindowEvent::CloseRequested { api, .. }) => {
                window.state::<Generation>().cancel();
                if window
                    .state::<Sessions>()
                    .close_when_stopped(window.app_handle())
                {
                    api.prevent_close();
                }
            }
            ("main", WindowEvent::Destroyed) => {
                window.state::<Generation>().cancel();
                window.state::<Sessions>().stop(window.app_handle());
                if let Some(captions) = window.app_handle().get_webview_window("captions") {
                    let _ = captions.destroy();
                }
            }
            _ => (),
        })
        .invoke_handler(tauri::generate_handler![
            app_update_info,
            load_settings,
            save_settings,
            has_api_key,
            save_api_key,
            delete_api_key,
            start_session,
            stop_session,
            set_meeting_paused,
            dismiss_learned_term,
            load_knowledge,
            generate_knowledge,
            cancel_knowledge,
            set_knowledge_enabled,
            remove_knowledge_entry,
            clear_knowledge,
            open_knowledge_source,
            save_transcript,
            list_transcripts,
            delete_transcript,
            list_records,
            list_usage,
            list_knowledge_usage,
            storage_info,
            open_storage_folder,
            get_record,
            update_record,
            read_record_audio,
            generate_record_notes,
            ask_record,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Livecap");
}
