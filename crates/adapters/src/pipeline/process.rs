use super::*;
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
pub fn resolve(executable: &str) -> Result<PathBuf> {
    let p = Path::new(executable);
    let mut options = Vec::new();
    if p.is_absolute() || p.components().count() > 1 {
        options.push(p.to_owned());
    } else {
        if let Ok(bin) = std::env::current_exe()
            && let Some(dir) = bin.parent()
        {
            options.push(dir.join("tools").join(p));
            options.push(dir.join("../Resources/tools").join(p));
            options.push(dir.join(p));
        }
        options.push(config::data_directory().join("resources/tools").join(p));
        // GUI apps launched from Finder or a desktop menu get a reduced PATH.
        let common = [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/Library/Frameworks/Python.framework/Versions/Current/bin",
            "/snap/bin",
        ]
        .map(PathBuf::from);
        let local = directories::BaseDirs::new().map(|b| b.home_dir().join(".local/bin"));
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path).chain(common).chain(local) {
            options.push(dir.join(p));
        }
    }
    for p in options {
        let mut paths = vec![p.clone()];
        if cfg!(windows) && p.extension().is_none() {
            paths.push(p.with_extension("exe"));
        }
        for p in paths {
            if p.is_file() {
                return p.canonicalize().map_err(Error::from);
            }
        }
    }
    Err(Error::operation(format!(
        "{executable} was not found. Choose it in Settings, install it, or use a Blitzline bundle containing it."
    )))
}
pub fn run(rt: &Runtime, executable: &str, args: &[String], timeout: u64) -> Result<String> {
    run_with(rt, executable, args, timeout, None)
}
/// Environment a sandboxed tool may inherit: enough to find its own login, nothing that changes its behaviour.
const ISOLATED_ENV: &[&str] = &[
    "HOME", "USER", "LOGNAME", "PATH", "LANG", "TERM", "TMPDIR", "TEMP", "TMP", "CLAUDE_CONFIG_DIR",
    "SYSTEMROOT", "USERPROFILE", "APPDATA", "LOCALAPPDATA",
];
/// Like `run`, but writes `input` to stdin and, in that case, also clears the environment (except
/// `ISOLATED_ENV`, `XDG_*` and `LC_*`) and starts in the temporary directory.
pub fn run_with(
    rt: &Runtime,
    executable: &str,
    args: &[String],
    timeout: u64,
    input: Option<&[u8]>,
) -> Result<String> {
    if rt.cancelled() {
        return Err(Error::paused());
    }
    let path = resolve(executable)?;
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if input.is_some() {
        command.env_clear().current_dir(std::env::temp_dir());
        for (k, v) in std::env::vars_os() {
            let name = k.to_string_lossy();
            if ISOLATED_ENV.contains(&name.as_ref()) || name.starts_with("XDG_") || name.starts_with("LC_") {
                command.env(k, v);
            }
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| Error::operation(format!("Could not start {executable}: {e}")))?;
    // Written from a thread so a child that answers before reading everything cannot deadlock us.
    let writer = input.zip(child.stdin.take()).map(|(bytes, mut stdin)| {
        let bytes = bytes.to_vec();
        thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let stdout = thread::spawn(move || {
        let mut b = Vec::new();
        out.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut b)
            .map(|_| b)
    });
    let stderr = thread::spawn(move || {
        let mut b = Vec::new();
        err.take(64 * 1024).read_to_end(&mut b).map(|_| b)
    });
    let start = Instant::now();
    let result = loop {
        if rt.cancelled() || start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            let _ = child.wait();
            break Err(if rt.cancelled() {
                Error::paused()
            } else {
                Error::operation(format!("{executable} timed out after {timeout} seconds"))
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(30)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.into());
            }
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let out = stdout
        .join()
        .map_err(|_| Error::operation("Tool output reader failed"))??;
    let err = stderr
        .join()
        .map_err(|_| Error::operation("Tool diagnostic reader failed"))??;
    let status = result?;
    if !status.success() {
        let _ = err;
        return Err(Error::operation(format!(
            "{executable} exited with {status}; verify the tool, source, and selected language. Diagnostic output is withheld because it may contain private input."
        )));
    }
    if out.len() > 16 * 1024 * 1024 {
        return Err(Error::operation("Tool output exceeded 16 MiB"));
    }
    String::from_utf8(out).map_err(|_| Error::operation("Tool returned invalid UTF-8"))
}
pub fn arguments(request: &Request, settings: &Value, path: &Path) -> Vec<String> {
    let mut args = [
        "blitz",
        "--language",
        &request.language,
        "--file",
        &path.to_string_lossy(),
        "--freq",
        "--lemmatize",
        "--context",
        "--bold",
        "html",
        "--format",
        "json",
        "--no-prompt",
        "--no-src",
        "--no-update-known",
        "--no-save-context",
    ]
    .map(str::to_string)
    .to_vec();
    let s = &settings["blitzer"];
    if s["no_config"].as_bool().unwrap_or(false) {
        args.push("--no-config".into());
    }
    for (key, flag) in [
        ("config", "--config"),
        ("plugins_dir", "--plugins-dir"),
        ("known_file", "--known-file"),
        ("skip_exact_words_file", "--skip-exact-words-file"),
        ("skip_word_families_file", "--skip-word-families-file"),
    ] {
        if let Some(v) = s[key].as_str().filter(|s| !s.is_empty()) {
            args.extend([flag.into(), v.into()]);
        }
    }
    args
}
pub fn extract(rt: &Runtime, request: &Request, settings: &Value) -> Result<Value> {
    let raw = run(
        rt,
        settings["blitzer"]["executable"]
            .as_str()
            .unwrap_or("bltzr"),
        &arguments(request, settings, &rt.path("artifacts/canonical.txt")?),
        settings["blitzer"]["timeout"].as_u64().unwrap_or(120),
    )?;
    serde_json::from_str(&raw).map_err(|_| {
        Error::operation("Blitzer did not return valid JSON; check its compiled CLI compatibility")
    })
}
pub fn dependencies(kind: &str, job: &Job) -> Result<Value> {
    let s = &job.settings;
    let mut files = BTreeMap::new();
    if kind == "forms" {
        if let Some(path) = s["llm"]["form_lexicon"].as_str().filter(|s| !s.is_empty()) {
            files.insert(path.into(), file_hash(Path::new(path))?);
        }
        return Ok(json!(files));
    }
    if kind == "blitzer" && job.request.vocabulary_json.is_some() {
        return Ok(json!({"imported":true}));
    }
    let keys = match kind {
        "blitzer" => vec![("blitzer", "executable")],
        "media" => vec![("media", "ffmpeg"), ("media", "ffprobe")],
        "asr" if s["asr"]["backend"] == "local" => vec![("asr", "executable")],
        _ => vec![],
    };
    for (section, key) in keys {
        let tool = s[section][key].as_str().unwrap_or("");
        let path = resolve(tool)?;
        files.insert(path.to_string_lossy().to_string(), file_hash(&path)?);
    }
    if kind == "blitzer" {
        let b = &s["blitzer"];
        let explicit = b["config"]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("BLITZER_CONFIG").map(PathBuf::from));
        let default = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| directories::BaseDirs::new().map(|p| p.home_dir().join(".config")))
            .map(|p| p.join("bltzr/bltzr.toml"));
        let path = if b["no_config"] == true {
            None
        } else {
            explicit.or(default.filter(|p| p.is_file()))
        };
        let raw: Value = if let Some(p) = &path {
            files.insert(p.to_string_lossy().into(), file_hash(p)?);
            serde_json::to_value(
                toml::from_str::<toml::Value>(&fs::read_to_string(p)?)
                    .map_err(|e| Error::input(e.to_string()))?,
            )?
        } else {
            json!({})
        };
        let base = path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(Path::new("."));
        let mut options = raw["defaults"].as_object().cloned().unwrap_or_default();
        if let Some(s) = raw["languages"][&job.request.language].as_object() {
            options.extend(s.clone());
        }
        let modern = ["skip_exact_words_file", "skip_word_families_file"]
            .iter()
            .any(|k| b[*k].as_str().is_some_and(|v| !v.is_empty()) || options.contains_key(*k));
        let mut paths = Vec::new();
        for key in if modern {
            vec!["skip_exact_words_file", "skip_word_families_file"]
        } else {
            vec!["known_file"]
        } {
            if let Some(p) = b[key].as_str().filter(|p| !p.is_empty()).or_else(|| {
                options
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
            }) {
                paths.push(PathBuf::from(config::resolve_path(p, base)?));
            }
        }
        if !modern {
            for key in ["exclusions", "forms_only"] {
                for p in options
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    paths.push(PathBuf::from(config::resolve_path(
                        p.as_str()
                            .ok_or_else(|| Error::input("Invalid Blitzer filtering path"))?,
                        base,
                    )?));
                }
            }
        }
        let root = b["plugins_dir"]
            .as_str()
            .filter(|p| !p.is_empty())
            .or_else(|| raw["locations"]["language_packs_dir"].as_str())
            .or_else(|| raw["locations"]["plugins_dir"].as_str())
            .map(|p| config::resolve_path(p, base).map(PathBuf::from))
            .transpose()?
            .unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .or_else(|| {
                        directories::BaseDirs::new().map(|b| b.home_dir().join(".local/share"))
                    })
                    .unwrap_or_default()
                    .join("bltzr/languages")
            });
        let pack = root.join(&job.request.language);
        if pack.is_dir() {
            for entry in walkdir::WalkDir::new(&pack).follow_links(true) {
                let entry = entry.map_err(|e| Error::operation(e.to_string()))?;
                if entry.file_type().is_file() {
                    paths.push(entry.into_path());
                }
            }
        }
        if options.get("sort") == Some(&json!("custom"))
            && let Some(p) = options.get("custom_order").and_then(Value::as_str)
        {
            paths.push(PathBuf::from(config::resolve_path(p, base)?));
        }
        for path in paths {
            files.insert(
                path.to_string_lossy().into(),
                if path.is_file() {
                    file_hash(&path)?
                } else {
                    "missing".into()
                },
            );
        }
    }
    if kind == "asr" && s["asr"]["backend"] == "local" {
        let path = s["asr"]["model_file"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                config::data_directory().join(format!(
                    "resources/models/ggml-{}.bin",
                    s["asr"]["model"].as_str().unwrap_or("small")
                ))
            });
        if path.is_file() {
            files.insert(path.to_string_lossy().to_string(), file_hash(&path)?);
        }
    }
    Ok(json!(files))
}
