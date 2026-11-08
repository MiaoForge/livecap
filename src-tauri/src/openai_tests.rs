use super::*;
use crate::{luna, session::authorization, transcript::Provider};
use std::sync::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_tungstenite::accept_async;

async fn next(socket: &mut WebSocketStream<TcpStream>, kind: &str) -> Value {
    let message = socket.next().await.unwrap().unwrap();
    let event: Value = serde_json::from_str(message.into_text().unwrap().as_str()).unwrap();
    assert_eq!(event["type"], kind);
    event
}

async fn send(socket: &mut WebSocketStream<TcpStream>, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn http_request(socket: &mut TcpStream) -> (String, Value) {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        header.push(socket.read_u8().await.unwrap());
        assert!(header.len() < 16_384);
    }
    let header = String::from_utf8(header).unwrap();
    let length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    let mut body = vec![0; length];
    socket.read_exact(&mut body).await.unwrap();
    (header, serde_json::from_slice(&body).unwrap())
}

async fn reply(socket: &mut TcpStream, status: &str, response: Value) {
    let body = response.to_string();
    socket.write_all(format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).as_bytes()).await.unwrap();
}

#[test]
fn realtime_config_auto_detects_language_and_uses_24khz_with_valid_keywords() {
    let config = config(&["C++".into(), "bad<term".into(), "bad\nterm".into()]);
    let input = &config["session"]["audio"]["input"];
    assert_eq!(config["type"], "session.update");
    assert_eq!(config["session"]["type"], "transcription");
    assert_eq!(input["format"], json!({"type":"audio/pcm","rate":24_000}));
    assert_eq!(input["transcription"]["model"], "gpt-live-transcribe");
    assert_eq!(input["transcription"]["keywords"], json!(["C++"]));
    assert!(input["transcription"].get("languages").is_none());
    assert_eq!(input["transcription"]["delay"], "low");
    assert!(input["turn_detection"].is_null());
}

fn new_turn() -> Turn {
    Turn {
        start: 0,
        samples: 0,
        quiet: 0,
    }
}

#[test]
fn brief_pauses_do_not_split_incomplete_sentences() {
    let mut turn = new_turn();
    assert!(!turn.add_packet(MIN_SAMPLES, true, false));
    assert!(!turn.add_packet(SENTENCE_QUIET_SAMPLES, false, false));
    assert!(!turn.add_packet(MIN_SAMPLES, true, false));
    assert!(!turn.add_packet(QUIET_SAMPLES - 1, false, false));
    assert!(turn.add_packet(1, false, false));
}

#[test]
fn sentence_boundary_requires_punctuation_and_a_pause() {
    assert!(ends_sentence("Does Clang parse it?” "));
    assert!(ends_sentence("完成了。"));
    assert!(!ends_sentence("We can..."));
    let mut turn = new_turn();
    assert!(!turn.add_packet(MIN_SAMPLES, true, true));
    assert!(!turn.add_packet(SENTENCE_QUIET_SAMPLES - 1, false, true));
    assert!(turn.add_packet(1, false, true));
}

#[test]
fn uninterrupted_audio_has_a_safety_limit() {
    let mut turn = new_turn();
    assert!(!turn.add_packet(SAMPLE_RATE as usize * 5, true, false));
    assert!(!turn.add_packet(MAX_SAMPLES - turn.samples - 1, true, false));
    assert!(turn.add_packet(1, true, false));
}

#[test]
fn pending_drafts_cannot_trigger_commit_of_the_current_turn() {
    let draft = |text: &str| Draft {
        text: text.into(),
        revision: 1,
    };
    let mut drafts = HashMap::from([
        ("pending".into(), draft("Clang crashes.")),
        ("current".into(), draft("It happens when")),
    ]);
    let mut pending = VecDeque::from([Pending {
        id: None,
        final_text: None,
        start: 0,
        end: 0,
        since: Instant::now(),
    }]);
    assert!(!current_draft_ends_sentence(&drafts, &pending));
    pending[0].id = Some("pending".into());
    assert!(!current_draft_ends_sentence(&drafts, &pending));
    drafts.insert("current".into(), draft("It happens here."));
    assert!(current_draft_ends_sentence(&drafts, &pending));
    pending[0].id = None;
    assert!(!current_draft_ends_sentence(&drafts, &pending));
}

