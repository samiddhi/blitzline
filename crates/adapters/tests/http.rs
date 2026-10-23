use blitzline_adapters::pipeline::{self as rt, Runtime, config};
use blitzline_engine::pipeline::Pipeline;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(mut handler: impl FnMut(Value, &str) -> (u16, Value) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        s.set_nonblocking(false).unwrap();
                        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                        let mut bytes = Vec::new();
                        let mut buf = [0u8; 4096];
                        let (header_end, total) = loop {
                            let n = s.read(&mut buf).unwrap();
                            if n == 0 {
                                break (0, 0);
                            }
                            bytes.extend_from_slice(&buf[..n]);
                            if let Some(pos) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&bytes[..pos]);
                                let length = head
                                    .lines()
                                    .find_map(|l| {
                                        l.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .and_then(|s| s.trim().parse::<usize>().ok())
                                    })
                                    .unwrap_or(0);
                                break (pos + 4, pos + 4 + length);
                            }
                        };
                        while bytes.len() < total {
                            let n = s.read(&mut buf).unwrap();
                            if n == 0 {
                                break;
                            }
                            bytes.extend_from_slice(&buf[..n]);
                        }
                        if header_end == 0 {
                            continue;
                        }
                        let header = String::from_utf8_lossy(&bytes[..header_end]);
                        let payload =
                            serde_json::from_slice(&bytes[header_end..]).unwrap_or(Value::Null);
                        let (status, response) = handler(payload, &header);
                        let body = response.to_string();
                        write!(s,"HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap()
}
fn setup(root: &Path, url: &str) -> Pipeline<Runtime> {
    let f = fixture();
    let source = root.join("sample.txt");
    fs::write(&source, f["raw"].as_str().unwrap()).unwrap();
    let vocab = root.join("vocabulary.json");
    fs::write(&vocab, f["rows"].to_string()).unwrap();
    let raw = json!({"profiles":{"native":{"provider":"compatible","base_url":url,"model":"test-model","api_key_env":"TEST_BLITZLINE_SECRET","retries":0,"timeout":3}},"llm":{"default":"native"},"run":{"method":"api"}});
    let s = config::merge(raw, root).unwrap();
    let mut p = rt::start(
        &root.join("run"),
        &source,
        "slv",
        "text",
        s,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    p.backend.verbose = false;
    p.backend.credentials.insert(
        "TEST_BLITZLINE_SECRET".into(),
        "TEST-SECRET-NEVER-SAVE".into(),
    );
    p.job.request.vocabulary_json = Some(vocab.to_string_lossy().into());
    p
}
fn completion(value: Value) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{"content":value.to_string()}}]})
}
fn good_reply(payload: &Value) -> Value {
    let data: Value =
        serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
    let ids = data["requested_ids"].as_array().unwrap();
    json!({"decisions":fixture()["decisions"].as_array().unwrap().iter().filter(|r|ids.contains(&r["candidate_id"])).collect::<Vec<_>>()})
}
#[test]
fn api_repairs_only_bad_items_and_keeps_secrets_out_of_saved_files() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let calls = seen.clone();
    let server = Server::new(move |p, header| {
        assert!(header.contains("Bearer TEST-SECRET-NEVER-SAVE"));
        let data: Value =
            serde_json::from_str(p["messages"][1]["content"].as_str().unwrap()).unwrap();
        let mut seen = calls.lock().unwrap();
        seen.push(data["requested_ids"].as_array().unwrap().len());
        let mut reply = good_reply(&p);
        if seen.len() == 1 {
            reply["decisions"][1]["english"] = json!("");
        }
        (200, completion(reply))
    });
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path(), &server.url);
    p.process(None).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![2, 1]);
    assert_eq!(p.job.cards, 2);
    p.process(None).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    for entry in walkdir::WalkDir::new(&p.backend.root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            assert!(
                !fs::read(entry.path())
                    .unwrap()
                    .windows(b"TEST-SECRET-NEVER-SAVE".len())
                    .any(|b| b == b"TEST-SECRET-NEVER-SAVE")
            );
        }
    }
}
#[test]
fn transport_failure_never_enters_the_content_repair_loop() {
    let calls = Arc::new(Mutex::new(0));
    let count = calls.clone();
    let server = Server::new(move |_, _| {
        *count.lock().unwrap() += 1;
        (503, json!({"error":"private provider data"}))
    });
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path(), &server.url);
    let error = p.process(None).unwrap_err();
    assert_eq!(error.code, "transport_retryable");
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(p.job.state, "failed");
    assert!(!error.message.contains("private"));
    assert!(p.job.pending.is_some());
}
#[test]
fn provider_routing_schema_and_reasoning_options_are_preserved() {
    let server = Server::new(move |p, _| {
        assert_eq!(p["response_format"]["type"], "json_schema");
        assert_eq!(p["provider"]["only"], json!(["chosen-provider"]));
        assert_eq!(p["provider"]["allow_fallbacks"], false);
        assert_eq!(p["reasoning"]["enabled"], false);
        assert_eq!(p["provider"]["max_price"]["input"], 0.5);
        (200, completion(good_reply(&p)))
    });
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path(), &server.url);
    let profile = &mut p.job.settings["profiles"]["native"];
    profile["provider"] = json!("openrouter");
    profile["response_format"] = json!("json_schema");
    profile["openrouter_provider"] = json!("chosen-provider");
    profile["reasoning_enabled"] = json!(false);
    profile["openrouter_max_price"] = json!({"input":0.5});
    p.process(None).unwrap();
}
#[test]
fn anki_delivery_recovers_and_updates_fields_without_scheduling_calls() {
    let notes = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = notes.clone();
    let updates = Arc::new(Mutex::new(0));
    let changed = updates.clone();
    let model = Arc::new(AtomicBool::new(false));
    let exists = model.clone();
    let server = Server::new(move |p, _| {
        let action = p["action"].as_str().unwrap();
        let v = match action {
            "version" => json!(6),
            "modelNames" => {
                if exists.load(Ordering::Relaxed) {
                    json!(["Blitzer Basic"])
                } else {
                    json!([])
                }
            }
            "createDeck" => json!(1),
            "createModel" => {
                exists.store(true, Ordering::Relaxed);
                Value::Null
            }
            "modelFieldNames" => json!([
                "Word",
                "Translation",
                "Sentence1",
                "Sentence2",
                "Comments",
                "Frequency",
                "Sentence1 Audio",
                "Sentence2 Audio",
                "Word Audio",
                "Image",
                "FillForTwoWay"
            ]),
            "findNotes" => {
                let notes = recorded.lock().unwrap();
                json!(
                    notes
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| n["tags"].as_array().unwrap().iter().any(|t| {
                            p["params"]["query"]
                                .as_str()
                                .unwrap()
                                .contains(t.as_str().unwrap())
                        }))
                        .map(|(i, _)| i + 1)
                        .collect::<Vec<_>>()
                )
            }
            "addNote" => {
                let mut n = recorded.lock().unwrap();
                n.push(p["params"]["note"].clone());
                json!(n.len())
            }
            "updateNoteFields" => {
                *changed.lock().unwrap() += 1;
                Value::Null
            }
            _ => panic!("Unexpected Anki action {action}"),
        };
        (200, json!({"result":v,"error":null}))
    });
    let dir = tempfile::tempdir().unwrap();
    let mut p = setup(dir.path(), &server.url);
    p.job.settings["run"]["method"] = json!("browser");
    assert_eq!(p.process(None).unwrap_err().code, "needs_review");
    let task = p.job.pending.clone().unwrap();
    p.submit(
        &json!({"exchange_id":task.id,"revision":task.revision,"decisions":fixture()["decisions"]})
            .to_string(),
    )
    .unwrap();
    p.process(None).unwrap();
    p.job.settings["export"]["format"] = json!("anki");
    p.job.settings["export"]["anki_url"] = json!(server.url);
    p.export_current().unwrap();
    p.export_current().unwrap();
    assert_eq!(notes.lock().unwrap().len(), 2);
    assert_eq!(*updates.lock().unwrap(), 0);
    p.job.settings["export"]["existing"] = json!("update");
    p.export_current().unwrap();
    assert_eq!(*updates.lock().unwrap(), 2);
    let receipt: Value = serde_json::from_slice(
        &fs::read(p.backend.root.join("exports/anki/anki-delivery.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["state"], "complete");
}

#[test]
fn http_asr_rebases_chunks_once_and_relistens_only_to_uncertain_audio() {
    use base64::Engine as _;
    if let Err(error) = rt::find_tool("ffmpeg") {
        assert!(
            std::env::var_os("BLITZLINE_REQUIRE_MEDIA_TESTS").is_none(),
            "{error}"
        );
        eprintln!("Skipping media HTTP test: {error}");
        return;
    }
    let counts = Arc::new(Mutex::new((0usize, 0usize)));
    let calls = counts.clone();
    let server = Server::new(move |payload, header| {
        let mut calls = calls.lock().unwrap();
        if payload.get("audio").is_some() {
            calls.0 += 1;
            let wave = base64::engine::general_purpose::STANDARD
                .decode(payload["audio"].as_str().unwrap())
                .unwrap();
            assert_eq!(&wave[..4], b"RIFF");
            assert_eq!(payload["filename"], "audio.wav");
            assert_eq!(payload["language"], "slv");
            assert!(header.to_ascii_lowercase().contains("idempotency-key:"));
            return (
                200,
                json!({"segments":[{"start":0.0,"end":0.5,"text":"Mačka spi.","words":[{"word":"Mačka","start":0.0,"end":0.2},{"word":"spi.","start":0.2,"end":0.5}]}]}),
            );
        }
        calls.1 += 1;
        let data: Value =
            serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
        if calls.1 == 2 {
            assert_eq!(data["requested"].as_array().unwrap().len(), 1);
            assert_eq!(data["second_asr"]["cue-000002"], "Mačka spi.");
        }
        let cues = data["requested"].as_array().unwrap().iter().map(|cue| json!({"cue_id":cue["cue_id"],"text":cue["text"],"uncertain":calls.1 == 1 && cue["cue_id"] == "cue-000002","reason":"Check unclear audio"})).collect::<Vec<_>>();
        (200, completion(json!({"cues":cues})))
    });
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.wav");
    let size = 16000u32 * 2 * 3;
    let mut wave = Vec::new();
    wave.extend(b"RIFF");
    wave.extend((36 + size).to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(1u16.to_le_bytes());
    wave.extend(16000u32.to_le_bytes());
    wave.extend(32000u32.to_le_bytes());
    wave.extend(2u16.to_le_bytes());
    wave.extend(16u16.to_le_bytes());
    wave.extend(b"data");
    wave.extend(size.to_le_bytes());
    wave.resize(wave.len() + size as usize, 0);
    fs::write(&source, wave).unwrap();
    // Video starts at zero; its selected audio stream starts one second later.
    let movie = dir.path().join("delayed-audio.mkv");
    let generated = std::process::Command::new(rt::find_tool("ffmpeg").unwrap())
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=16x16:r=1:d=4",
            "-itsoffset",
            "1",
            "-i",
        ])
        .arg(&source)
        .args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "ffv1",
            "-c:a",
            "pcm_s16le",
            "-t",
            "4",
        ])
        .arg(&movie)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let settings = config::merge(json!({"run":{"method":"api","proofread":true},"profiles":{"test":{"provider":"compatible","base_url":server.url,"model":"test","api_key_env":"TEST_KEY","retries":0}},"llm":{"default":"test"},"asr":{"backend":"http","endpoint":server.url,"chunk_seconds":1,"retries":0,"relisten":true}}),dir.path()).unwrap();
    let mut p = rt::start(
        &dir.path().join("run"),
        &movie,
        "slv",
        "media",
        settings,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    p.backend.verbose = false;
    p.backend
        .credentials
        .insert("TEST_KEY".into(), "test-credential".into());
    p.process(Some("proofread")).unwrap();
    assert_eq!(*counts.lock().unwrap(), (4, 2));
    let cues: Value = serde_json::from_slice(
        &fs::read(p.backend.root.join("artifacts/02-proofread.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cues[0]["start_ms"], 1000);
    assert_eq!(cues[1]["start_ms"], 2000);
    assert_eq!(cues[2]["start_ms"], 3000);
    p.process(Some("proofread")).unwrap();
    assert_eq!(*counts.lock().unwrap(), (4, 2));
}

#[test]
fn sparse_audit_is_batched_and_cannot_change_source_evidence() {
    let audited = Arc::new(Mutex::new(Vec::new()));
    let seen = audited.clone();
    let server = Server::new(move |payload, _| {
        let data: Value =
            serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
        if data.get("candidate_count").is_some() {
            assert!(data.get("original_candidates").is_none());
            assert_eq!(data["candidate_count"], 1);
            let term = data["candidates"][0]["term"].as_str().unwrap();
            seen.lock().unwrap().push(term.to_owned());
            let corrections = if term == "mačka" {
                json!([{"index":0,"decision":"keep","reason":"","english":"feline","context_indices":[0],"notes":""}])
            } else {
                json!([])
            };
            (200, completion(json!({"corrections":corrections})))
        } else {
            (200, completion(good_reply(&payload)))
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let mut p = setup(directory.path(), &server.url);
    p.job.settings["llm"]["batch_size"] = json!(1);
    p.job.settings["llm"]["audit_review"] = json!("native");
    p.process(None).unwrap();
    assert_eq!(*audited.lock().unwrap(), vec!["mačka", "čakati"]);
    assert_eq!(
        p.job.decisions["candidate-77073c4269187f69"].english,
        "feline"
    );
    let candidates: Value = serde_json::from_slice(
        &fs::read(p.backend.root.join("artifacts/04-vocabulary.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(candidates[0]["term"], "mačka");
    assert_eq!(candidates[0]["source_frequency"], 2);
}
