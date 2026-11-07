use super::*;
use crate::session::authorization;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::handshake::server::{Request, Response},
};

fn fixture() -> Vec<Value> {
    serde_json::from_str(include_str!("../../test-fixtures/soniox-stream.json")).unwrap()
}

#[test]
fn soniox_config_uses_embedded_translation() {
    let settings = Settings {
        terms: vec!["LLVM".into()],
        ..Settings::default()
    };
    let embedded = config(&settings);
    assert!(embedded.get("api_key").is_none());
    assert_eq!(embedded["audio_format"], "pcm_s16le");
    assert_eq!(embedded["sample_rate"], 16_000);
    assert_eq!(embedded["num_channels"], 1);
    assert_eq!(embedded["enable_speaker_diarization"], true);
    assert_eq!(embedded["translation"]["target_language"], "zh");
    assert_eq!(
        embedded["context"]["translation_terms"][0],
        json!({"source": "LLVM", "target": "LLVM"})
    );
    assert!(authorization("test-key").unwrap().is_sensitive());
    assert!(authorization("invalid\nkey").is_err());
}

#[test]
fn soniox_config_uses_selected_language_hint_and_target() {
    use crate::language::Language;
    let settings = Settings {
        source_language: Language::Ja,
        target_language: Language::De,
        ..Settings::default()
    };
    let config = config(&settings);
    assert_eq!(config["language_hints"], json!(["ja"]));
    assert_eq!(config["translation"]["target_language"], "de");
}

#[test]
fn malformed_frames_and_errors_do_not_become_caption_events() {
    for frame in [
        "invalid JSON",
        "{}",
        r#"{"tokens":[{"text":"missing is_final"}]}"#,
        r#"{"tokens":[{"text":"missing timestamps aren't errors","is_final":"yes"}]}"#,
    ] {
        assert_eq!(
            normalize(frame, "session-test").unwrap_err(),
            RESPONSE_ERROR
        );
    }
    let err = normalize(
        r#"{"error_code":401,"error_message":"fake secret that must stay private"}"#,
        "session-test",
    )
    .unwrap_err();
    assert_eq!(err, service_error(401));
    let event = normalize(r#"{"finished":true}"#, "session-test").unwrap();
    assert!(event.finished && event.tokens.is_empty());
}

#[tokio::test]
#[allow(clippy::result_large_err)] // Required by tungstenite's handshake callback type.
async fn local_socket_transcribes_mock_pcm_and_drains_final_translation() {
    timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/transcribe-websocket",
            listener.local_addr().unwrap()
        );
        let frames = fixture();
        let draft_count = frames.len() - 1;
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = accept_hdr_async(socket, |request: &Request, response: Response| {
                assert_eq!(request.uri().path(), "/transcribe-websocket");
                assert_eq!(request.headers()[AUTHORIZATION], "Bearer local-fake-key");
                Ok(response)
            })
            .await
            .unwrap();
            let first = socket.next().await.unwrap().unwrap();
            let config: Value = serde_json::from_str(first.into_text().unwrap().as_str()).unwrap();
            assert_eq!(config["translation"]["target_language"], "zh");
            assert!(config.get("api_key").is_none());

            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Binary(vec![0, 128].into())
            );
            for frame in &frames[..draft_count] {
                socket
                    .send(Message::Text(frame.to_string().into()))
                    .await
                    .unwrap();
            }
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Binary(vec![1, 2].into())
            );
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::Text("".into())
            );
            socket
                .send(Message::Text(frames[draft_count].to_string().into()))
                .await
                .unwrap();
        });
        let (mut stop_tx, mut stopped) = {
            let (tx, rx) = oneshot::channel();
            (Some(tx), rx)
        };
        let mut connection = Connection::open(
            &endpoint,
            &Settings::default(),
            authorization("local-fake-key").unwrap(),
            "session-test".into(),
            &mut stopped,
        )
        .await
        .unwrap()
        .unwrap();
        let (tx, mut packets) = mpsc::channel(2);
        tx.send(vec![0, 128]).await.unwrap();
        let mut events = Vec::new();
        let mut levels = Vec::new();
        connection
            .stream(&mut packets, &mut stopped, |event| match event {
                StreamEvent::Transcript(event) => {
                    events.push(event);
                    if events.len() == draft_count {
                        tx.try_send(vec![1, 2]).unwrap();
                        stop_tx.take().unwrap().send(()).unwrap();
                    }
                }
                StreamEvent::AudioLevel(level) => levels.push(level),
                StreamEvent::TranslationWarning(warning) => panic!("{warning}"),
                StreamEvent::AudioSent(_) => (),
                _ => panic!("Soniox should not emit OpenAI translation usage events"),
            })
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(levels, [1.0]);
        assert_eq!(events.len(), draft_count + 1);
        assert!(!events[0].tokens[0].is_final);
        assert!(events[1].tokens[0].is_final);
        assert_eq!(events[1].tokens[0].start_ms, Some(0));
        assert_eq!(events[1].tokens[0].end_ms, Some(340));
        assert!(!events[1].tokens[1].is_final);
        assert_eq!(events[2].tokens[1].speaker.as_deref(), Some("2"));
        let last = events.last().unwrap();
        assert!(last.finished && last.tokens[0].is_final);
        assert_eq!(last.tokens[0].text, "在 Sema 中吗？");
        assert_eq!(last.tokens[0].language.as_deref(), Some("zh"));
        assert_eq!(last.tokens[0].source_language.as_deref(), Some("en"));
        assert_eq!(last.tokens[0].translation_source, Some(Provider::Soniox));
    })
    .await
    .unwrap();
}