#[tokio::test]
async fn local_realtime_to_luna_preserves_drafts_and_orders_final_turns() {
    timeout(Duration::from_secs(10), async {
        let (draft_started, draft_running) = oneshot::channel();
        let (final_seen, final_arrived) = oneshot::channel();
        let final_seen = Mutex::new(Some(final_seen));
        let ws = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}/v1/realtime?intent=transcription", ws.local_addr().unwrap());
        let ws_server = tokio::spawn(async move {
            let mut socket = accept_async(ws.accept().await.unwrap().0).await.unwrap();
            next(&mut socket, "session.update").await;
            send(&mut socket, json!({"type":"session.created"})).await;
            send(&mut socket, json!({"type":"session.updated"})).await;

            let append = next(&mut socket, "input_audio_buffer.append").await;
            assert_eq!(STANDARD.decode(append["audio"].as_str().unwrap()).unwrap().len(), MIN_SAMPLES * 2);
            for delta in ["LLVM ", "Clang crashes."] {
                send(&mut socket, json!({
                    "type":"conversation.item.input_audio_transcription.delta", "item_id":"item-1", "delta":delta
                })).await;
            }
            next(&mut socket, "input_audio_buffer.append").await;
            next(&mut socket, "input_audio_buffer.commit").await;
            send(&mut socket, json!({"type":"input_audio_buffer.committed", "item_id":"item-1"})).await;

            next(&mut socket, "input_audio_buffer.append").await;
            send(&mut socket, json!({
                "type":"conversation.item.input_audio_transcription.delta", "item_id":"item-2", "delta":"Sema"
            })).await;
            next(&mut socket, "input_audio_buffer.commit").await;
            send(&mut socket, json!({"type":"input_audio_buffer.committed", "item_id":"item-2", "previous_item_id":"item-1"})).await;
            draft_running.await.unwrap();
            send(&mut socket, json!({
                "type":"conversation.item.input_audio_transcription.completed", "item_id":"item-2", "transcript":"LLVM Sema?"
            })).await;
            send(&mut socket, json!({
                "type":"conversation.item.input_audio_transcription.delta", "item_id":"item-2", "delta":"late noise"
            })).await;
            send(&mut socket, json!({
                "type":"conversation.item.input_audio_transcription.completed", "item_id":"item-1", "transcript":"LLVM Clang crashes."
            })).await;
        });

        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_url = format!("http://{}/v1/responses", http.local_addr().unwrap());
        let http_server = tokio::spawn(async move {
            let mut draft_started = Some(draft_started);
            let mut final_arrived = Some(final_arrived);
            for (index, (source, preceding)) in [
                ("LLVM Clang crashes.", json!([])), ("LLVM Clang crashes.", json!([])), ("LLVM Sema?", json!(["LLVM Clang crashes."]))
            ].into_iter().enumerate() {
                let (mut socket, _) = http.accept().await.unwrap();
                let (header, body) = http_request(&mut socket).await;
                assert!(header.starts_with("POST /v1/responses "));
                assert!(header.to_lowercase().contains("authorization: bearer local-only"));
                assert_eq!(body["model"], "gpt-6-luna");
                assert_eq!(body["reasoning"]["effort"], "none");
                assert_eq!(body["store"], false);
                assert!(body["instructions"].as_str().unwrap().contains("Do not invent a continuation"));
                let input: Value = serde_json::from_str(body["input"].as_str().unwrap()).unwrap();
                assert_eq!(input["source"], source);
                assert_eq!(input["target_language"], "zh");
                assert_eq!(input["glossary"], json!(["Clang"]));
                assert_eq!(input["preceding_context"], preceding);
                assert!(input.get("audio").is_none());
                if index == 2 {
                    assert_eq!(input["knowledge"], json!([{
                        "term": "Sema", "meaning": "Definition of Sema", "translation": "Sema"
                    }]));
                } else {
                    // Clang is a manual term and IR is unrelated to the source.
                    assert_eq!(input["knowledge"], json!([]));
                }
                assert_eq!(input["is_final"], index != 0);
                assert_eq!(input["learned_terms"], if index == 0 { json!([]) } else { json!(["LLVM"]) });
                if index == 1 {
                    assert_eq!(input["previous_draft"]["source"], "LLVM Clang crashes.");
                    assert_eq!(input["previous_draft"]["translation"], "Clang 崩溃。");
                } else {
                    assert!(input["previous_draft"].is_null());
                }
                if index == 0 {
                    draft_started.take().unwrap().send(()).unwrap();
                    final_arrived.take().unwrap().await.unwrap();
                }
                if source == "LLVM Sema?" {
                    reply(&mut socket, "429 Too Many Requests", json!({"error":"secret upstream data"})).await;
                } else {
                    reply(&mut socket, "200 OK", json!({"status":"completed",
                        "usage":{"input_tokens":50,"input_tokens_details":{"cached_tokens":5},"output_tokens":8},
                        "output":[{
                        "type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Clang 崩溃。"}]
                    }]})).await;
                }
            }
        });

        let (stop, mut stopped) = oneshot::channel();
        let stop = Mutex::new(Some(stop));
        let auth = authorization("local-only").unwrap();
        let mut connection = Connection::open(
            &ws_url, &["Clang".into()], auth.clone(), "local-session".into(), &mut stopped
        ).await.unwrap().unwrap();
        let (translate_emit, mut translated) = mpsc::unbounded_channel();
        let glossary = vec!["Clang".into()];
        let terms = std::sync::Arc::new(crate::terms::SessionTerms::new("local-session".into(), &glossary, |_| {}));
        let knowledge = Some(crate::knowledge::sample_cache(&["Clang", "Sema", "IR"]));
        let worker = luna::Worker::start(&http_url, auth, glossary, terms, knowledge, crate::language::Language::Zh, move |event| {
            translate_emit.send(event).unwrap();
        }).unwrap();
        let (tx, mut packets) = mpsc::channel(4);
        let speech = [0, 16].repeat(MIN_SAMPLES);
        tx.send(Packet::Pcm(speech.clone())).await.unwrap();
        let events = Mutex::new(Vec::new());
        let audio_samples = Mutex::new(0);
        connection.stream(&mut packets, &mut stopped, &|event| {
            if let StreamEvent::AudioSent(samples) = &event {
                *audio_samples.lock().unwrap() += samples;
            }
            if let StreamEvent::Transcript(frame) = &event {
                assert_eq!(frame.session_id, "local-session");
                let token = &frame.tokens[0];
                assert!(token.speaker.is_none());
                if !token.is_final && token.text == "LLVM Clang crashes." {
                    tx.try_send(Packet::Pcm(vec![0; SENTENCE_QUIET_SAMPLES * 2])).unwrap();
                    tx.try_send(Packet::Pcm(speech.clone())).unwrap();
                }
                if !token.is_final && token.text == "Sema" {
                    stop.lock().unwrap().take().unwrap().send(()).unwrap();
                }
                events.lock().unwrap().push((token.text.clone(), token.is_final, token.revision));
                if token.is_final && token.text == "LLVM Sema?" {
                    final_seen.lock().unwrap().take().unwrap().send(()).unwrap();
                }
            }
            worker.submit(&event).unwrap();
        }).await.unwrap();
        worker.finish().await.unwrap();
        ws_server.await.unwrap();
        http_server.await.unwrap();
        assert_eq!(*events.lock().unwrap(), [
            ("LLVM ".into(), false, Some(1)), ("LLVM Clang crashes.".into(), false, Some(2)),
            ("Sema".into(), false, Some(1)), ("LLVM Clang crashes.".into(), true, Some(3)), ("LLVM Sema?".into(), true, Some(2))
        ]);
        assert_eq!(*audio_samples.lock().unwrap(), MIN_SAMPLES * 2 + SENTENCE_QUIET_SAMPLES);
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationStarted)));
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationUsage(Some(_)))));
        let StreamEvent::Transcript(frame) = translated.recv().await.unwrap() else { panic!("expected translation") };
        assert_eq!(frame.session_id, "local-session");
        assert_eq!(frame.tokens[0].text, "Clang 崩溃。");
        assert_eq!(frame.tokens[0].source_segment_id.as_deref(), Some("item-1"));
        assert_eq!(frame.tokens[0].revision, Some(2));
        assert!(!frame.tokens[0].is_final);
        assert_eq!(frame.tokens[0].translation_source, Some(Provider::Openai));
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationStarted)));
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationUsage(Some(_)))));
        let StreamEvent::Transcript(frame) = translated.recv().await.unwrap() else { panic!("expected final translation") };
        assert_eq!(frame.tokens[0].revision, Some(3));
        assert!(frame.tokens[0].is_final);
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationStarted)));
        assert!(matches!(translated.recv().await, Some(StreamEvent::TranslationUsage(None))));
        let StreamEvent::TranslationWarning(warning) = translated.recv().await.unwrap() else { panic!("expected warning") };
        assert!(warning.contains("超限") && !warning.contains("secret"));
        let StreamEvent::Transcript(frame) = translated.recv().await.unwrap() else { panic!("expected cleared draft") };
        assert_eq!(frame.tokens[0].source_segment_id.as_deref(), Some("item-2"));
        assert!(frame.tokens[0].is_final && frame.tokens[0].text.is_empty());
        assert!(translated.recv().await.is_none());
    }).await.unwrap();
}

