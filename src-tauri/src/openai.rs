use crate::{
    audio,
    audio_feed::Packet,
    transcript::{StreamEvent, TranscriptEvent, TranscriptToken, TranslationStatus},
};
use base64::{Engine, engine::general_purpose::STANDARD};

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Error, Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};

pub const ENDPOINT: &str = "wss://api.openai.com/v1/realtime?intent=transcription";
pub const SAMPLE_RATE: u32 = 24_000;
const IO_TIMEOUT: Duration = Duration::from_secs(3);
const OPEN_TIMEOUT: Duration = Duration::from_secs(12);
const FINISH_TIMEOUT: Duration = Duration::from_secs(8);
const NETWORK_ERROR: &str = "与 OpenAI 实时转录的连接已中断。";
const INVALID_RESPONSE: &str = "OpenAI 返回了无效的转录事件。";
const MIN_SAMPLES: usize = SAMPLE_RATE as usize / 10;
const QUIET_SAMPLES: usize = SAMPLE_RATE as usize * 1200 / 1000;
const SENTENCE_QUIET_SAMPLES: usize = SAMPLE_RATE as usize * 450 / 1000;
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 15;
const MAX_PENDING: usize = 8;
const PRE_ROLL_BYTES: usize = SAMPLE_RATE as usize * 2 / 5;
const RECENT_COMPLETIONS: usize = 32;
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
const ACTIVITY_THRESHOLD: f32 = 0.003;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub fn valid_keyword(term: &str) -> bool {
    !term.contains(['<', '>', '\r', '\n'])
}

fn config(terms: &[String]) -> Value {
    let keywords: Vec<_> = terms.iter().filter(|term| valid_keyword(term)).collect();
    json!({
        "type": "session.update",
        "session": {
            "type": "transcription",
            "audio": {
                "input": {
                    "format": { "type": "audio/pcm", "rate": SAMPLE_RATE },
                    "transcription": {
                        "model": "gpt-live-transcribe",
                        "keywords": keywords,
                        "delay": "low",
                    },
                    "turn_detection": null,
                },
            },
        },
    })
}

struct Turn {
    start: u64,
    samples: usize,
    quiet: usize,
}

impl Turn {
    fn add_packet(&mut self, samples: usize, active: bool, sentence_end: bool) -> bool {
        self.samples += samples;
        self.quiet = if active { 0 } else { self.quiet + samples };
        let quiet_limit = if sentence_end {
            SENTENCE_QUIET_SAMPLES
        } else {
            QUIET_SAMPLES
        };
        self.quiet >= quiet_limit || self.samples >= MAX_SAMPLES
    }
}

fn ends_sentence(text: &str) -> bool {
    let text = text.trim_end_matches(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '”' | '’' | ')' | '）' | ']' | '】' | '」' | '』'
            )
    });
    !text.ends_with("...") && text.ends_with(['.', '!', '?', '。', '！', '？'])
}

struct Pending {
    id: Option<String>,
    final_text: Option<(String, u32)>,
    start: u64,
    end: u64,
    since: Instant,
}

#[derive(Default)]
struct Draft {
    text: String,
    revision: u32,
}

fn current_draft_ends_sentence(
    drafts: &HashMap<String, Draft>,
    pending: &VecDeque<Pending>,
) -> bool {
    // A draft can still refer to a committed turn while its acknowledgement is in flight.
    if pending.iter().any(|turn| turn.id.is_none()) {
        return false;
    }
    let mut current = drafts
        .iter()
        .filter(|(id, _)| !pending.iter().any(|turn| turn.id.as_ref() == Some(*id)));
    match (current.next(), current.next()) {
        (Some((_, draft)), None) => ends_sentence(&draft.text),
        _ => false,
    }
}

pub struct Connection {
    socket: Socket,
    session_id: String,
    position: u64,
    pre_roll: Vec<u8>,
    turn: Option<Turn>,
    pending: VecDeque<Pending>,
    drafts: HashMap<String, Draft>,
    completed: VecDeque<String>,
    last_level: Instant,
}

