use super::*;
use base64::Engine as _;
use std::time::Duration;
fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}
pub fn probe(rt: &Runtime, path: &Path, settings: &Value) -> Result<MediaInfo> {
    let s = &settings["media"];
    let raw = process::run(
        rt,
        s["ffprobe"].as_str().unwrap_or("ffprobe"),
        &args(&[
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
            &path.to_string_lossy(),
        ]),
        s["timeout"].as_u64().unwrap_or(300),
    )?;
    let v: Value = serde_json::from_str(&raw)?;
    let number = |v: &Value| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    };
    let duration = text::milliseconds(
        number(&v["format"]["duration"])
            .ok_or_else(|| Error::operation("Media duration is unavailable"))?,
    )?;
    if duration == 0 {
        return Err(Error::input("Media duration must be positive"));
    }
    let streams = v["streams"]
        .as_array()
        .ok_or_else(|| Error::operation("No media streams"))?;
    let audio = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .ok_or_else(|| Error::input("The source has no audio stream"))?;
    let origin = number(&v["format"]["start_time"]).unwrap_or(0.0);
    let start = number(&audio["start_time"]).unwrap_or(origin);
    Ok(MediaInfo {
        duration_ms: duration,
        audio_index: audio["index"]
            .as_u64()
            .ok_or_else(|| Error::operation("Invalid audio stream index"))?,
        video_index: streams
            .iter()
            .find(|s| s["codec_type"] == "video")
            .and_then(|s| s["index"].as_u64()),
        audio_offset_ms: text::milliseconds((start - origin).max(0.0))?,
    })
}
fn language(code: &str) -> &str {
    match code {
        "eng" => "en",
        "slv" => "sl",
        "pol" => "pl",
        "deu" => "de",
        "fra" => "fr",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "rus" => "ru",
        "jpn" => "ja",
        "zho" => "zh",
        "kor" => "ko",
        "ara" => "ar",
        "nld" => "nl",
        "ces" => "cs",
        "ukr" => "uk",
        "hrv" => "hr",
        "srp" => "sr",
        v => v,
    }
}
pub fn relisten(rt: &Runtime, request: &Request, settings: &Value, cue: &Cue) -> Result<String> {
    let (start, end) = cue
        .start_ms
        .zip(cue.end_ms)
        .filter(|(start, end)| end > start)
        .ok_or_else(|| Error::input("Second ASR requires a timed source interval"))?;
    let info = probe(rt, Path::new(&request.source), settings)?;
    if end > info.duration_ms {
        return Err(Error::input("Second-ASR interval exceeds the source"));
    }
    let wave = rt.path(&format!(
        ".cache/relisten/{}.wav",
        digest(&json!([request.source_hash, start, end]))
    ))?;
    fs::create_dir_all(wave.parent().unwrap())?;
    process::run(
        rt,
        settings["media"]["ffmpeg"].as_str().unwrap_or("ffmpeg"),
        &args(&[
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-i",
            &request.source,
            "-ss",
            &format!("{:.3}", start as f64 / 1000.0),
            "-t",
            &format!("{:.3}", (end - start) as f64 / 1000.0),
            "-map",
            &format!("0:{}", info.audio_index),
            "-vn",
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            &wave.to_string_lossy(),
        ]),
        settings["media"]["timeout"].as_u64().unwrap_or(300),
    )?;
    let mut cropped = request.clone();
    cropped.source = wave.to_string_lossy().into();
    cropped.source_hash = file_hash(&wave)?;
    let info = probe(rt, &wave, settings)?;
    let value = transcribe_with_namespace(rt, &cropped, settings, &info, "second-asr")?;
    let text = value["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|segment| segment["text"].as_str())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    nonblank(&text, "Second-ASR evidence")?;
    Ok(text)
}
pub fn transcribe(
    rt: &Runtime,
    request: &Request,
    settings: &Value,
    info: &MediaInfo,
) -> Result<Value> {
    transcribe_with_namespace(rt, request, settings, info, "initial-asr")
}
fn transcribe_with_namespace(
    rt: &Runtime,
    request: &Request,
    settings: &Value,
    info: &MediaInfo,
    namespace: &str,
) -> Result<Value> {
    let s = &settings["asr"];
    let backend = s["backend"].as_str().unwrap_or("local");
    let step = s["chunk_seconds"].as_u64().unwrap_or(600) * 1000;
    let length = info.duration_ms.saturating_sub(info.audio_offset_ms);
    let mut segments = Vec::new();
    let lang = s["language"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| language(&request.language));
    let identity = Job {
        implementation: String::new(),
        schema_version: 1,
        created: 0,
        revision: 0,
        request: request.clone(),
        settings: settings.clone(),
        state: String::new(),
        message: String::new(),
        stages: BTreeMap::new(),
        pending: None,
        history: vec![],
        decisions: BTreeMap::new(),
        cards: 0,
        omitted: 0,
        input_hashes: BTreeMap::new(),
        exports: BTreeMap::new(),
    };
    let dependencies = json!([
        process::dependencies("asr", &identity)?,
        process::dependencies("media", &identity)?
    ]);
    for start in (0..length).step_by(step as usize) {
        let duration = step.min(length - start);
        let offset = start + info.audio_offset_ms;
        let wave = rt.path(&format!(".cache/asr/audio-{offset}-{duration}.wav"))?;
        fs::create_dir_all(wave.parent().unwrap())?;
        let key = digest(&json!([
            namespace,
            request.source_hash,
            request.language,
            offset,
            duration,
            s,
            dependencies
        ]));
        let cache = format!(".cache/asr/{key}.json");
        let data = if let Ok(bytes) = rt.read(&cache) {
            let saved: Value = serde_json::from_slice(&bytes)?;
            if saved["checksum"] != digest(&saved["value"]) {
                return Err(Error::operation("Transcription cache checksum mismatch"));
            }
            saved["value"].clone()
        } else {
            process::run(
                rt,
                settings["media"]["ffmpeg"].as_str().unwrap_or("ffmpeg"),
                &args(&[
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-i",
                    &request.source,
                    "-ss",
                    &format!("{:.3}", offset as f64 / 1000.0),
                    "-t",
                    &format!("{:.3}", duration as f64 / 1000.0),
                    "-map",
                    &format!("0:{}", info.audio_index),
                    "-vn",
                    "-ar",
                    "16000",
                    "-ac",
                    "1",
                    "-c:a",
                    "pcm_s16le",
                    &wave.to_string_lossy(),
                ]),
                settings["media"]["timeout"].as_u64().unwrap_or(300),
            )?;
            if config::is_local_worker(s) {
                start_worker(rt, s)?;
            }
            let mut data = if backend == "local" {
                let model = s["model_file"]
                    .as_str()
                    .filter(|m| !m.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        config::data_directory().join(format!(
                            "resources/models/ggml-{}.bin",
                            s["model"].as_str().unwrap_or("small")
                        ))
                    });
                if !model.is_file() {
                    return Err(Error::operation(format!(
                        "Install the preconverted whisper.cpp model through Tools & downloads, or set asr.model_file ({}). No conversion scripts are needed.",
                        model.display()
                    )));
                }
                let prefix = rt.path(&format!(".cache/asr/whisper-{key}"))?;
                let mut argv = args(&[
                    "-m",
                    &model.to_string_lossy(),
                    "-f",
                    &wave.to_string_lossy(),
                    "-l",
                    lang,
                    "-oj",
                    "-of",
                    &prefix.to_string_lossy(),
                ]);
                if s["device"] == "cpu" {
                    argv.push("-ng".into());
                }
                process::run(
                    rt,
                    s["executable"].as_str().unwrap_or("whisper-cli"),
                    &argv,
                    s["timeout"].as_u64().unwrap_or(7200),
                )?;
                let raw: Value = serde_json::from_slice(&fs::read(prefix.with_extension("json"))?)?;
                if raw["segments"].is_array() {
                    raw
                } else {
                    let rows = raw["transcription"].as_array().ok_or_else(|| {
                        Error::operation("whisper.cpp returned no transcription segments")
                    })?;
                    json!({"segments":rows.iter().map(|r|json!({"text":r["text"],"start":r["offsets"]["from"].as_f64().unwrap_or(0.0)/1000.0,"end":r["offsets"]["to"].as_f64().unwrap_or(0.0)/1000.0})).collect::<Vec<_>>()})
                }
            } else if backend == "groq" {
                let url = s["endpoint"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("https://api.groq.com/openai/v1/audio/transcriptions");
                config::validate_url(url)?;
                let key_env = s["api_key_env"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("GROQ_API_KEY");
                let auth = providers::secret(rt, key_env)?;
                let mut result = None;
                for attempt in 0..=s["retries"].as_u64().unwrap_or(2).min(10) {
                    if rt.cancelled() {
                        return Err(Error::paused());
                    }
                    let form = reqwest::blocking::multipart::Form::new()
                        .file("file", &wave)
                        .map_err(|_| Error::operation("Could not read normalized audio"))?
                        .text(
                            "model",
                            if s["model"] == "small" {
                                "whisper-large-v3-turbo"
                            } else {
                                s["model"].as_str().unwrap_or("whisper-large-v3-turbo")
                            }
                            .to_string(),
                        )
                        .text("language", lang.to_string())
                        .text("response_format", "verbose_json")
                        .text("timestamp_granularities[]", "segment");
                    let response = providers::client(s["timeout"].as_u64().unwrap_or(7200))?
                        .post(url)
                        .bearer_auth(&auth)
                        .header("Idempotency-Key", &key)
                        .multipart(form)
                        .send()
                        .map_err(|_| Error::operation("Hosted transcription connection failed"))
                        .and_then(|r| providers::json_response(r, 16 * 1024 * 1024));
                    match response {
                        Err(e)
                            if e.code == "transport_retryable"
                                && attempt < s["retries"].as_u64().unwrap_or(2) =>
                        {
                            std::thread::sleep(Duration::from_secs(1 << attempt.min(3)))
                        }
                        r => {
                            result = Some(r?);
                            break;
                        }
                    }
                }
                result.ok_or_else(|| Error::operation("Hosted transcription retries exhausted"))?
            } else {
                let url = s["endpoint"]
                    .as_str()
                    .ok_or_else(|| Error::input("Set asr.endpoint for the HTTP worker"))?;
                let env = s["api_key_env"].as_str().unwrap_or("");
                let mut headers = BTreeMap::from([("Idempotency-Key".into(), key.clone())]);
                if !env.is_empty() {
                    headers.insert(
                        "Authorization".into(),
                        format!("Bearer {}", providers::secret(rt, env)?),
                    );
                }
                providers::post(
                    rt,
                    url,
                    &json!({"audio":base64::engine::general_purpose::STANDARD.encode(fs::read(&wave)?),"filename":"audio.wav","language":request.language,"model":s["model"],"compute_type":s["remote_compute_type"],"options":s["options"]}),
                    &headers,
                    s["timeout"].as_u64().unwrap_or(7200),
                    s["retries"].as_u64().unwrap_or(2),
                )?
            };
            if let Some(m) = data.as_object_mut() {
                m.remove("blitzline_audio_offset_ms");
            }
            text::normalize_asr(&data, duration, s)?;
            rt.write(
                &cache,
                &serde_json::to_vec(&json!({"checksum":digest(&data),"value":data}))?,
            )?;
            let _ = fs::remove_file(&wave);
            data
        };
        // Chunk results are relative to the selected audio stream, then restored once.
        text::normalize_asr(&data, duration, s)?;
        for mut segment in data["segments"]
            .as_array()
            .ok_or_else(|| Error::operation("Missing ASR segments"))?
            .clone()
        {
            for bound in ["start", "end"] {
                let v = segment[bound]
                    .as_f64()
                    .ok_or_else(|| Error::operation("Invalid chunk timing"))?;
                segment[bound] = json!(v + offset as f64 / 1000.0);
            }
            if let Some(words) = segment["words"].as_array_mut() {
                for w in words {
                    for bound in ["start", "end"] {
                        if let Some(v) = w[bound].as_f64() {
                            w[bound] = json!(v + offset as f64 / 1000.0);
                        }
                    }
                }
            }
            segments.push(segment);
        }
    }
    let data = json!({"segments":segments});
    text::normalize_asr(&data, info.duration_ms, s)?;
    Ok(data)
}
pub fn clips(
    rt: &Runtime,
    cards: &[Card],
    request: &Request,
    info: &MediaInfo,
    settings: &Value,
) -> Result<(Vec<Card>, Vec<Value>)> {
    let cues: Vec<Cue> = serde_json::from_slice(&rt.read("artifacts/02-proofread.json")?)?;
    let s = &settings["media"];
    let encoder = process::dependencies(
        "media",
        &Job {
            implementation: String::new(),
            schema_version: 1,
            created: 0,
            revision: 0,
            request: request.clone(),
            settings: settings.clone(),
            state: String::new(),
            message: String::new(),
            stages: BTreeMap::new(),
            pending: None,
            history: vec![],
            decisions: BTreeMap::new(),
            cards: 0,
            omitted: 0,
            input_hashes: BTreeMap::new(),
            exports: BTreeMap::new(),
        },
    )?;
    let mut output = Vec::new();
    let mut errors = Vec::new();
    for card in cards {
        let mut card = card.clone();
        let mut failed = false;
        for context in &mut card.contexts {
            *context = vocabulary::shorten(
                context,
                settings["export"]["max_sentence_words"]
                    .as_u64()
                    .unwrap_or(25) as usize,
            );
            let result = (|| -> Result<String> {
                vocabulary::retime(context, &cues)?;
                if !context.error.is_empty() || context.cue_ids.is_empty() {
                    return Err(Error::input("Context has no reliable source match"));
                }
                let (start, end) = context
                    .start_ms
                    .zip(context.end_ms)
                    .ok_or_else(|| Error::input("Media context has no timing"))?;
                let start = start.saturating_sub(s["padding_ms"].as_u64().unwrap_or(200));
                let end = end
                    .saturating_add(s["padding_ms"].as_u64().unwrap_or(200))
                    .min(info.duration_ms);
                if end <= start || end - start > s["max_clip_ms"].as_u64().unwrap_or(45000) {
                    return Err(Error::input(
                        "Example clip is invalid or too long; choose another example",
                    ));
                }
                let video = s["kind"] == "video";
                if video && info.video_index.is_none() {
                    return Err(Error::input("Video examples require a video stream"));
                }
                let plan = json!({"source":request.source_hash,"start_ms":start,"end_ms":end,"kind":s["kind"],"audio_codec":s["audio_codec"],"video_codec":s["video_codec"],"encoder":encoder});
                let name = format!(
                    "blitzline-{}.{}",
                    &digest(&plan)[..24],
                    if video { "mp4" } else { "mp3" }
                );
                let path = rt.path(&format!("media/{name}"))?;
                let receipt = format!("media/{name}.json");
                if let Ok(raw) = rt.read(&receipt) {
                    let v: Value = serde_json::from_slice(&raw)?;
                    if v["plan"] == plan && path.is_file() && v["checksum"] == file_hash(&path)? {
                        return Ok(name);
                    }
                }
                fs::create_dir_all(path.parent().unwrap())?;
                let temporary = path.with_file_name(format!(".encoding-{name}"));
                let mut argv = args(&[
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-i",
                    &request.source,
                    "-ss",
                    &format!("{:.3}", start as f64 / 1000.0),
                    "-t",
                    &format!("{:.3}", (end - start) as f64 / 1000.0),
                    "-map",
                    &format!("0:{}", info.audio_index),
                ]);
                if video {
                    argv.extend(args(&[
                        "-map",
                        &format!("0:{}", info.video_index.unwrap()),
                        "-c:v",
                        s["video_codec"].as_str().unwrap_or("libx264"),
                        "-pix_fmt",
                        "yuv420p",
                        "-c:a",
                        "aac",
                        "-movflags",
                        "+faststart",
                    ]));
                } else {
                    argv.extend(args(&[
                        "-vn",
                        "-c:a",
                        s["audio_codec"].as_str().unwrap_or("libmp3lame"),
                        "-q:a",
                        "2",
                    ]));
                }
                argv.push(temporary.to_string_lossy().into());
                process::run(
                    rt,
                    s["ffmpeg"].as_str().unwrap_or("ffmpeg"),
                    &argv,
                    s["timeout"].as_u64().unwrap_or(300),
                )?;
                let checked = probe(rt, &temporary, settings)?;
                if checked.duration_ms.abs_diff(end - start) > 500
                    || video && checked.video_index.is_none()
                {
                    let _ = fs::remove_file(&temporary);
                    return Err(Error::operation(
                        "Generated example failed media verification",
                    ));
                }
                fs::rename(&temporary, &path)?;
                rt.write(
                    &receipt,
                    &serde_json::to_vec(&json!({"plan":plan,"checksum":file_hash(&path)?}))?,
                )?;
                Ok(name)
            })();
            match result {
                Ok(name) => card.media.push(name),
                Err(e) if e.code == "paused" => return Err(e),
                Err(e) => {
                    errors
                        .push(json!({"card_id":card.id,"context_id":context.id,"error":e.message}));
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            output.push(card);
        }
    }
    Ok((output, errors))
}
/// Make sure the ASR worker answers, launching `asr.worker_command` if it does not.
fn start_worker(rt: &Runtime, asr: &Value) -> Result<()> {
    use std::{net::{TcpStream, ToSocketAddrs}, process::{Command, Stdio}};
    let up = || {
        config::local_worker_address(asr)
            .ok()
            .and_then(|a| a.to_socket_addrs().ok()?.next())
            .is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
    };
    config::local_worker_address(asr)?;
    if up() {
        return Ok(());
    }
    let cmd: Vec<&str> = asr["worker_command"].as_array().map(|c| c.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let Some((program, rest)) = cmd.split_first() else {
        return Err(Error::operation("The ASR worker is not running. Start it, or set asr.worker_command so Blitzline can."));
    };
    // The worker exits on its own once this process is gone (it polls BLITZLINE_PARENT_PID).
    let log = std::env::temp_dir().join("blitzline-worker.log");
    let stderr = std::fs::File::create(&log).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .env("BLITZLINE_PARENT_PID", std::process::id().to_string())
        .spawn()
        .map_err(|_| Error::operation("Could not launch asr.worker_command"))?;
    for _ in 0..360 {
        if rt.cancelled() {
            return Err(Error::paused());
        }
        if up() {
            return Ok(());
        }
        if child.try_wait().ok().flatten().is_some() {
            let why = std::fs::read_to_string(&log).unwrap_or_default();
            let why = why.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no output").to_string();
            return Err(Error::operation(format!("The ASR worker exited while starting: {why} (full log: {})", log.display())));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(Error::operation("The ASR worker did not start within three minutes"))
}
