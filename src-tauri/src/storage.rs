use crate::{
    knowledge::{KnowledgeCache, KnowledgeUsage, MAX_KNOWLEDGE_SOURCES},
    language::{Language, LanguagePair},
    records::{
        NewRecord, RecordDetail, RecordInsights, RecordPatch, RecordStatus, RecordSummary,
        SessionMode, UsageEntry,
    },
    transcript::Provider,
    usage::{Snapshot, Tokens},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Settings {
    pub terms: Vec<String>,
    pub model: String,
    pub provider: Provider,
    pub font_size: u32,
    pub always_on_top: bool,
    pub source_language: Language,
    pub target_language: Language,
    pub caption_mode: CaptionMode,
    pub translation_first: bool,
    pub caption_theme: CaptionTheme,
    pub caption_opacity: f64,
    pub onboarding_complete: bool,
    pub auto_learn_meeting_terms: bool,
    pub auto_generate_meeting_notes: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptionMode {
    #[default]
    Both,
    Translation,
    Original,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptionTheme {
    #[default]
    Dark,
    Light,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            terms: Vec::new(),
            model: "stt-rt-v6".into(),
            provider: Provider::default(),
            font_size: 24,
            always_on_top: true,
            source_language: Language::En,
            target_language: Language::Zh,
            caption_mode: CaptionMode::Both,
            translation_first: true,
            caption_theme: CaptionTheme::Dark,
            caption_opacity: 0.9,
            onboarding_complete: false,
            auto_learn_meeting_terms: false,
            auto_generate_meeting_notes: true,
        }
    }
}

impl Settings {
    pub fn languages(&self) -> LanguagePair {
        LanguagePair {
            source: self.source_language,
            target: self.target_language,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    id: i64,
    created_at: String,
    content: String,
}

pub struct Storage(Mutex<Connection>);

const STORAGE_ERROR: &str = "无法读取或写入本地数据。";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeUsageEntry {
    id: i64,
    domain: String,
    created_at: String,
    usage: KnowledgeUsage,
}

impl Storage {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "PRAGMA secure_delete = ON;
            CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS transcripts (
                id INTEGER PRIMARY KEY,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                content TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS knowledge_cache (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS knowledge_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                domain TEXT NOT NULL,
                created_at TEXT NOT NULL,
                usage TEXT NOT NULL
            );
            INSERT INTO knowledge_usage (domain, created_at, usage)
                SELECT json_extract(data, '$.domain'), json_extract(data, '$.updatedAt'),
                       json_extract(data, '$.usage')
                FROM knowledge_cache
                WHERE NOT EXISTS (SELECT 1 FROM knowledge_usage)
                    AND CASE WHEN json_valid(data) THEN
                        json_type(data, '$.domain') = 'text'
                        AND json_type(data, '$.updatedAt') = 'text'
                        AND json_type(data, '$.usage') = 'object'
                    ELSE 0 END;
            CREATE TABLE IF NOT EXISTS records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                provider TEXT NOT NULL,
                audio_file TEXT,
                error TEXT,
                usage TEXT,
                notes TEXT NOT NULL DEFAULT '',
                transcript_text TEXT,
                events TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS legacy_record_edits (
                id INTEGER PRIMARY KEY,
                title TEXT,
                notes TEXT,
                transcript_text TEXT,
                usage TEXT
            );
            CREATE TABLE IF NOT EXISTS subtitle_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at TEXT NOT NULL,
                usage TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS record_insights (
                record_id INTEGER PRIMARY KEY,
                target_language TEXT NOT NULL,
                timeline TEXT
            );
            CREATE TABLE IF NOT EXISTS record_diagrams (
                record_id INTEGER PRIMARY KEY,
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS record_note_sources (
                record_id INTEGER PRIMARY KEY,
                data TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS record_segment_edits (
                record_id INTEGER PRIMARY KEY,
                data TEXT NOT NULL
            );",
        )?;
        Ok(Self(Mutex::new(connection)))
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.0.lock().map_err(|_| STORAGE_ERROR.into())
    }

    pub fn load_knowledge(&self) -> Result<Option<KnowledgeCache>, String> {
        let connection = self.connection()?;
        read_knowledge(&connection)
    }

    pub fn record_timestamp(&self) -> Result<String, String> {
        self.connection()?
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get(0)
            })
            .map_err(|_| STORAGE_ERROR.into())
    }

    pub fn save_subtitle_usage(&self, created_at: &str, usage: Snapshot) -> Result<(), String> {
        let usage = serde_json::to_string(&usage).map_err(|_| STORAGE_ERROR)?;
        self.connection()?
            .execute(
                "INSERT INTO subtitle_usage (created_at,usage) VALUES (?1,?2)",
                params![created_at, usage],
            )
            .map_err(|_| STORAGE_ERROR)?;
        Ok(())
    }

    pub fn list_usage(&self) -> Result<Vec<UsageEntry>, String> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare(
                "SELECT id,created_at,usage,'subtitle' FROM subtitle_usage
             UNION ALL SELECT id,created_at,usage,'meeting' FROM records WHERE usage IS NOT NULL
             UNION ALL SELECT -t.id,t.created_at,e.usage,'meeting' FROM transcripts t
                JOIN legacy_record_edits e ON e.id=t.id WHERE e.usage IS NOT NULL
             ORDER BY created_at DESC,id DESC",
            )
            .map_err(|_| STORAGE_ERROR)?;
        query
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let mode = row.get::<_, String>(3)?;
                Ok(UsageEntry {
                    id: format!("{mode}-{id}"),
                    record_id: (mode == "meeting").then_some(id),
                    mode: if mode == "meeting" {
                        SessionMode::Meeting
                    } else {
                        SessionMode::Subtitle
                    },
                    created_at: row.get(1)?,
                    usage: json_column(row, 2)?.ok_or(rusqlite::Error::InvalidQuery)?,
                })
            })
            .map_err(|_| STORAGE_ERROR)?
            .collect::<rusqlite::Result<_>>()
            .map_err(|_| STORAGE_ERROR.into())
    }

    pub fn save_record(&self, record: NewRecord) -> Result<i64, String> {
        let title = record
            .events
            .iter()
            .flat_map(|event| &event.tokens)
            .filter(|token| {
                token.is_final
                    && token.translation_source.is_none()
                    && !token.text.trim().is_empty()
            })
            .flat_map(|token| token.text.chars())
            .take(64)
            .collect::<String>();
        let title = if title.is_empty() {
            "字幕会话"
        } else {
            &title
        };
        let events = serde_json::to_string(&record.events).map_err(|_| STORAGE_ERROR)?;
        let usage = record
            .usage
            .map(|usage| serde_json::to_string(&usage))
            .transpose()
            .map_err(|_| STORAGE_ERROR)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        transaction.execute(
            "INSERT INTO records (title, created_at, duration_ms, provider, audio_file, error, usage, events)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                title, record.created_at, record.audio.as_ref().map_or(0, |audio| audio.duration_ms as i64),
                match record.provider { Provider::Openai => "openai", Provider::Soniox => "soniox" },
                record.audio.map(|audio| audio.file_name), record.error, usage, events,
            ],
        ).map_err(|_| STORAGE_ERROR)?;
        let id = transaction.last_insert_rowid();
        let language = serde_json::to_string(&record.target_language).map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute(
                "INSERT INTO record_insights (record_id,target_language) VALUES (?1,?2)",
                params![id, language],
            )
            .map_err(|_| STORAGE_ERROR)?;
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        Ok(id)
    }

    pub fn list_records(&self) -> Result<Vec<RecordSummary>, String> {
        let connection = self.connection()?;
        let mut records = connection.prepare(
            "SELECT id,title,created_at,duration_ms,provider,audio_file,error,usage FROM records
             ORDER BY created_at DESC,id DESC LIMIT 200",
        ).map_err(|_| STORAGE_ERROR)?;
        let mut output = records
            .query_map([], record_summary)
            .map_err(|_| STORAGE_ERROR)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| STORAGE_ERROR)?;
        let mut legacy = connection
            .prepare(
                "SELECT t.id,t.created_at,NULL,e.title,e.usage
             FROM transcripts t LEFT JOIN legacy_record_edits e ON e.id=t.id
             ORDER BY t.created_at DESC,t.id DESC LIMIT 200",
            )
            .map_err(|_| STORAGE_ERROR)?;
        for row in legacy
            .query_map([], legacy_summary)
            .map_err(|_| STORAGE_ERROR)?
        {
            output.push(row.map_err(|_| STORAGE_ERROR)?);
        }
        output.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right.id.cmp(&left.id))
        });
        output.truncate(200);
        Ok(output)
    }

    pub fn get_record(&self, id: i64) -> Result<RecordDetail, String> {
        let connection = self.connection()?;
        read_record(&connection, id)
    }

    pub fn update_record(&self, id: i64, mut patch: RecordPatch) -> Result<RecordDetail, String> {
        patch.validate()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        if patch.transcript_text.is_some() {
            let previous = read_record(&transaction, id)?;
            if !previous.notes.is_empty() {
                let data =
                    serde_json::to_string(&previous.note_sources).map_err(|_| STORAGE_ERROR)?;
                transaction
                    .execute(
                        "INSERT OR IGNORE INTO record_note_sources (record_id,data) VALUES (?1,?2)",
                        params![id, data],
                    )
                    .map_err(|_| STORAGE_ERROR)?;
            }
        }
        if id < 0 {
            let legacy_id = id.checked_neg().ok_or("记录不存在。")?;
            let changed = transaction
                .execute(
                    "INSERT INTO legacy_record_edits (id,title,notes,transcript_text)
                 SELECT id, ?2, ?3, ?4 FROM transcripts WHERE id=?1
                 ON CONFLICT(id) DO UPDATE SET
                     title=coalesce(excluded.title, title), notes=coalesce(excluded.notes, notes),
                     transcript_text=coalesce(excluded.transcript_text, transcript_text)",
                    params![legacy_id, patch.title, patch.notes, patch.transcript_text],
                )
                .map_err(|_| STORAGE_ERROR)?;
            if changed == 0 {
                return Err("记录不存在。".into());
            }
        } else {
            let changed = transaction
                .execute(
                    "UPDATE records SET title=coalesce(?2,title),notes=coalesce(?3,notes),
                    transcript_text=coalesce(?4,transcript_text) WHERE id=?1",
                    params![id, patch.title, patch.notes, patch.transcript_text],
                )
                .map_err(|_| STORAGE_ERROR)?;
            if changed == 0 {
                return Err("记录不存在。".into());
            }
        }
        if let Some(edits) = patch.segment_edits {
            let data = serde_json::to_string(&edits).map_err(|_| STORAGE_ERROR)?;
            transaction
                .execute(
                    "INSERT INTO record_segment_edits (record_id,data) VALUES (?1,?2)
                 ON CONFLICT(record_id) DO UPDATE SET data=excluded.data",
                    params![id, data],
                )
                .map_err(|_| STORAGE_ERROR)?;
        }
        let record = read_record(&transaction, id)?;
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        Ok(record)
    }

    pub fn record_audio_file(&self, id: i64) -> Result<String, String> {
        let file: Option<String> = self
            .connection()?
            .query_row("SELECT audio_file FROM records WHERE id=?1", [id], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|_| STORAGE_ERROR)?
            .flatten();
        file.ok_or("该记录没有录音。".into())
    }

    pub fn save_record_analysis(
        &self,
        id: i64,
        generated: Option<(&RecordDetail, &RecordInsights, Language)>,
        tokens: Option<Tokens>,
    ) -> Result<RecordDetail, String> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        let record = read_record(&transaction, id)?;
        let mut usage = record
            .summary
            .usage
            .unwrap_or_else(|| Snapshot::for_record_analysis(id, record.summary.provider));
        usage.add_analysis(tokens);
        let data = serde_json::to_string(&usage).map_err(|_| STORAGE_ERROR)?;
        let conflict = generated.is_some_and(|(previous, _, _)| {
            record.notes != previous.notes || record.transcript_text != previous.transcript_text
        });
        let accepted = generated.filter(|_| !conflict);
        let notes = accepted.and_then(|(_, next, _)| next.notes.as_deref());
        let title = accepted.and_then(|(previous, next, _)| {
            (record.summary.title == previous.summary.title).then_some(next.title.as_str())
        });
        if id < 0 {
            transaction
                .execute(
                    "INSERT INTO legacy_record_edits (id,title,notes,usage) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(id) DO UPDATE SET title=coalesce(excluded.title,title),
                 notes=coalesce(excluded.notes,notes),usage=excluded.usage",
                    params![-id, title, notes, data],
                )
                .map_err(|_| STORAGE_ERROR)?;
        } else {
            transaction.execute(
                "UPDATE records SET title=coalesce(?2,title),notes=coalesce(?3,notes),usage=?4 WHERE id=?1",
                params![id, title, notes, data],
            ).map_err(|_| STORAGE_ERROR)?;
        }
        if let Some((previous, next, language)) = accepted {
            if next.notes.is_some() {
                let times = crate::record_ai::Input::from_record(previous)
                    .map(|input| input.source_times())
                    .unwrap_or_default();
                let data = serde_json::to_string(&times).map_err(|_| STORAGE_ERROR)?;
                transaction
                    .execute(
                        "INSERT INTO record_note_sources (record_id,data) VALUES (?1,?2)
                     ON CONFLICT(record_id) DO UPDATE SET data=excluded.data",
                        params![id, data],
                    )
                    .map_err(|_| STORAGE_ERROR)?;
            }
            let language = serde_json::to_string(&language).map_err(|_| STORAGE_ERROR)?;
            let timeline = serde_json::to_string(&next.timeline).map_err(|_| STORAGE_ERROR)?;
            transaction.execute(
                "INSERT INTO record_insights (record_id,target_language,timeline) VALUES (?1,?2,?3)
                 ON CONFLICT(record_id) DO UPDATE SET timeline=excluded.timeline",
                params![id, language, timeline],
            ).map_err(|_| STORAGE_ERROR)?;
        }
        if let Some((_, next, _)) = accepted {
            if let Some(diagram) = &next.diagram {
                let data = serde_json::to_string(diagram).map_err(|_| STORAGE_ERROR)?;
                transaction
                    .execute(
                        "INSERT INTO record_diagrams (record_id,data) VALUES (?1,?2)
                     ON CONFLICT(record_id) DO UPDATE SET data=excluded.data",
                        params![id, data],
                    )
                    .map_err(|_| STORAGE_ERROR)?;
            } else {
                transaction
                    .execute("DELETE FROM record_diagrams WHERE record_id=?1", [id])
                    .map_err(|_| STORAGE_ERROR)?;
            }
        }
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        if conflict {
            return Err("整理期间纪要或原文已被编辑，已保留你的修改。".into());
        }
        read_record(&connection, id)
    }

    pub fn list_knowledge_usage(&self) -> Result<Vec<KnowledgeUsageEntry>, String> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare("SELECT id,domain,created_at,usage FROM knowledge_usage ORDER BY created_at DESC,id DESC")
            .map_err(|_| STORAGE_ERROR)?;
        query
            .query_map([], |row| {
                Ok(KnowledgeUsageEntry {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    created_at: row.get(2)?,
                    usage: json_column(row, 3)?.ok_or(rusqlite::Error::InvalidQuery)?,
                })
            })
            .map_err(|_| STORAGE_ERROR)?
            .collect::<rusqlite::Result<_>>()
            .map_err(|_| STORAGE_ERROR.into())
    }

    pub fn replace_knowledge(&self, mut cache: KnowledgeCache) -> Result<KnowledgeCache, String> {
        cache.normalize_translations();
        cache.validate_generated()?;
        let usage = serde_json::to_string(&cache.usage).map_err(|_| STORAGE_ERROR)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        if let Some(previous) = read_knowledge(&transaction)?
            && ((previous.domain == cache.domain && previous.focus == cache.focus)
                || previous.domain == crate::knowledge_generate::MEETING_AREA)
        {
            let learned = previous
                .entries
                .into_iter()
                .filter(|entry| entry.coverage_area == crate::knowledge_generate::MEETING_AREA)
                .collect();
            merge_meeting_entries(&mut cache, previous.sources, learned);
        }
        cache.updated_at = transaction
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get(0)
            })
            .map_err(|_| STORAGE_ERROR)?;
        cache.enabled = true;
        write_knowledge(&transaction, &cache)?;
        transaction
            .execute(
                "INSERT INTO knowledge_usage (domain, created_at, usage) VALUES (?1,?2,?3)",
                params![cache.domain, cache.updated_at, usage],
            )
            .map_err(|_| STORAGE_ERROR)?;
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        Ok(cache)
    }

    pub fn append_meeting_knowledge(&self, mut incoming: KnowledgeCache) -> Result<usize, String> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        let now: String = transaction
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get(0)
            })
            .map_err(|_| STORAGE_ERROR)?;
        incoming.normalize_translations();
        let usage = serde_json::to_string(&incoming.usage).map_err(|_| STORAGE_ERROR)?;
        let mut added = incoming.entries.len();
        if added != 0 {
            if let Some(mut cache) = read_knowledge(&transaction)? {
                added = merge_meeting_entries(&mut cache, incoming.sources, incoming.entries);
                if added != 0 {
                    cache.updated_at = now.clone();
                    cache.validate_generated()?;
                    write_knowledge(&transaction, &cache)?;
                }
            } else {
                incoming.updated_at = now.clone();
                incoming.validate_generated()?;
                write_knowledge(&transaction, &incoming)?;
            }
        }
        transaction
            .execute(
                "INSERT INTO knowledge_usage (domain, created_at, usage) VALUES (?1,?2,?3)",
                params![crate::knowledge_generate::MEETING_AREA, now, usage],
            )
            .map_err(|_| STORAGE_ERROR)?;
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        Ok(added)
    }

    pub fn edit_knowledge(
        &self,
        edit: impl FnOnce(&mut KnowledgeCache),
    ) -> Result<KnowledgeCache, String> {
        let connection = self.connection()?;
        let mut cache = read_knowledge(&connection)?.ok_or("暂无知识缓存。")?;
        edit(&mut cache);
        write_knowledge(&connection, &cache)?;
        Ok(cache)
    }

    pub fn clear_knowledge(&self) -> Result<(), String> {
        self.connection()?
            .execute("DELETE FROM knowledge_cache WHERE id = 1", [])
            .map_err(|_| STORAGE_ERROR)?;
        Ok(())
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        let data: Option<String> = self
            .connection()?
            .query_row("SELECT data FROM settings WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|_| STORAGE_ERROR)?;
        match data {
            Some(data) => decode_settings(&data).map_err(|_| STORAGE_ERROR.into()),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, mut settings: Settings) -> Result<(), String> {
        settings.languages().validate()?;
        if !(0.6..=1.0).contains(&settings.caption_opacity) {
            return Err("字幕不透明度需在 60% 到 100% 之间。".into());
        }
        settings.model = settings.model.trim().into();
        if settings.model.is_empty() || settings.model.chars().count() > 100 {
            return Err("模型名称需在 1 到 100 字符之间。".into());
        }
        if !(18..=36).contains(&settings.font_size) {
            return Err("字号需在 18 到 36 之间。".into());
        }
        if settings.terms.len() > 100
            || settings
                .terms
                .iter()
                .any(|term| term.trim().chars().count() > 100)
        {
            return Err("最多可保存 100 个术语，每个术语最多 100 字符。".into());
        }
        let mut seen = std::collections::HashSet::new();
        settings.terms = settings
            .terms
            .into_iter()
            .map(|term| term.trim().to_owned())
            .filter(|term| !term.is_empty() && seen.insert(term.clone()))
            .collect();
        if settings.provider == Provider::Openai
            && settings
                .terms
                .iter()
                .any(|term| !crate::openai::valid_keyword(term))
        {
            return Err("OpenAI 术语不能包含尖括号或换行。".into());
        }
        let data = serde_json::to_string(&settings).map_err(|_| STORAGE_ERROR)?;
        self.connection()?
            .execute(
                "INSERT INTO settings (id, data) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                [data],
            )
            .map_err(|_| STORAGE_ERROR)?;
        Ok(())
    }

    pub fn save_transcript(&self, content: &str) -> Result<i64, String> {
        if content.trim().is_empty() {
            return Err("当前没有可保存的字幕。".into());
        }
        if content.len() > 4 * 1024 * 1024 {
            return Err("字幕内容超过 4 MB。".into());
        }
        let connection = self.connection()?;
        connection
            .execute("INSERT INTO transcripts (content) VALUES (?1)", [content])
            .map_err(|_| STORAGE_ERROR)?;
        Ok(connection.last_insert_rowid())
    }

    pub fn list_transcripts(&self) -> Result<Vec<Transcript>, String> {
        let connection = self.connection()?;
        let mut query = connection
            .prepare("SELECT id, created_at, content FROM transcripts ORDER BY id DESC LIMIT 50")
            .map_err(|_| STORAGE_ERROR)?;
        query
            .query_map([], |row| {
                Ok(Transcript {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    content: row.get(2)?,
                })
            })
            .map_err(|_| STORAGE_ERROR)?
            .collect::<rusqlite::Result<_>>()
            .map_err(|_| STORAGE_ERROR.into())
    }

    pub fn delete_transcript(&self, id: i64) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute("DELETE FROM legacy_record_edits WHERE id = ?1", [id])
            .map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute(
                "DELETE FROM record_segment_edits WHERE record_id = -?1",
                [id],
            )
            .map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute("DELETE FROM transcripts WHERE id = ?1", params![id])
            .map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute("DELETE FROM record_insights WHERE record_id = -?1", [id])
            .map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute(
                "DELETE FROM record_note_sources WHERE record_id = -?1",
                [id],
            )
            .map_err(|_| STORAGE_ERROR)?;
        transaction
            .execute("DELETE FROM record_diagrams WHERE record_id=-?1", [id])
            .map_err(|_| STORAGE_ERROR)?;
        transaction.commit().map_err(|_| STORAGE_ERROR)?;
        Ok(())
    }
}

