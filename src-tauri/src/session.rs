use crate::{
    audio,
    knowledge::{Generation, KnowledgeCache, merge_asr_terms},
    knowledge_generate, luna, openai,
    records::{AudioArtifact, NewRecord, Recorder, RecordingsDirectory, SessionMode, WavWriter},
    soniox,
    storage::{Settings, Storage},
    terms::SessionTerms,
    transcript::{Provider, StreamEvent, TranslationStatus},
    usage::Meter,
};
use futures_util::FutureExt;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    sync::{mpsc, oneshot},
    time::timeout,
};
use tokio_tungstenite::tungstenite::http::HeaderValue;

#[derive(Default)]
pub struct Sessions(Mutex<Slot>);

#[derive(Default)]
struct Slot {
    active: Option<Active>,
    terms: Option<Arc<SessionTerms>>,
    closing: bool,
}

struct Active {
    stop: Option<oneshot::Sender<()>>,
    capture_stop: Arc<AtomicBool>,
    close_main: bool,
}

struct RunConfig {
    settings: Settings,
    terms: Arc<SessionTerms>,
    asr_terms: Vec<String>,
    knowledge: Option<KnowledgeCache>,
}

struct Archive {
    directory: Option<PathBuf>,
    recorder: Option<Arc<Recorder>>,
    meter: Meter,
    audio: Option<AudioArtifact>,
}

impl Archive {
    fn observe(&self, event: &StreamEvent) {
        if let Some(recorder) = &self.recorder {
            recorder.observe(event);
        }
    }
}

#[derive(Clone, Serialize)]
struct Status {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

fn status(app: &AppHandle, state: &'static str, message: Option<String>) {
    let _ = app.emit("session-status", Status { state, message });
}

pub fn authorization(key: &str) -> Result<HeaderValue, String> {
    const INVALID: &str = "API Key 格式无效。";
    if key.is_empty() || key.len() > 1024 || key.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err(INVALID.into());
    }
    let mut value = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| INVALID)?;
    value.set_sensitive(true);
    Ok(value)
}

async fn learn_from_meeting(
    app: &AppHandle,
    auth: HeaderValue,
    transcript: String,
) -> Result<usize, String> {
    let storage = app.state::<Storage>();
    if !storage.load_settings()?.auto_learn_meeting_terms {
        return Ok(0);
    }
    let generation = app.state::<Generation>();
    let mut ticket = generation.begin()?;
    let cache = storage.load_knowledge()?;
    let known = cache
        .iter()
        .flat_map(|cache| &cache.entries)
        .flat_map(|entry| std::iter::once(&entry.term).chain(&entry.aliases))
        .take(1_000)
        .cloned()
        .collect();
    let _ = app.emit("knowledge-learning", "正在自动查证本次会议的新术语…");
    let incoming = ticket
        .wait(async {
            timeout(
                std::time::Duration::from_secs(300),
                knowledge_generate::learn_meeting_terms(auth, transcript, known),
            )
            .await
            .map_err(|_| "会后术语补充超时，已发出的请求可能计费。")?
        })
        .await?;
    let count = ticket.commit(|| storage.append_meeting_knowledge(incoming))?;
    if count > 0 {
        let _ = app.emit("knowledge-updated", ());
    }
    let _ = app.emit("usage-updated", ());
    Ok(count)
}