#[derive(serde::Deserialize)]
struct TopicBenchmarkClip {
    pcm: String,
    topic: String,
}

async fn replay_topic_benchmark(
    pcm: &[u8],
    auth: HeaderValue,
    glossary: &[String],
    cache: crate::knowledge::KnowledgeCache,
    topic: &str,
) -> Result<Value, String> {
    use crate::{knowledge::merge_asr_terms, terms::SessionTerms};
    use std::sync::Arc;

    let session_id = uuid::Uuid::new_v4().to_string();
    let keywords = merge_asr_terms(glossary, Some(&cache), topic);
    let (stop, mut stopped) = oneshot::channel();
    let mut connection = Connection::open(
        ENDPOINT,
        &keywords,
        auth.clone(),
        session_id.clone(),
        &mut stopped,
    )
    .await?
    .ok_or("Benchmark stopped before the session opened")?;
    let terms = Arc::new(SessionTerms::new(session_id, &keywords, |_| {}));
    let translations = Arc::new(Mutex::new(Vec::new()));
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let worker = luna::Worker::start(
        luna::ENDPOINT,
        auth,
        glossary.to_vec(),
        terms.clone(),
        Some(cache),
        crate::language::Language::Zh,
        {
            let translations = translations.clone();
            let warnings = warnings.clone();
            move |event| match event {
                StreamEvent::Transcript(frame) => translations
                    .lock()
                    .unwrap()
                    .extend(frame.tokens.into_iter().filter(|token| token.is_final)),
                StreamEvent::TranslationWarning(message) => warnings.lock().unwrap().push(message),
                _ => {}
            }
        },
    )?;
    let originals = Mutex::new(Vec::new());
    let (tx, mut packets) = mpsc::channel(8);
    let feed = async move {
        let mut clock = tokio::time::interval(Duration::from_millis(20));
        for chunk in pcm.chunks(SAMPLE_RATE as usize * 2 / 50) {
            clock.tick().await;
            if tx.send(Packet::Pcm(chunk.to_vec())).await.is_err() {
                return;
            }
        }
        let _ = stop.send(());
    };
    let emit = |event| {
        if let StreamEvent::Transcript(frame) = &event {
            originals
                .lock()
                .unwrap()
                .extend(frame.tokens.iter().filter(|t| t.is_final).cloned());
        }
        if let Err(message) = worker.submit(&event) {
            warnings.lock().unwrap().push(message);
        }
    };
    let receive = connection.stream(&mut packets, &mut stopped, &emit);
    let (_, result) = tokio::join!(feed, receive);
    result?;
    worker.finish().await?;
    Ok(json!({
        "keywords": keywords,
        "originals": originals.into_inner().unwrap(),
        "translations": *translations.lock().unwrap(),
        "warnings": *warnings.lock().unwrap(),
    }))
}