fn transcription_error(response: &Value) -> String {
    let error = &response["error"];
    let code = error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .unwrap_or("");
    let explanation = match code {
        "invalid_api_key" | "authentication_error" => "OpenAI 认证失败，请在设置中检查 API Key。",
        "insufficient_quota" | "billing_hard_limit_reached" => {
            "OpenAI 账户可用额度不足，请检查账户余额和项目预算。"
        }
        "rate_limit_exceeded" => "OpenAI 请求速率达到限制，请稍后重试。",
        "model_not_found" | "permission_denied" | "insufficient_permissions" => {
            "OpenAI 模型不可用或项目无访问权限，请检查模型权限。"
        }
        "invalid_request_error"
        | "invalid_value"
        | "unknown_parameter"
        | "missing_required_parameter" => "OpenAI 拒绝了实时转录配置。",
        "server_error" | "internal_server_error" => "OpenAI 转录服务暂时异常，请稍后重试。",
        "audio_unintelligible" => "该段音频无法识别。",
        _ => "OpenAI 实时转录失败，请根据错误代码检查服务配置或稍后重试。",
    };
    // Surface structured diagnostics, never upstream free text that may echo credentials/audio.
    let safe = |value: &str| {
        !value.is_empty()
            && value.len() <= 100
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.[]".contains(&c))
    };
    let mut message = explanation.to_owned();
    if safe(code) {
        message.push_str(&format!("（代码：{code}）"));
    }
    if let Some(param) = error["param"].as_str().filter(|param| safe(param)) {
        message.push_str(&format!(" 参数：{param}"));
    }
    message
}

impl Connection {
    pub async fn open(
        endpoint: &str,
        terms: &[String],
        auth: HeaderValue,
        session_id: String,
        stopped: &mut oneshot::Receiver<()>,
    ) -> Result<Option<Self>, String> {
        tokio::select! {
            biased;
            _ = &mut *stopped => Ok(None),
            result = timeout(OPEN_TIMEOUT, Self::handshake(endpoint, terms, auth, session_id)) => {
                result.map_err(|_| "连接 OpenAI 实时转录超时。")?.map(Some)
            }
        }
    }

