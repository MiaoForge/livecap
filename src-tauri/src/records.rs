use crate::{
    language::Language,
    transcript::{Provider, StreamEvent, TranscriptEvent},
    usage::Snapshot,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use uuid::Uuid;

const WAV_HEADER: u64 = 44;
const MAX_AUDIO_CHUNK: usize = 1024 * 1024;
const AUDIO_ERROR: &str = "无法保存录音文件，请检查磁盘空间与访问权限。";

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    #[default]
    Subtitle,
    Meeting,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub id: String,
    pub record_id: Option<i64>,
    pub mode: SessionMode,
    pub created_at: String,
    pub usage: Snapshot,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    Complete,
    Partial,
}

impl RecordStatus {
    pub fn from_error(error: &Option<String>) -> Self {
        if error.is_some() {
            Self::Partial
        } else {
            Self::Complete
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordSummary {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub duration_ms: u64,
    pub provider: Option<Provider>,
    pub status: RecordStatus,
    pub audio_available: bool,
    pub error: Option<String>,
    pub usage: Option<Snapshot>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDetail {
    #[serde(flatten)]
    pub summary: RecordSummary,
    pub notes: String,
    #[serde(default)]
    pub target_language: Option<Language>,
    #[serde(default)]
    pub timeline: Vec<TimelinePoint>,
    #[serde(default)]
    pub note_sources: Vec<Option<u64>>,
    #[serde(default)]
    pub diagram: Option<RecordDiagram>,
    pub transcript_text: Option<String>,
    pub transcript_events: Vec<TranscriptEvent>,
    pub legacy_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_edits: Option<BTreeMap<String, SegmentEdit>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelinePoint {
    pub start_ms: Option<u64>,
    pub title: String,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramNode {
    pub id: String,
    pub label: String,
    pub start_ms: Option<u64>,
}

#[derive(Deserialize, Serialize)]
pub struct DiagramEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

#[derive(Deserialize, Serialize)]
pub struct RecordDiagram {
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
}

pub struct RecordInsights {
    pub title: String,
    pub notes: Option<String>,
    pub timeline: Vec<TimelinePoint>,
    pub diagram: Option<RecordDiagram>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentEdit {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub transcript_text: Option<String>,
    pub segment_edits: Option<BTreeMap<String, SegmentEdit>>,
}

impl RecordPatch {
    pub fn validate(&mut self) -> Result<(), String> {
        if let Some(edits) = &self.segment_edits {
            if self.transcript_text.is_none() {
                return Err("保存段落校对时需同时提供校对后的原文。".into());
            }
            if edits.len() > 500
                || edits.iter().any(|(id, edit)| {
                    id.is_empty()
                        || id.len() > 256
                        || edit.text.len() > 64 * 1024
                        || edit
                            .speaker
                            .as_ref()
                            .is_some_and(|speaker| speaker.chars().count() > 100)
                })
                || edits.values().map(|edit| edit.text.len()).sum::<usize>() > 4 * 1024 * 1024
            {
                return Err("段落校对超过可保存的长度。".into());
            }
        }
        if let Some(title) = self.title.as_mut() {
            *title = title.trim().into();
            if title.is_empty() || title.chars().count() > 120 {
                return Err("记录标题需在 1 到 120 字之间。".into());
            }
        }
        if self
            .notes
            .as_ref()
            .is_some_and(|text| text.len() > 1024 * 1024)
            || self
                .transcript_text
                .as_ref()
                .is_some_and(|text| text.len() > 4 * 1024 * 1024)
        {
            return Err("编辑内容超过可保存的长度。".into());
        }
        Ok(())
    }
}

pub struct NewRecord {
    pub provider: Provider,
    pub target_language: Language,
    pub created_at: String,
    pub audio: Option<AudioArtifact>,
    pub error: Option<String>,
    pub usage: Option<Snapshot>,
    pub events: Vec<TranscriptEvent>,
}

#[derive(Default)]
pub struct Recorder(Mutex<(Vec<TranscriptEvent>, Option<String>)>);

impl Recorder {
    pub fn observe(&self, event: &StreamEvent) {
        let Ok(mut state) = self.0.lock() else { return };
        match event {
            StreamEvent::Transcript(event) => {
                let tokens: Vec<_> = event
                    .tokens
                    .iter()
                    .filter(|token| token.is_final)
                    .cloned()
                    .collect();
                if !tokens.is_empty() {
                    state.0.push(TranscriptEvent {
                        session_id: event.session_id.clone(),
                        tokens,
                        finished: event.finished,
                    });
                }
            }
            StreamEvent::TranslationWarning(warning) => {
                state.1.get_or_insert_with(|| warning.clone());
            }
            _ => (),
        }
    }

    pub fn take(&self) -> (Vec<TranscriptEvent>, Option<String>) {
        self.0
            .lock()
            .map(|mut state| std::mem::take(&mut *state))
            .unwrap_or_default()
    }
}

pub struct AudioArtifact {
    pub file_name: String,
    pub duration_ms: u64,
}

pub struct RecordingsDirectory(pub PathBuf);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    data_directory: String,
    database_path: String,
    recordings_directory: String,
    database_bytes: u64,
    recording_count: u64,
    recording_bytes: u64,
    total_bytes: u64,
}

impl RecordingsDirectory {
    pub fn data_directory(&self) -> Result<&Path, String> {
        self.0
            .parent()
            .ok_or_else(|| "无法定位应用数据目录。".into())
    }

    pub fn info(&self) -> Result<StorageInfo, String> {
        const ERROR: &str = "无法读取本地存储用量。";
        let data_directory = self.data_directory()?;
        let database_path = data_directory.join("livecap.sqlite3");
        let database_bytes = std::fs::metadata(&database_path).map_err(|_| ERROR)?.len();
        let mut recording_count = 0u64;
        let mut recording_bytes = 0u64;
        for entry in std::fs::read_dir(&self.0).map_err(|_| ERROR)? {
            let entry = entry.map_err(|_| ERROR)?;
            if !entry.file_type().map_err(|_| ERROR)?.is_file()
                || entry.file_name().to_str().and_then(wav_id).is_none()
            {
                continue;
            }
            recording_count += 1;
            recording_bytes =
                recording_bytes.saturating_add(entry.metadata().map_err(|_| ERROR)?.len());
        }
        Ok(StorageInfo {
            data_directory: data_directory.to_string_lossy().into_owned(),
            database_path: database_path.to_string_lossy().into_owned(),
            recordings_directory: self.0.to_string_lossy().into_owned(),
            database_bytes,
            recording_count,
            recording_bytes,
            total_bytes: database_bytes.saturating_add(recording_bytes),
        })
    }
}

fn wav_id(name: &str) -> Option<Uuid> {
    name.strip_suffix(".wav")
        .and_then(|id| Uuid::parse_str(id).ok())
}

pub struct WavWriter {
    file: File,
    path: PathBuf,
    rate: u32,
    bytes: u32,
}

impl WavWriter {
    pub fn create(directory: &Path, rate: u32) -> Result<Self, &'static str> {
        let path = directory.join(format!("{}.wav", Uuid::new_v4()));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| AUDIO_ERROR)?;
        let mut writer = Self {
            file,
            path,
            rate,
            bytes: 0,
        };
        if writer.write_header().is_err() {
            let _ = writer.finish();
            return Err(AUDIO_ERROR);
        }
        Ok(writer)
    }

    pub fn append(&mut self, pcm: &[u8]) -> Result<(), &'static str> {
        let size = u32::try_from(pcm.len()).map_err(|_| AUDIO_ERROR)?;
        let total = self
            .bytes
            .checked_add(size)
            .filter(|bytes| *bytes <= u32::MAX - 36)
            .ok_or("录音已达到 WAV 文件大小上限。")?;
        self.file.write_all(pcm).map_err(|_| AUDIO_ERROR)?;
        self.bytes = total;
        Ok(())
    }

    pub fn finish(mut self) -> Result<Option<AudioArtifact>, &'static str> {
        if self.bytes == 0 {
            drop(self.file);
            let _ = std::fs::remove_file(&self.path);
            return Ok(None);
        }
        let result = self
            .file
            .set_len(WAV_HEADER + u64::from(self.bytes))
            .and_then(|()| self.write_header())
            .and_then(|()| self.file.sync_all());
        if result.is_err() {
            drop(self.file);
            let _ = std::fs::remove_file(&self.path);
            return Err(AUDIO_ERROR);
        }
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AUDIO_ERROR)?;
        Ok(Some(AudioArtifact {
            file_name: file_name.into(),
            duration_ms: u64::from(self.bytes) * 1000 / (u64::from(self.rate) * 2),
        }))
    }

    fn write_header(&mut self) -> std::io::Result<()> {
        let mut header = Vec::with_capacity(WAV_HEADER as usize);
        header.extend(b"RIFF");
        header.extend((self.bytes + 36).to_le_bytes());
        header.extend(b"WAVEfmt ");
        header.extend(16u32.to_le_bytes());
        header.extend(1u16.to_le_bytes()); // PCM
        header.extend(1u16.to_le_bytes()); // mono
        header.extend(self.rate.to_le_bytes());
        header.extend((self.rate * 2).to_le_bytes());
        header.extend(2u16.to_le_bytes()); // block alignment
        header.extend(16u16.to_le_bytes());
        header.extend(b"data");
        header.extend(self.bytes.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioChunk {
    data: String,
    total_bytes: u64,
    mime_type: &'static str,
}

pub fn read_audio(
    directory: &Path,
    name: &str,
    offset: u64,
    length: usize,
) -> Result<AudioChunk, String> {
    let invalid = "该记录的录音文件不可用。";
    let id = wav_id(name).ok_or(invalid)?;
    let mut file = File::open(directory.join(format!("{id}.wav"))).map_err(|_| invalid)?;
    let total_bytes = file.metadata().map_err(|_| invalid)?.len();
    if offset > total_bytes || !(1..=MAX_AUDIO_CHUNK).contains(&length) {
        return Err("录音读取范围无效。".into());
    }
    file.seek(SeekFrom::Start(offset)).map_err(|_| invalid)?;
    let mut bytes = vec![0; length.min((total_bytes - offset) as usize)];
    file.read_exact(&mut bytes).map_err(|_| invalid)?;
    Ok(AudioChunk {
        data: STANDARD.encode(bytes),
        total_bytes,
        mime_type: "audio/wav",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_info_counts_owned_wav_files_and_real_bytes() {
        let root = std::env::temp_dir().join(format!("livecap-test-{}", Uuid::new_v4()));
        let directory = RecordingsDirectory(root.join("recordings"));
        std::fs::create_dir_all(&directory.0).unwrap();
        let database = root.join("livecap.sqlite3");
        std::fs::write(&database, [0; 128]).unwrap();
        let unrelated = directory.0.join("unrelated.wav");
        std::fs::write(&unrelated, [0; 80]).unwrap();
        let mut writer = WavWriter::create(&directory.0, 16_000).unwrap();
        writer.append(&[1, 2]).unwrap();
        let audio = writer.finish().unwrap().unwrap();
        let info = directory.info().unwrap();
        assert_eq!(info.data_directory, root.to_string_lossy());
        assert_eq!(info.database_path, database.to_string_lossy());
        assert_eq!(info.recording_count, 1);
        assert_eq!(info.recording_bytes, 46);
        assert_eq!(info.database_bytes, 128);
        assert_eq!(info.total_bytes, 174);
        std::fs::remove_file(directory.0.join(audio.file_name)).unwrap();
        std::fs::remove_file(unrelated).unwrap();
        std::fs::remove_file(database).unwrap();
        std::fs::remove_dir(directory.0).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn writes_valid_pcm_header_and_reads_only_requested_range() {
        let directory = std::env::temp_dir().join(format!("livecap-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let mut writer = WavWriter::create(&directory, 24_000).unwrap();
        writer.append(&[1, 2].repeat(2_400)).unwrap();
        writer.append(&[3, 4].repeat(2_400)).unwrap();
        let artifact = writer.finish().unwrap().unwrap();
        assert_eq!(artifact.duration_ms, 200);
        let first = read_audio(&directory, &artifact.file_name, 0, 46).unwrap();
        let bytes = STANDARD.decode(first.data).unwrap();
        assert_eq!(first.total_bytes, 9_644);
        assert_eq!(first.mime_type, "audio/wav");
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 9_636);
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            24_000
        );
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 9_600);
        assert_eq!(&bytes[44..46], &[1, 2]);
        let tail = read_audio(&directory, &artifact.file_name, 9_642, 10).unwrap();
        assert_eq!(STANDARD.decode(tail.data).unwrap(), [3, 4]);
        assert!(read_audio(&directory, "../external.wav", 0, 1).is_err());
        assert!(read_audio(&directory, &artifact.file_name, 0, MAX_AUDIO_CHUNK + 1).is_err());
        std::fs::remove_file(directory.join(artifact.file_name)).unwrap();
        assert!(
            WavWriter::create(&directory, 16_000)
                .unwrap()
                .finish()
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn archives_confirmed_tokens_including_late_final_translation() {
        let recorder = Recorder::default();
        let event = |text: &str, is_final: bool| {
            StreamEvent::Transcript(
                serde_json::from_value(serde_json::json!({
                    "session_id":"test", "finished":false,
                    "tokens":[{"text":text,"is_final":is_final,"translation_status":"original"}]
                }))
                .unwrap(),
            )
        };
        recorder.observe(&event("draft", false));
        recorder.observe(&event("final source", true));
        recorder.observe(&event("final translation", true));
        recorder.observe(&StreamEvent::TranslationWarning("some failure".into()));
        let (events, error) = recorder.take();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].tokens[0].text, "final source");
        assert_eq!(events[1].tokens[0].text, "final translation");
        assert_eq!(error.as_deref(), Some("some failure"));
    }
}