#[tokio::test]
#[ignore = "opt-in live OpenAI A/B test using public or explicitly authorized audio"]
async fn live_meeting_topic_benchmark() {
    use crate::knowledge::KnowledgeCache;
    use std::{fs, path::PathBuf};

    let root = PathBuf::from(std::env::var_os("LIVECAP_TOPIC_BENCH").unwrap());
    let manifest: Vec<TopicBenchmarkClip> =
        serde_json::from_slice(&fs::read(root.join("clips.json")).unwrap()).unwrap();
    let db = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
        .join("com.chenmiaoi.livecap")
        .join("livecap.sqlite3");
    let db = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let cache: String = db
        .query_row("SELECT data FROM knowledge_cache WHERE id = 1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let glossary: Vec<String> = db
        .query_row(
            "SELECT coalesce(json_extract(data, '$.terms'), '[]') FROM settings WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|terms| serde_json::from_str(&terms).ok())
        .unwrap_or_default();
    let key = crate::credentials::Credentials::default()
        .get(Provider::Openai)
        .unwrap()
        .expect("OpenAI credential is missing");
    let auth = authorization(&key).unwrap();
    drop(key);

    for clip in manifest {
        let pcm = fs::read(root.join(&clip.pcm)).unwrap();
        assert!(!pcm.is_empty() && pcm.len() % 2 == 0);
        eprintln!("Running both variants on {}", clip.pcm);
        let (baseline, selected) = tokio::time::timeout(
            Duration::from_secs(pcm.len() as u64 / (u64::from(SAMPLE_RATE) * 2) + 90),
            async {
                tokio::join!(
                    replay_topic_benchmark(
                        &pcm,
                        auth.clone(),
                        &glossary,
                        serde_json::from_str::<KnowledgeCache>(&cache).unwrap(),
                        ""
                    ),
                    replay_topic_benchmark(
                        &pcm,
                        auth.clone(),
                        &glossary,
                        serde_json::from_str::<KnowledgeCache>(&cache).unwrap(),
                        &clip.topic
                    ),
                )
            },
        )
        .await
        .expect("Benchmark replay timed out");
        let result = json!({
            "pcm": clip.pcm, "topic": clip.topic,
            "baseline": baseline, "selected": selected,
        });
        fs::write(
            root.join(format!("{}.json", clip.pcm)),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
        assert!(
            result["baseline"]["Ok"].is_object() && result["selected"]["Ok"].is_object(),
            "Replay failed: {result}"
        );
    }
}

#[tokio::test]
async fn pause_commits_audio_keeps_connection_alive_and_resumes_without_timeline_gap() {
    timeout(Duration::from_secs(12), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}", listener.local_addr().unwrap());
        let (resume_tx, resume_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut socket = accept_async(listener.accept().await.unwrap().0).await.unwrap();
            next(&mut socket, "session.update").await;
            send(&mut socket, json!({"type":"session.updated"})).await;
            next(&mut socket, "input_audio_buffer.append").await;
            next(&mut socket, "input_audio_buffer.commit").await;
            send(&mut socket, json!({"type":"input_audio_buffer.committed","item_id":"before"})).await;
            send(&mut socket, json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"before","transcript":"Bonjour"})).await;
            // Nothing from the paused period is appended; only a WebSocket ping arrives.
            let ping = socket.next().await.unwrap().unwrap();
            assert!(matches!(ping, Message::Ping(_)));
            socket.send(Message::Pong(Vec::new().into())).await.unwrap();
            resume_tx.send(()).unwrap();
            next(&mut socket, "input_audio_buffer.append").await;
            next(&mut socket, "input_audio_buffer.commit").await;
            send(&mut socket, json!({"type":"input_audio_buffer.committed","item_id":"after","previous_item_id":"before"})).await;
            send(&mut socket, json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"after","transcript":"こんにちは"})).await;
        });
        let (stop_tx, mut stopped) = oneshot::channel();
        let mut connection = Connection::open(&endpoint, &[], authorization("test-only").unwrap(), "pause-test".into(), &mut stopped).await.unwrap().unwrap();
        let (tx, mut packets) = mpsc::channel(8);
        let feed = std::sync::Arc::new(crate::audio_feed::AudioFeed::new(tx));
        let speech = [0, 16].repeat(MIN_SAMPLES);
        feed.forward(&speech, |_| Ok(())).unwrap();
        feed.set_paused(true).unwrap();
        feed.forward(&speech, |_| panic!("paused audio must not be recorded")).unwrap();
        let resumed_feed = feed.clone();
        let resume = tokio::spawn(async move {
            resume_rx.await.unwrap();
            resumed_feed.set_paused(false).unwrap();
            resumed_feed.forward(&speech, |_| Ok(())).unwrap();
            resumed_feed.set_paused(true).unwrap();
            stop_tx.send(()).unwrap(); // Stopping while paused must drain and complete too.
        });
        let transcripts = Mutex::new(Vec::new());
        let sent_samples = Mutex::new(0);
        connection.stream(&mut packets, &mut stopped, &|event| match event {
            StreamEvent::Transcript(frame) => transcripts.lock().unwrap().push(frame),
            StreamEvent::AudioSent(samples) => *sent_samples.lock().unwrap() += samples,
            _ => {}
        }).await.unwrap();
        server.await.unwrap();
        resume.await.unwrap();
        let transcripts = transcripts.lock().unwrap();
        assert_eq!(transcripts.len(), 2);
        assert_eq!(transcripts[0].tokens[0].end_ms, Some(100));
        assert_eq!(transcripts[1].tokens[0].start_ms, Some(100));
        assert_eq!(transcripts[1].tokens[0].end_ms, Some(200));
        assert_eq!(*sent_samples.lock().unwrap(), MIN_SAMPLES * 2);
    }).await.unwrap();
}