impl Sessions {
    pub fn start(
        &self,
        app: AppHandle,
        settings: Settings,
        key: &str,
        session_id: String,
        knowledge: Option<KnowledgeCache>,
        meeting_topic: Option<&str>,
    ) -> Result<(), String> {
        let mode = if meeting_topic.is_some() {
            SessionMode::Meeting
        } else {
            SessionMode::Subtitle
        };
        let auth = authorization(key)?;
        let learn_auth = (mode == SessionMode::Meeting
            && settings.provider == Provider::Openai
            && settings.languages().supports_knowledge()
            && settings.auto_learn_meeting_terms)
            .then(|| auth.clone());
        let mut slot = self.0.lock().map_err(|_| "无法读取转写状态。")?;
        if slot.active.is_some() || slot.closing {
            return Err("当前已有转写会话。".into());
        }
        let created_at = app.state::<Storage>().record_timestamp()?;
        let (stop, stopped) = oneshot::channel();
        let capture_stop = Arc::new(AtomicBool::new(false));
        slot.active = Some(Active {
            stop: Some(stop),
            capture_stop: capture_stop.clone(),
            close_main: false,
        });
        let terms_app = app.clone();
        let asr_terms = merge_asr_terms(
            &settings.terms,
            knowledge.as_ref(),
            meeting_topic.unwrap_or_default(),
        );
        let terms = Arc::new(SessionTerms::new(session_id, &asr_terms, move |snapshot| {
            let _ = terms_app.emit("session-terms", snapshot);
        }));
        slot.terms = Some(terms.clone());
        terms.publish_initial();
        status(&app, "connecting", None);
        tauri::async_runtime::spawn(async move {
            let provider = settings.provider;
            let target_language = settings.target_language;
            let include_notes = settings.auto_generate_meeting_notes;
            let can_summarize = mode == SessionMode::Meeting
                && (provider == Provider::Openai
                    || crate::storage::api_key(Provider::Openai)
                        .ok()
                        .flatten()
                        .is_some());
            let mut archive = Archive {
                directory: (mode == SessionMode::Meeting)
                    .then(|| app.state::<RecordingsDirectory>().0.clone()),
                recorder: (mode == SessionMode::Meeting).then(|| Arc::new(Recorder::default())),
                meter: Meter::new(app.clone(), provider, &terms.session_id),
                audio: None,
            };
            let config = RunConfig {
                settings,
                terms,
                asr_terms,
                knowledge,
            };
            let mut result =
                guard_session_run(run(&app, config, auth, stopped, capture_stop, &mut archive))
                    .await;
            archive.meter.finish(result.is_err());
            let usage = archive.meter.snapshot();
            let mut learned_from = None;
            let mut summarize_id = None;
            if let Some(recorder) = archive.recorder {
                let (events, translation_error) = recorder.take();
                if archive.audio.is_some() || !events.is_empty() || result.is_err() {
                    let transcript = learn_auth.as_ref().and_then(|_| {
                        let text: String = events
                            .iter()
                            .flat_map(|frame| &frame.tokens)
                            .filter(|token| {
                                token.is_final
                                    && token.translation_status != TranslationStatus::Translation
                            })
                            .flat_map(|token| token.text.chars().chain(std::iter::once(' ')))
                            .take(28_000)
                            .collect();
                        (!text.trim().is_empty()).then_some(text)
                    });
                    let has_text = events.iter().flat_map(|event| &event.tokens).any(|token| {
                        token.is_final
                            && token.translation_status != TranslationStatus::Translation
                            && !token.text.trim().is_empty()
                    });
                    let record = NewRecord {
                        provider,
                        target_language,
                        created_at,
                        audio: archive.audio,
                        usage,
                        error: result.as_ref().err().cloned().or(translation_error),
                        events,
                    };
                    match app.state::<Storage>().save_record(record) {
                        Ok(id) => {
                            let _ = app.emit("records-updated", serde_json::json!({ "id": id }));
                            let _ = app.emit("usage-updated", ());
                            learned_from = transcript;
                            if can_summarize && has_text {
                                summarize_id = Some(id);
                            }
                        }
                        Err(error) => result = Err(error),
                    }
                }
            } else if let Some(usage) = usage {
                match app
                    .state::<Storage>()
                    .save_subtitle_usage(&created_at, usage)
                {
                    Ok(()) => {
                        let _ = app.emit("usage-updated", ());
                    }
                    Err(error) => result = Err(error),
                }
            }
            let sessions = app.state::<Sessions>();
            let mut close_main = false;
            if let Ok(mut slot) = sessions.0.lock() {
                close_main = slot.active.take().is_some_and(|active| active.close_main);
                match result {
                    Ok(()) => status(&app, "idle", None),
                    Err(message) => status(&app, "error", Some(message)),
                }
            }
            if close_main {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.destroy();
                }
                return;
            }
            if let Some(id) = summarize_id {
                let _ = app.emit(
                    "record-summary-status",
                    serde_json::json!({"id":id,"running":true}),
                );
                let outcome =
                    crate::summarize_record(&app, &app.state::<Storage>(), id, include_notes).await;
                let _ = app.emit(
                    "record-summary-status",
                    serde_json::json!({"id":id,"running":false,"error":outcome.err()}),
                );
            }
            if let (Some(auth), Some(transcript)) = (learn_auth, learned_from) {
                let outcome = learn_from_meeting(&app, auth, transcript).await;
                let message = match outcome {
                    Ok(0) => "本次会议没有新增可核实的术语。".into(),
                    Ok(count) => format!("已从本次会议补充 {count} 条术语。"),
                    Err(error) => format!("会后术语补充未完成。{error}"),
                };
                let _ = app.emit("knowledge-learning", message);
            }
        });
        Ok(())
    }

    pub fn stop(&self, app: &AppHandle) {
        if let Ok(mut slot) = self.0.lock() {
            if let Some(active) = slot.active.as_mut() {
                request_stop(app, active);
            } else {
                status(app, "idle", None);
            }
        }
    }

    pub fn close_when_stopped(&self, app: &AppHandle) -> bool {
        let Ok(mut slot) = self.0.lock() else {
            return false;
        };
        if slot.active.is_none() {
            return false;
        }
        slot.closing = true;
        let Some(active) = slot.active.as_mut() else {
            return false;
        };
        active.close_main = true;
        request_stop(app, active);
        true
    }

    pub fn dismiss_term(&self, session_id: &str, term: &str) -> Result<(), String> {
        let slot = self.0.lock().map_err(|_| "无法读取转写状态。")?;
        let terms = slot
            .terms
            .as_ref()
            .filter(|terms| terms.session_id == session_id)
            .ok_or("该会话的术语已失效。")?;
        terms.dismiss(term)
    }
}