fn read_record(connection: &Connection, id: i64) -> Result<RecordDetail, String> {
    let record = if id < 0 {
        let id = id.checked_neg().ok_or("记录不存在。")?;
        connection
            .query_row(
                "SELECT t.id,t.created_at,t.content,e.title,e.usage,e.notes,e.transcript_text
             FROM transcripts t LEFT JOIN legacy_record_edits e ON e.id=t.id WHERE t.id=?1",
                [id],
                |row| {
                    Ok(RecordDetail {
                        summary: legacy_summary(row)?,
                        notes: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        transcript_text: row.get(6)?,
                        transcript_events: Vec::new(),
                        legacy_content: Some(row.get(2)?),
                        segment_edits: None,
                        target_language: None,
                        timeline: Vec::new(),
                        note_sources: Vec::new(),
                        diagram: None,
                    })
                },
            )
            .optional()
    } else {
        connection.query_row(
            "SELECT id,title,created_at,duration_ms,provider,audio_file,error,usage,notes,transcript_text,events
             FROM records WHERE id=?1",
            [id], |row| Ok(RecordDetail {
                summary: record_summary(row)?,
                notes: row.get(8)?,
                transcript_text: row.get(9)?,
                transcript_events: json_column(row, 10)?.unwrap_or_default(),
                diagram: None,
                legacy_content: None,
                segment_edits: None,
                target_language: None,
                timeline: Vec::new(),
                note_sources: Vec::new(),
            }),
        ).optional()
    };
    let mut record = record.map_err(|_| STORAGE_ERROR)?.ok_or("记录不存在。")?;
    record.segment_edits = connection
        .query_row(
            "SELECT data FROM record_segment_edits WHERE record_id=?1",
            [id],
            |row| json_column(row, 0),
        )
        .optional()
        .map_err(|_| STORAGE_ERROR)?
        .flatten();
    if let Some((language, timeline)) = connection
        .query_row(
            "SELECT target_language,timeline FROM record_insights WHERE record_id=?1",
            [id],
            |row| Ok((json_column(row, 0)?, json_column(row, 1)?)),
        )
        .optional()
        .map_err(|_| STORAGE_ERROR)?
    {
        record.target_language = language;
        record.timeline = timeline.unwrap_or_default();
    }
    if !record.notes.is_empty() {
        let stored: Option<Vec<Option<u64>>> = connection
            .query_row(
                "SELECT data FROM record_note_sources WHERE record_id=?1",
                [id],
                |row| json_column(row, 0),
            )
            .optional()
            .map_err(|_| STORAGE_ERROR)?
            .flatten();
        record.note_sources = stored.unwrap_or_else(|| {
            crate::record_ai::Input::from_record(&record)
                .map(|input| input.source_times())
                .unwrap_or_default()
        });
    }
    record.diagram = connection
        .query_row(
            "SELECT data FROM record_diagrams WHERE record_id=?1",
            [id],
            |row| json_column(row, 0),
        )
        .optional()
        .map_err(|_| STORAGE_ERROR)?
        .flatten();
    Ok(record)
}