#[test]
fn transcription_diagnostics_distinguish_quota_rate_configuration_and_unknown_failures() {
    for (code, expected) in [
        ("insufficient_quota", "额度不足"),
        ("rate_limit_exceeded", "请求速率"),
        ("invalid_api_key", "认证失败"),
        ("model_not_found", "模型不可用"),
        ("unknown_parameter", "配置"),
        ("server_error", "服务暂时异常"),
        ("unexpected_failure", "unexpected_failure"),
    ] {
        let message = transcription_error(
            &json!({"error":{"code":code,"message":"secret upstream data","param":"session.audio.input"}}),
        );
        assert!(message.contains(expected), "{message}");
        assert!(message.contains(code));
        assert!(message.contains("session.audio.input"));
        assert!(!message.contains("secret"));
    }
    let message =
        transcription_error(&json!({"error":{"code":"sk-secret-token","param":"Bearer secret"}}));
    assert!(!message.contains("secret"));
    assert!(!message.contains("账户"));
}

#[tokio::test]
async fn unintelligible_audio_clears_its_draft_and_unblocks_later_turns() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut socket = accept_async(listener.accept().await.unwrap().0)
            .await
            .unwrap();
        next(&mut socket, "session.update").await;
        send(&mut socket, json!({"type":"session.updated"})).await;
        socket
    });
    let mut connection = Connection::handshake(
        &endpoint,
        &[],
        authorization("test-only").unwrap(),
        "recovery".into(),
    )
    .await
    .unwrap();
    let _server_socket = server.await.unwrap();
    for id in ["unintelligible", "later", "after-recovery"] {
        connection.pending.push_back(Pending {
            id: Some(id.into()),
            final_text: None,
            start: 0,
            end: 24000,
            since: Instant::now(),
        });
    }
    let events = Mutex::new(Vec::new());
    let emit = |event| events.lock().unwrap().push(event);
    connection.receive(json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"unintelligible","delta":"draft"}), &emit).unwrap();
    connection.receive(json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"later","transcript":"Confirmed later"}), &emit).unwrap();
    connection.receive(json!({"type":"conversation.item.input_audio_transcription.failed","item_id":"unintelligible","error":{"code":"audio_unintelligible"}}), &emit).unwrap();
    assert!(connection.drafts.is_empty());
    assert_eq!(connection.pending.len(), 1);
    connection.receive(json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"after-recovery","transcript":"Meeting continues"}), &emit).unwrap();
    assert!(connection.pending.is_empty());
    let events = events.into_inner().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, StreamEvent::TranslationWarning(_)))
            .count(),
        1
    );
    let final_texts: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::Transcript(frame) if frame.tokens[0].is_final => {
                Some(frame.tokens[0].text.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(final_texts, ["", "Confirmed later", "Meeting continues"]);
    assert!(
        connection
            .receive(
                json!({"type":"error","error":{"code":"insufficient_quota"}}),
                &|_| {}
            )
            .unwrap_err()
            .contains("额度不足")
    );
}
