use crate::{
    audio,
    storage::Settings,
    transcript::{Provider, StreamEvent, TranscriptEvent, TranscriptToken, TranslationStatus},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Error as WebSocketError, Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
    },
};

pub const ENDPOINT: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
pub const SAMPLE_RATE: u32 = 16_000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
pub const IO_TIMEOUT: Duration = Duration::from_secs(3);
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_ERROR: &str = "与 Soniox 的连接已中断，请检查网络后重新开始。";
const RESPONSE_ERROR: &str = "Soniox 返回了无法解析的转写结果。";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn config(settings: &Settings) -> Value {
    json!({
        "model": settings.model,
        "audio_format": "pcm_s16le",
        "sample_rate": SAMPLE_RATE,
        "num_channels": 1,
        "enable_language_identification": true,
        "enable_speaker_diarization": true,
        "translation": { "type": "one_way", "target_language": settings.target_language.code() },
        "context": {
            "terms": settings.terms,
            "translation_terms": settings.terms.iter()
                .map(|term| json!({ "source": term, "target": term }))
                .collect::<Vec<_>>(),
        },
    })
}

fn service_error(code: u16) -> String {
    match code {
        400 => "Soniox 拒绝了转写请求，请检查模型和术语设置。".into(),
        401 | 403 => "Soniox 拒绝了认证，请检查 API Key 和账户权限。".into(),
        402 => "Soniox 账户余额不足。".into(),
        429 => "Soniox 请求超限，请检查账户用量或并发限制。".into(),
        _ => format!("Soniox 转写失败（{code}）。"),
    }
}

#[derive(Deserialize)]
struct SonioxResponse {
    tokens: Option<Vec<SonioxToken>>,
    #[serde(default)]
    finished: bool,
    error_code: Option<u16>,
}

#[derive(Deserialize)]
struct SonioxToken {
    text: String,
    is_final: bool,
    speaker: Option<String>,
    language: Option<String>,
    source_language: Option<String>,
    start_ms: Option<u64>,
    end_ms: Option<u64>,
    #[serde(default)]
    translation_status: TranslationStatus,
}

fn normalize(text: &str, session_id: &str) -> Result<TranscriptEvent, String> {
    let response: SonioxResponse = serde_json::from_str(text).map_err(|_| RESPONSE_ERROR)?;
    if let Some(code) = response.error_code {
        return Err(service_error(code));
    }
    if response.tokens.is_none() && !response.finished {
        return Err(RESPONSE_ERROR.into());
    }
    let tokens = response
        .tokens
        .unwrap_or_default()
        .into_iter()
        .map(|token| TranscriptToken {
            text: token.text,
            is_final: token.is_final,
            speaker: token.speaker,
            language: token.language,
            source_language: token.source_language,
            start_ms: token.start_ms,
            end_ms: token.end_ms,
            translation_status: token.translation_status,
            translation_source: (token.translation_status == TranslationStatus::Translation)
                .then_some(Provider::Soniox),
            source_segment_id: None,
            revision: None,
        })
        .collect();
    Ok(TranscriptEvent {
        session_id: session_id.into(),
        tokens,
        finished: response.finished,
    })
}

pub struct Connection {
    socket: Socket,
    session_id: String,
}

impl Connection {
    pub async fn open(
        endpoint: &str,
        settings: &Settings,
        auth: HeaderValue,
        session_id: String,
        stopped: &mut oneshot::Receiver<()>,
    ) -> Result<Option<Self>, String> {
        let mut request = endpoint
            .into_client_request()
            .map_err(|_| "无法创建 Soniox 请求。")?;
        request.headers_mut().insert(AUTHORIZATION, auth);
        let socket = tokio::select! {
            biased;
            _ = &mut *stopped => return Ok(None),
            result = timeout(CONNECT_TIMEOUT, connect_async(request)) => {
                match result {
                    Ok(Ok((socket, _))) => socket,
                    Ok(Err(WebSocketError::Http(response))) => return Err(service_error(response.status().as_u16())),
                    Ok(Err(_)) => return Err("无法连接 Soniox，请检查网络。".into()),
                    Err(_) => return Err("连接 Soniox 超时。".into()),
                }
            }
        };
        let mut connection = Self { socket, session_id };
        tokio::select! {
            biased;
            _ = &mut *stopped => return Ok(None),
            result = connection.send(Message::Text(config(settings).to_string().into())) => result?,
        }
        Ok(Some(connection))
    }

    async fn send(&mut self, message: Message) -> Result<(), String> {
        timeout(IO_TIMEOUT, self.socket.send(message))
            .await
            .map_err(|_| "向 Soniox 发送数据超时。")?
            .map_err(|_| NETWORK_ERROR.into())
    }

    fn receive(
        session_id: &str,
        message: Option<Result<Message, WebSocketError>>,
        emit: &mut impl FnMut(StreamEvent),
    ) -> Result<bool, String> {
        match message {
            Some(Ok(Message::Text(text))) => {
                let event = normalize(text.as_str(), session_id)?;
                let finished = event.finished;
                emit(StreamEvent::Transcript(event));
                Ok(finished)
            }
            None | Some(Ok(Message::Close(_))) | Some(Err(_)) => Err(NETWORK_ERROR.into()),
            _ => Ok(false),
        }
    }

    pub async fn stream(
        &mut self,
        packets: &mut mpsc::Receiver<Vec<u8>>,
        stopped: &mut oneshot::Receiver<()>,
        mut emit: impl FnMut(StreamEvent),
    ) -> Result<(), String> {
        let mut sent_audio = false;
        let level_interval = Duration::from_millis(80);
        let mut last_level = Instant::now() - level_interval;
        loop {
            tokio::select! {
                biased;
                _ = &mut *stopped => break,
                response = self.socket.next() => {
                    if Self::receive(&self.session_id, response, &mut emit)? {
                        return Ok(());
                    }
                }
                packet = packets.recv() => {
                    let Some(packet) = packet else {
                        return Err("系统音频采集已停止。".into());
                    };
                    if last_level.elapsed() >= level_interval {
                        emit(StreamEvent::AudioLevel(audio::level(&packet)));
                        last_level = Instant::now();
                    }
                    let samples = packet.len() / 2;
                    self.send(Message::Binary(packet.into())).await?;
                    emit(StreamEvent::AudioSent(samples));
                    sent_audio = true;
                }
            }
        }
        self.finish(packets, sent_audio, emit).await
    }

    pub async fn finish(
        &mut self,
        packets: &mut mpsc::Receiver<Vec<u8>>,
        mut sent_audio: bool,
        mut emit: impl FnMut(StreamEvent),
    ) -> Result<(), String> {
        timeout(FINISH_TIMEOUT, async {
            packets.close();
            while let Some(packet) = packets.recv().await {
                let samples = packet.len() / 2;
                self.send(Message::Binary(packet.into())).await?;
                emit(StreamEvent::AudioSent(samples));
                sent_audio = true;
            }
            // Soniox expects at least one audio frame, even if the user stops immediately.
            if !sent_audio {
                self.send(Message::Binary(vec![0; 320].into())).await?;
                emit(StreamEvent::AudioSent(160));
            }
            self.send(Message::Text("".into())).await?;
            loop {
                let response = self.socket.next().await;
                if Self::receive(&self.session_id, response, &mut emit)? {
                    return Ok(());
                }
            }
        })
        .await
        .unwrap_or_else(|_| Err("等待 Soniox 返回最后一段字幕超时。".into()))
    }
}

#[cfg(test)]
#[path = "soniox_tests.rs"]
mod tests;