fn json_column<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    column: usize,
) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(column)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}

fn record_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordSummary> {
    let error = row.get(6)?;
    Ok(RecordSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        duration_ms: row.get::<_, i64>(3)?.max(0) as u64,
        provider: match row.get::<_, String>(4)?.as_str() {
            "openai" => Some(Provider::Openai),
            "soniox" => Some(Provider::Soniox),
            _ => None,
        },
        status: RecordStatus::from_error(&error),
        audio_available: row.get::<_, Option<String>>(5)?.is_some(),
        error,
        usage: json_column(row, 7)?,
    })
}

fn legacy_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordSummary> {
    let title = row
        .get::<_, Option<String>>(3)?
        .unwrap_or_else(|| "已保存字幕".into());
    Ok(RecordSummary {
        id: -row.get::<_, i64>(0)?,
        title,
        created_at: row.get(1)?,
        duration_ms: 0,
        provider: None,
        status: RecordStatus::Complete,
        audio_available: false,
        error: None,
        usage: json_column(row, 4)?,
    })
}

fn merge_meeting_entries(
    cache: &mut KnowledgeCache,
    sources: Vec<crate::knowledge::KnowledgeSource>,
    entries: Vec<crate::knowledge::KnowledgeEntry>,
) -> usize {
    use std::collections::HashSet;
    let mut known: HashSet<_> = cache
        .entries
        .iter()
        .flat_map(|entry| std::iter::once(&entry.term).chain(&entry.aliases))
        .map(|term| term.to_ascii_lowercase())
        .collect();
    let mut urls: HashSet<_> = cache
        .sources
        .iter()
        .map(|source| source.url.clone())
        .collect();
    let mut added = 0;
    for entry in entries {
        let names: Vec<_> = std::iter::once(&entry.term)
            .chain(&entry.aliases)
            .map(|term| term.to_ascii_lowercase())
            .collect();
        if names.iter().any(|term| known.contains(term)) {
            continue;
        }
        let new_sources: Vec<_> = sources
            .iter()
            .filter(|source| !urls.contains(&source.url) && entry.source_urls.contains(&source.url))
            .collect();
        if urls.len() + new_sources.len() > MAX_KNOWLEDGE_SOURCES {
            continue;
        }
        for source in new_sources {
            urls.insert(source.url.clone());
            cache.sources.push(crate::knowledge::KnowledgeSource {
                url: source.url.clone(),
                title: source.title.clone(),
            });
        }
        known.extend(names);
        cache.entries.push(entry);
        added += 1;
    }
    added
}