    async fn handshake(
        endpoint: &str,
        terms: &[String],
        auth: HeaderValue,
        session_id: String,
    ) -> Result<Self, String> {
        let mut request = endpoint.into_client_request().map_err(|_| NETWORK_ERROR)?;
        request.headers_mut().insert(AUTHORIZATION, auth);
        let (socket, _) = connect_async(request).await.map_err(|error| match error {
            Error::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
                "OpenAI 拒绝了认证，请检查 API Key 和模型权限。"
            }
            Error::Http(response) if response.status().as_u16() == 429 => {
                "OpenAI 实时转录请求超限。"
            }
            _ => NETWORK_ERROR,
        })?;
        let mut connection = Self {
            socket,
            session_id,
            position: 0,
            pre_roll: Vec::new(),
            turn: None,
            pending: VecDeque::new(),
            drafts: HashMap::new(),
            completed: VecDeque::new(),
            last_level: Instant::now() - Duration::from_millis(80),
        };
        connection.send(config(terms)).await?;
        loop {
            let response = parse(connection.socket.next().await)?;
            match response["type"].as_str() {
                Some("session.updated" | "transcription_session.updated") => return Ok(connection),
                Some("error") => {
                    return Err(transcription_error(&response));
                }
                _ => (),
            }
        }
    }

    async fn send(&mut self, event: Value) -> Result<(), String> {
        timeout(
            IO_TIMEOUT,
            self.socket.send(Message::Text(event.to_string().into())),
        )
        .await
        .map_err(|_| "向 OpenAI 发送数据超时。")?
        .map_err(|_| NETWORK_ERROR.into())
    }

    async fn append(&mut self, pcm: &[u8], emit: &impl Fn(StreamEvent)) -> Result<(), String> {
        self.send(json!({ "type": "input_audio_buffer.append", "audio": STANDARD.encode(pcm) }))
            .await?;
        emit(StreamEvent::AudioSent(pcm.len() / 2));
        Ok(())
    }

    async fn packet(&mut self, pcm: Vec<u8>, emit: &impl Fn(StreamEvent)) -> Result<(), String> {
        let level = audio::level(&pcm);
        if self.last_level.elapsed() >= Duration::from_millis(80) {
            emit(StreamEvent::AudioLevel(level));
            self.last_level = Instant::now();
        }
        if self
            .pending
            .front()
            .is_some_and(|turn| turn.since.elapsed() > Duration::from_secs(20))
        {
            return Err("等待 OpenAI 返回最终转录超时。".into());
        }
        let samples = pcm.len() / 2;
        let active = level >= ACTIVITY_THRESHOLD;
        let start = self.position;
        self.position += samples as u64;
        if self.turn.is_none() {
            if !active {
                self.pre_roll.extend_from_slice(&pcm);
                let excess = self.pre_roll.len().saturating_sub(PRE_ROLL_BYTES);
                self.pre_roll.drain(..excess);
                return Ok(());
            }
            let pre_roll = std::mem::take(&mut self.pre_roll);
            let pre_samples = pre_roll.len() / 2;
            if !pre_roll.is_empty() {
                self.append(&pre_roll, emit).await?;
            }
            self.turn = Some(Turn {
                start: start.saturating_sub(pre_samples as u64),
                samples: pre_samples,
                quiet: 0,
            });
        }
        self.append(&pcm, emit).await?;
        let sentence_end = !active && current_draft_ends_sentence(&self.drafts, &self.pending);
        if self
            .turn
            .as_mut()
            .is_some_and(|turn| turn.add_packet(samples, active, sentence_end))
        {
            self.commit(emit).await?;
        }
        Ok(())
    }

    async fn commit(&mut self, emit: &impl Fn(StreamEvent)) -> Result<(), String> {
        let Some(turn) = self.turn.take() else {
            return Ok(());
        };
        if self.pending.len() >= MAX_PENDING {
            return Err("OpenAI 转录积压，已停止采集。".into());
        }
        if turn.samples < MIN_SAMPLES {
            self.append(&vec![0; (MIN_SAMPLES - turn.samples) * 2], emit)
                .await?;
        }
        self.send(json!({"type": "input_audio_buffer.commit"}))
            .await?;
        self.pending.push_back(Pending {
            id: None,
            final_text: None,
            start: turn.start,
            end: self.position,
            since: Instant::now(),
        });
        Ok(())
    }

    fn receive(&mut self, response: Value, emit: &impl Fn(StreamEvent)) -> Result<(), String> {
        match response["type"].as_str() {
            Some("conversation.item.input_audio_transcription.failed")
                if response["error"]["code"] == "audio_unintelligible" =>
            {
                let id = field(&response, "item_id", 256)?;
                if self
                    .pending
                    .iter()
                    .any(|turn| turn.id.as_deref() == Some(id))
                {
                    emit(StreamEvent::TranslationWarning(
                        "一段音频无法识别，已跳过，会议继续。".into(),
                    ));
                    return self.receive(
                        json!({
                            "type": "conversation.item.input_audio_transcription.completed",
                            "item_id": id,
                            "transcript": ""
                        }),
                        emit,
                    );
                }
            }
            Some("error" | "conversation.item.input_audio_transcription.failed") => {
                return Err(transcription_error(&response));
            }
            Some("input_audio_buffer.committed") => {
                let id = field(&response, "item_id", 256)?;
                if let Some(turn) = self.pending.iter_mut().find(|turn| turn.id.is_none()) {
                    turn.id = Some(id.into());
                }
            }
            Some("conversation.item.input_audio_transcription.delta") => {
                let id = field(&response, "item_id", 256)?;
                if self.completed.iter().any(|completed| completed == id) {
                    return Ok(());
                }
                let delta = field(&response, "delta", MAX_TRANSCRIPT_BYTES)?;
                if !self.drafts.contains_key(id) && self.drafts.len() > MAX_PENDING {
                    return Err(INVALID_RESPONSE.into());
                }
                let draft = self.drafts.entry(id.into()).or_default();
                if draft.text.len() + delta.len() > MAX_TRANSCRIPT_BYTES {
                    return Err(INVALID_RESPONSE.into());
                }
                draft.text.push_str(delta);
                draft.revision += 1;
                emit(StreamEvent::Transcript(transcript(
                    &self.session_id,
                    id.into(),
                    draft.text.clone(),
                    draft.revision,
                    None,
                )));
            }
            Some("conversation.item.input_audio_transcription.completed") => {
                let id = field(&response, "item_id", 256)?;
                if self.completed.iter().any(|completed| completed == id) {
                    return Ok(());
                }
                let text = field(&response, "transcript", MAX_TRANSCRIPT_BYTES)?;
                let revision = self.drafts.remove(id).map_or(1, |draft| draft.revision + 1);
                if let Some(turn) = self
                    .pending
                    .iter_mut()
                    .find(|turn| turn.id.as_deref() == Some(id))
                {
                    turn.final_text = Some((text.trim().to_owned(), revision));
                    if self.completed.len() == RECENT_COMPLETIONS {
                        self.completed.pop_front();
                    }
                    self.completed.push_back(id.into());
                }
                while self
                    .pending
                    .front()
                    .is_some_and(|turn| turn.final_text.is_some())
                {
                    let mut turn = self.pending.pop_front().expect("front checked");
                    let id = turn.id.take().expect("completion matched by item ID");
                    let (text, revision) = turn.final_text.take().expect("completion checked");
                    emit(StreamEvent::Transcript(transcript(
                        &self.session_id,
                        id,
                        text,
                        revision,
                        Some(turn),
                    )));
                }
            }
            _ => (),
        }
        Ok(())
    }

    async fn ingest(&mut self, packet: Packet, emit: &impl Fn(StreamEvent)) -> Result<(), String> {
        match packet {
            Packet::Pcm(pcm) => self.packet(pcm, emit).await,
            Packet::Paused => {
                self.commit(emit).await?;
                self.pre_roll.clear();
                Ok(())
            }
            Packet::Resumed => {
                self.pre_roll.clear();
                Ok(())
            }
        }
    }

    pub async fn stream(
        &mut self,
        packets: &mut mpsc::Receiver<Packet>,
        stopped: &mut oneshot::Receiver<()>,
        emit: &impl Fn(StreamEvent),
    ) -> Result<(), String> {
        let mut keepalive = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(5),
            Duration::from_secs(5),
        );
        loop {
            tokio::select! {
                biased;
                _ = &mut *stopped => return self.finish(packets, emit).await,
                response = self.socket.next() => self.receive(parse(response)?, emit)?,
                _ = keepalive.tick() => {
                    timeout(IO_TIMEOUT, self.socket.send(Message::Ping(Vec::new().into())))
                        .await.map_err(|_| "OpenAI 连接保活超时。")?
                        .map_err(|_| NETWORK_ERROR)?;
                    if self.pending.front().is_some_and(|turn| turn.since.elapsed() > Duration::from_secs(20)) {
                        return Err("等待 OpenAI 返回最终转录超时。".into());
                    }
                }
                packet = packets.recv() => {
                    self.ingest(packet.ok_or("系统音频采集已停止。")?, emit).await?;
                }
            }
        }
    }

    pub async fn finish(
        &mut self,
        packets: &mut mpsc::Receiver<Packet>,
        emit: &impl Fn(StreamEvent),
    ) -> Result<(), String> {
        timeout(FINISH_TIMEOUT, async {
            packets.close();
            while let Some(packet) = packets.recv().await {
                self.ingest(packet, emit).await?;
            }
            self.commit(emit).await?;
            while !self.pending.is_empty() {
                let response = parse(self.socket.next().await)?;
                self.receive(response, emit)?;
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("等待 OpenAI 返回最后一段转录超时。".into()))
    }
}

fn parse(message: Option<Result<Message, Error>>) -> Result<Value, String> {
    match message {
        Some(Ok(Message::Text(text))) => {
            let event: Value = serde_json::from_str(text.as_str()).map_err(|_| INVALID_RESPONSE)?;
            if event["type"].is_string() {
                Ok(event)
            } else {
                Err(INVALID_RESPONSE.into())
            }
        }
        Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(Value::Null),
        _ => Err(NETWORK_ERROR.into()),
    }
}

fn field<'a>(event: &'a Value, name: &str, max_len: usize) -> Result<&'a str, String> {
    event[name]
        .as_str()
        .filter(|text| text.len() <= max_len)
        .ok_or_else(|| INVALID_RESPONSE.into())
}

fn transcript(
    session_id: &str,
    id: String,
    text: String,
    revision: u32,
    turn: Option<Pending>,
) -> TranscriptEvent {
    let time = |samples: u64| samples * 1000 / u64::from(SAMPLE_RATE);
    TranscriptEvent {
        session_id: session_id.into(),
        tokens: vec![TranscriptToken {
            text,
            is_final: turn.is_some(),
            speaker: None,
            language: None,
            source_language: None,
            start_ms: turn.as_ref().map(|turn| time(turn.start)),
            end_ms: turn.as_ref().map(|turn| time(turn.end)),
            translation_status: TranslationStatus::Original,
            translation_source: None,
            source_segment_id: Some(id),
            revision: Some(revision),
        }],
        finished: false,
    }
}