// A failed provider/native task must still reach record persistence and slot cleanup.
// Tokio otherwise contains the panic in the detached task, leaving the UI stopping forever.
async fn guard_session_run(
    future: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    std::panic::AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            Err(
                "转写任务意外退出，已停止会话；已有会议内容将保留。请检查开发终端的错误日志。"
                    .into(),
            )
        })
}

fn request_stop(app: &AppHandle, active: &mut Active) {
    status(app, "stopping", None);
    if let Some(stop) = active.stop.take() {
        let _ = stop.send(());
        active.capture_stop.store(true, Ordering::Release);
    }
}

struct StopCapture(Arc<AtomicBool>);

impl Drop for StopCapture {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

async fn run(
    app: &AppHandle,
    config: RunConfig,
    auth: HeaderValue,
    mut stopped: oneshot::Receiver<()>,
    capture_stop: Arc<AtomicBool>,
    archive: &mut Archive,
) -> Result<(), String> {
    let RunConfig {
        mut settings,
        terms,
        asr_terms,
        knowledge,
    } = config;
    let session_id = terms.session_id.clone();
    let languages = settings.languages();
    // Both variants own a single similarly sized WebSocket.
    #[allow(clippy::large_enum_variant)]
    enum Connection {
        Soniox(soniox::Connection),
        Openai(openai::Connection, luna::Worker),
    }
    let (mut connection, sample_rate) = match settings.provider {
        Provider::Soniox => {
            settings.terms = asr_terms;
            let Some(socket) = soniox::Connection::open(
                soniox::ENDPOINT,
                &settings,
                auth,
                session_id,
                &mut stopped,
            )
            .await?
            else {
                return Ok(());
            };
            (Connection::Soniox(socket), soniox::SAMPLE_RATE)
        }
        Provider::Openai => {
            let Some(socket) = openai::Connection::open(
                openai::ENDPOINT,
                &asr_terms,
                auth.clone(),
                session_id,
                languages.source,
                &mut stopped,
            )
            .await?
            else {
                return Ok(());
            };
            let worker_app = app.clone();
            let worker_meter = archive.meter.clone();
            let worker_recorder = archive.recorder.clone();
            let translator = luna::Worker::start(
                luna::ENDPOINT,
                auth,
                settings.terms,
                terms,
                knowledge,
                languages,
                move |event| {
                    worker_meter.record(&event);
                    if let Some(recorder) = &worker_recorder {
                        recorder.observe(&event);
                    }
                    emit(&worker_app, &event);
                },
            )?;
            (Connection::Openai(socket, translator), openai::SAMPLE_RATE)
        }
    };
    let capture_guard = StopCapture(capture_stop.clone());
    let (ready, ready_rx) = oneshot::channel();
    let (tx, mut packets) = mpsc::channel(32);
    let directory = archive.directory.clone();
    let capture = tauri::async_runtime::spawn_blocking(move || {
        let mut writer = directory
            .as_deref()
            .map(|directory| WavWriter::create(directory, sample_rate))
            .transpose()?;
        let outcome = audio::capture(sample_rate, capture_stop, ready, tx, |packet| {
            if let Some(writer) = writer.as_mut() {
                writer.append(packet)?;
            }
            Ok(())
        });
        let artifact = writer.map(WavWriter::finish).transpose()?.flatten();
        Ok::<_, &'static str>((artifact, outcome))
    });

    let ready = tokio::select! {
        biased;
        _ = &mut stopped => Ok(false),
        result = timeout(soniox::IO_TIMEOUT, ready_rx) => {
            match result {
                Ok(Ok(())) => mark_listening(app).map(|()| true),
                Ok(Err(_)) => Err("无法启动系统音频采集。".into()),
                Err(_) => Err("启动系统音频采集超时。".into()),
            }
        }
    };
    let result = match ready {
        Ok(listening) => match &mut connection {
            Connection::Soniox(socket) => {
                if listening {
                    socket
                        .stream(&mut packets, &mut stopped, |event| {
                            archive.meter.record(&event);
                            archive.observe(&event);
                            emit(app, &event);
                        })
                        .await
                } else {
                    socket
                        .finish(&mut packets, false, |event| {
                            archive.meter.record(&event);
                            archive.observe(&event);
                            emit(app, &event);
                        })
                        .await
                }
            }
            Connection::Openai(socket, translator) => {
                let forward = |event| {
                    archive.meter.record(&event);
                    archive.observe(&event);
                    emit(app, &event);
                    if let Err(warning) = translator.submit(&event) {
                        let warning = StreamEvent::TranslationWarning(warning);
                        archive.observe(&warning);
                        emit(app, &warning);
                    }
                };
                if listening {
                    socket.stream(&mut packets, &mut stopped, &forward).await
                } else {
                    socket.finish(&mut packets, &forward).await
                }
            }
        },
        Err(error) => Err(error),
    };
    drop(capture_guard);
    // An OS-level audio call cannot be interrupted. Keep the session slot
    // occupied until the worker exits, with the provider connection already closed.
    match connection {
        Connection::Openai(socket, translator) => {
            drop(socket);
            if result.is_err() {
                status(app, "stopping", None);
            }
            if let Err(warning) = translator.finish().await {
                let warning = StreamEvent::TranslationWarning(warning);
                archive.observe(&warning);
                emit(app, &warning);
            }
        }
        Connection::Soniox(socket) => drop(socket),
    }
    let (artifact, capture_result) = capture.await.map_err(|_| "系统音频采集意外停止。")??;
    archive.audio = artifact;
    capture_result?;
    result
}

fn emit(app: &AppHandle, event: &StreamEvent) {
    match event {
        StreamEvent::Transcript(transcript) => {
            let _ = app.emit("transcript", transcript);
        }
        StreamEvent::AudioLevel(level) => {
            let _ = app.emit("audio-level", *level);
        }
        StreamEvent::TranslationWarning(warning) => {
            let _ = app.emit("translation-warning", warning);
        }
        StreamEvent::AudioSent(_)
        | StreamEvent::TranslationStarted
        | StreamEvent::TranslationUsage(_) => {}
    }
}

fn mark_listening(app: &AppHandle) -> Result<(), String> {
    let sessions = app.state::<Sessions>();
    let slot = sessions.0.lock().map_err(|_| "无法读取转写状态。")?;
    if slot
        .active
        .as_ref()
        .is_some_and(|active| active.stop.is_some())
    {
        status(app, "listening", None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panicked_session_cancels_capture_and_reaches_cleanup() {
        tauri::async_runtime::block_on(async {
            let stop = Arc::new(AtomicBool::new(false));
            let capture_stop = stop.clone();
            let result = guard_session_run(async move {
                let _capture = StopCapture(capture_stop);
                panic!("simulated provider task failure");
            })
            .await;
            assert!(result.unwrap_err().contains("转写任务意外退出"));
            assert!(stop.load(Ordering::Acquire));
        });
    }

    #[test]
    fn session_guard_preserves_normal_completion_and_errors() {
        tauri::async_runtime::block_on(async {
            assert!(guard_session_run(async { Ok(()) }).await.is_ok());
            assert_eq!(
                guard_session_run(async { Err("provider unavailable".into()) }).await,
                Err("provider unavailable".into())
            );
        });
    }
}