fn read_knowledge(connection: &Connection) -> Result<Option<KnowledgeCache>, String> {
    let data: Option<String> = connection
        .query_row("SELECT data FROM knowledge_cache WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|_| STORAGE_ERROR)?;
    data.map(|data| {
        let mut cache: KnowledgeCache = serde_json::from_str(&data).map_err(|_| STORAGE_ERROR)?;
        cache.normalize_translations();
        Ok(cache)
    })
    .transpose()
}

fn write_knowledge(connection: &Connection, cache: &KnowledgeCache) -> Result<(), String> {
    let data = serde_json::to_string(cache).map_err(|_| STORAGE_ERROR)?;
    connection
        .execute(
            "INSERT INTO knowledge_cache (id, data) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            [data],
        )
        .map_err(|_| STORAGE_ERROR)?;
    Ok(())
}

fn decode_settings(data: &str) -> serde_json::Result<Settings> {
    let mut value: serde_json::Value = serde_json::from_str(data)?;
    if let Some(settings) = value.as_object_mut() {
        settings.remove("translationProvider");
    }
    serde_json::from_value(value)
}

pub fn credential(provider: Provider) -> Result<keyring::Entry, String> {
    let account = match provider {
        Provider::Soniox => "soniox",
        Provider::Openai => "openai",
    };
    keyring::Entry::new("com.chenmiaoi.livecap", account)
        .map_err(|_| "无法访问系统安全凭据存储。".into())
}

pub fn api_key(provider: Provider) -> Result<Option<String>, String> {
    match credential(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(format!("无法读取 {} API Key。", provider.label())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Generation, sample_cache};

    #[test]
    fn old_settings_receive_caption_defaults_and_validate_new_fields() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let old = decode_settings(r#"{"fontSize":30,"alwaysOnTop":false}"#).unwrap();
        let initial = serde_json::to_value(old).unwrap();
        assert_eq!(initial["sourceLanguage"], "en");
        assert_eq!(initial["targetLanguage"], "zh");
        assert_eq!(initial["captionMode"], "both");
        assert_eq!(initial["translationFirst"], true);
        assert_eq!(initial["captionTheme"], "dark");
        assert_eq!(initial["captionOpacity"], 0.9);
        assert_eq!(initial["onboardingComplete"], false);
        assert_eq!(initial["fontSize"], 30);
        let valid: Settings = serde_json::from_value(serde_json::json!({
            "sourceLanguage":"fr", "targetLanguage":"ja", "captionMode":"translation",
            "captionTheme":"light", "captionOpacity":0.65, "onboardingComplete":true
        }))
        .unwrap();
        storage.save_settings(valid).unwrap();
        let saved = serde_json::to_value(storage.load_settings().unwrap()).unwrap();
        assert_eq!(saved["captionMode"], "translation");
        assert_eq!(saved["captionTheme"], "light");
        assert_eq!(saved["onboardingComplete"], true);
        assert!(
            storage
                .save_settings(Settings {
                    target_language: Language::En,
                    ..Settings::default()
                })
                .is_err()
        );
        assert!(
            storage
                .save_settings(Settings {
                    caption_opacity: 0.5,
                    ..Settings::default()
                })
                .is_err()
        );
    }

    #[test]
    fn segment_edits_persist_with_corrected_text_and_preserve_raw_record() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let id = -storage.save_transcript("original transcript").unwrap();
        let segment_edits = serde_json::json!({"s1":{"text":"edited","speaker":"Lin"}});
        let missing: RecordPatch = serde_json::from_value(serde_json::json!({
            "segmentEdits":segment_edits
        }))
        .unwrap();
        assert!(storage.update_record(id, missing).is_err());
        let patch: RecordPatch = serde_json::from_value(serde_json::json!({
            "segmentEdits":segment_edits,"transcriptText":"edited source"
        }))
        .unwrap();
        storage.update_record(id, patch).unwrap();
        let record = serde_json::to_value(storage.get_record(id).unwrap()).unwrap();
        assert_eq!(record["segmentEdits"], segment_edits);
        assert_eq!(record["transcriptText"], "edited source");
        assert_eq!(record["legacyContent"], "original transcript");
        storage.delete_transcript(-id).unwrap();
        let id = -storage.save_transcript("new").unwrap();
        assert!(storage.get_record(id).unwrap().segment_edits.is_none());
    }

    #[test]
    fn records_round_trip_with_legacy_compatibility_and_analysis_usage() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let legacy = -storage.save_transcript("old source").unwrap();
        let id = storage
            .save_record(NewRecord {
                target_language: Language::Zh,
                provider: Provider::Openai,
                created_at: storage.record_timestamp().unwrap(),
                audio: Some(crate::records::AudioArtifact {
                    file_name: "stored.wav".into(),
                    duration_ms: 500,
                }),
                error: Some("network".into()),
                usage: None,
                events: Vec::new(),
            })
            .unwrap();
        assert_eq!(storage.list_records().unwrap().len(), 2);
        assert_eq!(storage.record_audio_file(id).unwrap(), "stored.wav");
        assert!(storage.record_audio_file(legacy).is_err());
        let old = storage.get_record(id).unwrap();
        assert!(matches!(old.summary.status, RecordStatus::Partial));
        assert_eq!(old.summary.duration_ms, 500);
        storage
            .update_record(
                id,
                RecordPatch {
                    notes: Some("manual".into()),
                    transcript_text: Some("corrected".into()),
                    ..RecordPatch::default()
                },
            )
            .unwrap();
        let tokens = Tokens {
            input: 100,
            cached: 10,
            output: 40,
        };
        assert!(
            storage
                .save_record_analysis(
                    id,
                    Some((
                        &old,
                        &RecordInsights {
                            diagram: None,
                            title: "AI title".into(),
                            notes: Some("AI".into()),
                            timeline: Vec::new()
                        },
                        Language::Zh
                    )),
                    Some(tokens)
                )
                .is_err()
        );
        let stored = storage.get_record(id).unwrap();
        assert_eq!(stored.notes, "manual");
        assert_eq!(stored.transcript_text.as_deref(), Some("corrected"));
        let usage = serde_json::to_value(stored.summary.usage.unwrap()).unwrap();
        assert_eq!(usage["input_tokens"], 100);
        assert_eq!(usage["analysis_usd"].as_f64().unwrap(), tokens.estimate());
        let edited = storage
            .update_record(
                legacy,
                RecordPatch {
                    title: Some("旧记录".into()),
                    ..RecordPatch::default()
                },
            )
            .unwrap();
        assert_eq!(edited.summary.title, "旧记录");
        assert_eq!(edited.legacy_content.as_deref(), Some("old source"));
        let legacy_usage = storage
            .save_record_analysis(legacy, None, Some(tokens))
            .unwrap();
        assert!(legacy_usage.summary.usage.is_some());
        storage.delete_transcript(-legacy).unwrap();
        let replacement = -storage.save_transcript("fresh source").unwrap();
        let fresh = storage.get_record(replacement).unwrap();
        assert_eq!(fresh.summary.title, "已保存字幕");
        assert!(fresh.summary.usage.is_none());
    }

    #[test]
    fn generated_summary_persists_timeline_without_overwriting_a_new_title() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let id = storage
            .save_record(NewRecord {
                provider: Provider::Openai,
                target_language: Language::Ja,
                created_at: storage.record_timestamp().unwrap(),
                audio: None,
                error: None,
                usage: None,
                events: Vec::new(),
            })
            .unwrap();
        let original = storage.get_record(id).unwrap();
        storage
            .update_record(
                id,
                RecordPatch {
                    title: Some("手動タイトル".into()),
                    ..RecordPatch::default()
                },
            )
            .unwrap();
        let result = storage
            .save_record_analysis(
                id,
                Some((
                    &original,
                    &RecordInsights {
                        diagram: None,
                        title: "自動タイトル".into(),
                        notes: Some("要点".into()),
                        timeline: vec![crate::records::TimelinePoint {
                            start_ms: Some(42),
                            title: "開始".into(),
                            summary: "概要".into(),
                            details: vec![],
                        }],
                    },
                    Language::Ja,
                )),
                None,
            )
            .unwrap();
        assert_eq!(result.summary.title, "手動タイトル");
        assert_eq!(result.notes, "要点");
        assert_eq!(result.timeline[0].start_ms, Some(42));
        assert!(result.target_language == Some(Language::Ja));
        assert!(storage.load_settings().unwrap().auto_generate_meeting_notes);
    }

    #[test]
    fn existing_note_citations_resolve_from_confirmed_originals_and_survive_transcript_edits() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let events = serde_json::from_value(serde_json::json!([
            {"session_id":"test","finished":false,"tokens":[{"text":"draft","is_final":false,"start_ms":10,"translation_status":"original"}]},
            {"session_id":"test","finished":false,"tokens":[{"text":"translated","is_final":true,
                "start_ms":20,"translation_status":"translation"}]},
            {"session_id":"test","finished":false,"tokens":[{"text":"first","is_final":true,"start_ms":6000,"translation_status":"original"}]},
            {"session_id":"test","finished":false,"tokens":[{"text":"second","is_final":true,"start_ms":21000,"translation_status":"original"}]}
        ])).unwrap();
        let id = storage
            .save_record(NewRecord {
                provider: Provider::Openai,
                target_language: Language::Zh,
                created_at: storage.record_timestamp().unwrap(),
                audio: None,
                error: None,
                usage: None,
                events,
            })
            .unwrap();
        let notes = "引用原文 [S2]。";
        let existing = storage
            .update_record(
                id,
                RecordPatch {
                    notes: Some(notes.into()),
                    ..RecordPatch::default()
                },
            )
            .unwrap();
        assert_eq!(existing.note_sources, vec![Some(6000), Some(21000)]);
        let corrected = storage
            .update_record(
                id,
                RecordPatch {
                    transcript_text: Some("Corrected text".into()),
                    ..RecordPatch::default()
                },
            )
            .unwrap();
        assert_eq!(corrected.note_sources, existing.note_sources);
        assert_eq!(corrected.notes, notes);
    }

    #[test]
    fn subtitle_usage_creates_no_archive_record() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let date = storage.record_timestamp().unwrap();
        storage
            .save_subtitle_usage(
                &date,
                Snapshot::for_record_analysis(3, Some(Provider::Openai)),
            )
            .unwrap();
        assert!(storage.list_records().unwrap().is_empty());
        let usage = storage.list_usage().unwrap();
        assert_eq!(usage.len(), 1);
        assert!(matches!(usage[0].mode, SessionMode::Subtitle));
        assert!(usage[0].record_id.is_none());
        assert_eq!(usage[0].created_at, date);
    }

    #[test]
    fn invalid_or_cancelled_replacement_preserves_current_cache() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        assert!(storage.load_knowledge().unwrap().is_none());
        let old = storage.replace_knowledge(sample_cache(&["AST"])).unwrap();
        assert!(old.updated_at.ends_with('Z'));
        let mut invalid = sample_cache(&["Sema"]);
        invalid.entries[0].source_urls = vec!["https://unverified.example.com/".into()];
        assert!(storage.replace_knowledge(invalid).is_err());
        let generation = Generation::default();
        let ticket = generation.begin().unwrap();
        assert!(generation.edit(|| storage.clear_knowledge()).is_err());
        generation.cancel();
        assert!(
            ticket
                .commit(|| storage.replace_knowledge(sample_cache(&["LLVM"])))
                .is_err()
        );
        drop(ticket);
        let saved = storage.load_knowledge().unwrap().unwrap();
        assert_eq!(saved.entries[0].term, "AST");
        assert_eq!(saved.updated_at, old.updated_at);
        assert_eq!(storage.list_knowledge_usage().unwrap().len(), 1);
    }

    #[test]
    fn knowledge_usage_backfills_once_and_survives_cache_replacement_and_clear() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let mut old = sample_cache(&["AST"]);
        old.updated_at = "2026-11-06T00:00:00.000Z".into();
        write_knowledge(&storage.connection().unwrap(), &old).unwrap();
        let connection = storage.0.into_inner().unwrap();
        let storage = Storage::initialize(connection).unwrap();
        let storage = Storage::initialize(storage.0.into_inner().unwrap()).unwrap();
        assert_eq!(storage.list_knowledge_usage().unwrap().len(), 1);

        storage.replace_knowledge(sample_cache(&["Sema"])).unwrap();
        storage.clear_knowledge().unwrap();
        let history = storage.list_knowledge_usage().unwrap();
        assert_eq!(history.len(), 2);
        assert!(
            history
                .iter()
                .any(|entry| entry.created_at == old.updated_at)
        );
    }

    #[test]
    fn meeting_terms_append_once_and_survive_refresh_of_the_same_domain() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        storage.replace_knowledge(sample_cache(&["AST"])).unwrap();
        for expected in [1, 0] {
            let mut learned = sample_cache(&["PGO"]);
            learned.entries[0].coverage_area = crate::knowledge_generate::MEETING_AREA.into();
            assert_eq!(storage.append_meeting_knowledge(learned).unwrap(), expected);
        }
        let cache = storage.replace_knowledge(sample_cache(&["Sema"])).unwrap();
        assert_eq!(
            cache
                .entries
                .iter()
                .map(|entry| entry.term.as_str())
                .collect::<Vec<_>>(),
            ["Sema", "PGO"]
        );
        assert_eq!(storage.list_knowledge_usage().unwrap().len(), 4);
    }

    #[test]
    fn disabling_and_removing_cache_entries_keeps_manual_terms() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        storage
            .save_settings(Settings {
                terms: vec!["AST".into()],
                ..Settings::default()
            })
            .unwrap();
        storage.replace_knowledge(sample_cache(&["AST"])).unwrap();
        storage
            .edit_knowledge(|cache| {
                cache.entries.clear();
                cache.enabled = false;
            })
            .unwrap();
        let cached = storage.load_knowledge().unwrap().unwrap();
        assert!(!cached.enabled && cached.entries.is_empty());
        assert_eq!(storage.load_settings().unwrap().terms, ["AST"]);
        storage.clear_knowledge().unwrap();
        assert!(storage.load_knowledge().unwrap().is_none());
    }

    #[test]
    fn old_settings_default_to_openai_and_keep_preferences() {
        let saved = decode_settings(
            r#"{"terms":["Clang"],"model":"stt-rt-v6","translationProvider":"soniox","fontSize":24,"alwaysOnTop":true}"#,
        )
        .unwrap();
        assert_eq!(saved.provider, Provider::Openai);
        assert_eq!(saved.terms, ["Clang"]);
    }

    #[test]
    fn settings_and_explicit_transcript_storage() {
        let storage = Storage::initialize(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(storage.load_settings().unwrap().font_size, 24);
        assert!(storage.list_transcripts().unwrap().is_empty());

        let settings = Settings {
            terms: vec![" LLVM ".into(), "LLVM".into(), "".into(), "C++".into()],
            ..Settings::default()
        };
        storage.save_settings(settings).unwrap();
        assert_eq!(storage.load_settings().unwrap().terms, ["LLVM", "C++"]);
        assert!(storage.save_transcript(" ").is_err());
        let id = storage.save_transcript("LLVM\n你好").unwrap();
        let saved = storage.list_transcripts().unwrap();
        assert_eq!(saved[0].id, id);
        assert_eq!(saved[0].content, "LLVM\n你好");
        assert!(saved[0].created_at.ends_with('Z'));
        storage.delete_transcript(id).unwrap();
        assert!(storage.list_transcripts().unwrap().is_empty());
    }
}
