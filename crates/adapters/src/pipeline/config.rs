use blitzline_domain::pipeline::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
/// Profile provider that runs the locally installed `claude -p` instead of calling an HTTP service.
pub const CLAUDE_CLI: &str = "claude-cli";
/// Models offered for the Claude CLI (aliases the CLI itself resolves to the latest version).
pub const CLAUDE_CLI_MODELS: [&str; 3] = ["sonnet", "opus", "haiku"];
/// Effort levels of `claude --effort`; an empty setting leaves the CLI default.
pub const MAX_PARALLEL: u64 = 16;
pub const CLAUDE_CLI_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
pub fn data_directory() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return p.join("blitzline");
    }
    directories::ProjectDirs::from("org", "Blitzline", "Blitzline")
        .map(|p| p.data_dir().to_owned())
        .unwrap_or_else(|| PathBuf::from(".blitzline"))
}
pub fn defaults() -> Value {
    json!({
        "run":{"output_dir":data_directory().join("runs"),"proofread":false,"translate":false,"allow_partial":false,"method":"auto","proofread_method":"auto","translate_method":"auto","review_method":"auto"},
        "asr":{"backend":"local","executable":"whisper-cli","model":"small","model_file":"","device":"cpu","compute_type":"int8","language":"","timeout":7200,"max_cue_ms":8000,"line_length":42,"pause_ms":800,"max_chars_per_second":25,"relisten":false,"endpoint":"","worker_command":[],"api_key_env":"","remote_compute_type":"float16","chunk_seconds":600,"retries":2,"options":{}},
        "blitzer":{"executable":"bltzr","config":"","plugins_dir":"","known_file":"","skip_exact_words_file":"","skip_word_families_file":"","anki_collection":"","anki_deck":"","anki_fields":[],"anki_family_field":"","no_config":false,"timeout":120},
        "llm":{"default":"","proofread":"","translate":"","review":"","ambiguity_review":"","audit_review":"","form_lexicon":"","batch_size":20,"ambiguity_batch_size":5,"max_chars":24000,"repair_attempts":2},
        "profiles":{},"decks":{},
        "media":{"kind":"audio","padding_ms":200,"max_clip_ms":45000,"ffmpeg":"ffmpeg","ffprobe":"ffprobe","timeout":300,"audio_codec":"libmp3lame","video_codec":"libx264"},
        "export":{"format":"tsv","deck":"Blitzline","model":"Blitzer Basic","anki_url":"http://127.0.0.1:8765","existing":"skip","preset":"blitzer","deck_profile":"","card_types":[],"fields":[],"templates":[],"front_template":null,"back_template":null,"css":null,"css_file":"","assets":[],"tags":[],"max_sentence_words":25,"order_by":["chronological"],"two_way":false,"update_model":false}
    })
}
pub fn selected_config(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(p) = explicit
        .map(Path::to_owned)
        .or_else(|| std::env::var_os("BLITZLINE_CONFIG").map(PathBuf::from))
    {
        if !p.is_file() {
            return Err(Error::input(format!(
                "Configuration does not exist: {}",
                p.display()
            )));
        }
        return Ok(Some(p.canonicalize()?));
    }
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("blitzline"))
        .unwrap_or_else(|| {
            directories::ProjectDirs::from("org", "Blitzline", "Blitzline")
                .map(|p| p.config_dir().to_owned())
                .unwrap_or_else(|| PathBuf::from(".blitzline"))
        });
    let path = root.join("config.toml");
    Ok(path.is_file().then_some(path))
}
pub fn load(explicit: Option<&Path>) -> Result<Value> {
    let selected = selected_config(explicit)?;
    let base = selected
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."));
    let raw = if let Some(path) = &selected {
        serde_json::to_value(
            toml::from_str::<toml::Value>(&std::fs::read_to_string(path)?)
                .map_err(|e| Error::input(format!("Invalid configuration: {e}")))?,
        )?
    } else {
        json!({})
    };
    merge(raw, base)
}
pub fn merge(raw: Value, base: &Path) -> Result<Value> {
    let mut result = defaults();
    let root = raw
        .as_object()
        .ok_or_else(|| Error::input("Configuration must be a table"))?;
    for (section, fields) in root {
        let target = result
            .get_mut(section)
            .ok_or_else(|| Error::input(format!("Unknown configuration table {section}")))?;
        let fields = fields
            .as_object()
            .ok_or_else(|| Error::input(format!("{section} must be a table")))?;
        if section == "profiles" || section == "decks" {
            *target = json!(fields);
            continue;
        }
        for (k, v) in fields {
            if target.get(k).is_none() {
                return Err(Error::input(format!(
                    "Unsupported {section}.{k}; check the Rust migration notes"
                )));
            }
            target[k] = v.clone();
        }
    }
    if raw["export"]["model"] == "Blitzline v1" && raw["export"].get("preset").is_none() {
        result["export"]["preset"] = json!("legacy");
    }
    for (section, key) in [
        ("run", "output_dir"),
        ("blitzer", "config"),
        ("blitzer", "plugins_dir"),
        ("blitzer", "known_file"),
        ("blitzer", "skip_exact_words_file"),
        ("blitzer", "skip_word_families_file"),
        ("blitzer", "anki_collection"),
        ("llm", "form_lexicon"),
        ("asr", "model_file"),
    ] {
        if let Some(v) = result[section][key].as_str().filter(|v| !v.is_empty()) {
            result[section][key] = json!(resolve_path(v, base)?);
        }
    }
    for (section, key) in [
        ("asr", "executable"),
        ("blitzer", "executable"),
        ("media", "ffmpeg"),
        ("media", "ffprobe"),
    ] {
        if let Some(v) = result[section][key]
            .as_str()
            .filter(|v| v.contains(['/', '\\']))
        {
            result[section][key] = json!(resolve_path(v, base)?);
        }
    }
    resolve_export_paths(&mut result["export"], base)?;
    for (_, deck) in result["decks"]
        .as_object_mut()
        .ok_or_else(|| Error::input("Decks must be a table"))?
    {
        resolve_export_paths(deck, base)?;
    }
    for (_, profile) in result["profiles"]
        .as_object_mut()
        .ok_or_else(|| Error::input("Profiles must be a table"))?
    {
        *profile = profile_defaults(profile)?;
    }
    validate(&result)?;
    Ok(result)
}
pub fn resolve_path(s: &str, base: &Path) -> Result<String> {
    let mut s = s.to_string();
    if s.starts_with("~/")
        && let Some(home) = directories::BaseDirs::new()
    {
        s = home.home_dir().join(&s[2..]).to_string_lossy().into();
    }
    let p = PathBuf::from(s);
    let path = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()?.join(base).join(p)
    };
    Ok(path.to_string_lossy().into())
}
fn resolve_export_paths(s: &mut Value, base: &Path) -> Result<()> {
    if let Some(path) = s["css_file"].as_str().filter(|p| !p.is_empty()) {
        s["css_file"] = json!(resolve_path(path, base)?);
    }
    if let Some(paths) = s["assets"].as_array_mut() {
        for p in paths {
            *p = json!(resolve_path(
                p.as_str()
                    .ok_or_else(|| Error::input("Asset path must be text"))?,
                base
            )?);
        }
    }
    if let Some(rows) = s["templates"].as_array_mut() {
        for row in rows {
            for side in ["front_file", "back_file"] {
                if let Some(p) = row[side].as_str().filter(|p| !p.is_empty()) {
                    row[side] = json!(resolve_path(p, base)?);
                }
            }
        }
    }
    Ok(())
}
fn profile_defaults(p: &Value) -> Result<Value> {
    let provider = p["provider"].as_str().unwrap_or("openrouter");
    let (base, key) = match provider {
        "openrouter" => ("https://openrouter.ai/api/v1", "OPENROUTER_API_KEY"),
        "deepseek" => ("https://api.deepseek.com", "DEEPSEEK_API_KEY"),
        "compatible" | "custom" => ("", "LLM_API_KEY"),
        // No endpoint or key: the locally installed Claude CLI uses its own subscription login.
        CLAUDE_CLI => ("", ""),
        _ => {
            return Err(Error::input(
                "LLM provider must be openrouter, deepseek, compatible, or claude-cli",
            ));
        }
    };
    let mut out = json!({"provider":provider,"base_url":base,"api_key_env":key,"model":"","temperature":0.2,"max_tokens":8192,"timeout":120,"retries":2,"parallel":1,"response_format":"json_object","referer":"","openrouter_provider":"","openrouter_max_price":{}});
    if provider == CLAUDE_CLI {
        out["command"] = json!(["claude"]);
        out["effort"] = json!("");
    }
    for (k, v) in p
        .as_object()
        .ok_or_else(|| Error::input("LLM profile must be a table"))?
    {
        if out.get(k).is_none() && k != "reasoning_enabled" {
            return Err(Error::input(format!(
                "Unknown LLM profile setting {k}; credentials belong in environment variables or OS credential storage"
            )));
        }
        out[k] = v.clone();
    }
    Ok(out)
}
pub fn validate(s: &Value) -> Result<()> {
    let defaults = defaults();
    for (section, fields) in defaults.as_object().unwrap() {
        let actual = s[section]
            .as_object()
            .ok_or_else(|| Error::input(format!("{section} must be a table")))?;
        if section == "profiles" || section == "decks" {
            continue;
        }
        for (key, expected) in fields.as_object().unwrap() {
            let Some(value) = actual.get(key) else {
                // Runs saved before the Anki settings existed lack them; absent means off.
                if section == "blitzer" && key.starts_with("anki_") {
                    continue;
                }
                return Err(Error::input(format!("Missing {section}.{key}")));
            };
            let matches = if expected.is_string() {
                value.is_string()
            } else if expected.is_boolean() {
                value.is_boolean()
            } else if expected.is_number() {
                value.is_number()
            } else if expected.is_array() {
                value.is_array()
            } else if expected.is_object() {
                value.is_object()
            } else {
                value.is_null() || value.is_string()
            };
            if !matches {
                return Err(Error::input(format!("Invalid type for {section}.{key}")));
            }
        }
        if actual.keys().any(|key| fields.get(key).is_none()) {
            return Err(Error::input(format!("Unknown setting in {section}")));
        }
    }
    for (section, keys) in [
        ("run", vec!["proofread", "translate", "allow_partial"]),
        ("asr", vec!["relisten"]),
        ("blitzer", vec!["no_config"]),
        ("export", vec!["two_way", "update_model"]),
    ] {
        for k in keys {
            if !s[section][k].is_boolean() {
                return Err(Error::input(format!("{section}.{k} must be true or false")));
            }
        }
    }
    if is_local_worker(&s["asr"]) {
        local_worker_address(&s["asr"])?;
    }
    if !s["asr"]["worker_command"]
        .as_array()
        .is_some_and(|c| c.iter().all(|a| a.as_str().is_some_and(|a| !a.is_empty())))
    {
        return Err(Error::input(
            "asr.worker_command must be a list of non-empty strings",
        ));
    }
    for (section, key, choices) in [
        (
            "asr",
            "backend",
            vec!["local", "local-worker", "http", "groq"],
        ),
        ("media", "kind", vec!["audio", "video"]),
        ("export", "format", vec!["tsv", "apkg", "anki"]),
        ("export", "existing", vec!["skip", "update"]),
    ] {
        if !choices.contains(&s[section][key].as_str().unwrap_or("")) {
            return Err(Error::input(format!(
                "Invalid {section}.{key}; choose {}",
                choices.join(", ")
            )));
        }
    }
    for k in [
        "method",
        "proofread_method",
        "translate_method",
        "review_method",
    ] {
        if !["auto", "browser", "manual", "api"].contains(&s["run"][k].as_str().unwrap_or("")) {
            return Err(Error::input(format!("Invalid run.{k}")));
        }
    }
    for (section, key, min, max) in [
        ("llm", "batch_size", 1, 1000),
        ("llm", "ambiguity_batch_size", 1, 1000),
        ("llm", "max_chars", 100, 1_000_000),
        ("llm", "repair_attempts", 0, 10),
        ("asr", "chunk_seconds", 1, 600),
        ("asr", "timeout", 1, 86400),
        ("asr", "retries", 0, 10),
        ("asr", "max_cue_ms", 1, 3_600_000),
        ("asr", "line_length", 1, 1000),
        ("asr", "pause_ms", 0, 60_000),
        ("asr", "max_chars_per_second", 1, 1000),
        ("blitzer", "timeout", 1, 86400),
        ("media", "timeout", 1, 86400),
        ("media", "max_clip_ms", 1, 3_600_000),
        ("media", "padding_ms", 0, 60_000),
        ("export", "max_sentence_words", 1, 1000),
    ] {
        if !s[section][key]
            .as_u64()
            .is_some_and(|v| v >= min && v <= max)
        {
            return Err(Error::input(format!(
                "Invalid {section}.{key}: expected integer {min}..{max}"
            )));
        }
    }
    for (name, p) in s["profiles"]
        .as_object()
        .ok_or_else(|| Error::input("Invalid profiles"))?
    {
        nonblank(
            p["model"].as_str().unwrap_or(""),
            &format!("Profile {name} model"),
        )?;
        let cli = p["provider"] == CLAUDE_CLI;
        if cli {
            if !CLAUDE_CLI_MODELS.contains(&p["model"].as_str().unwrap_or("")) {
                return Err(Error::input(format!(
                    "Profile {name} model must be one of {}",
                    CLAUDE_CLI_MODELS.join(", ")
                )));
            }
            let effort = p["effort"].as_str().unwrap_or("");
            if !effort.is_empty() && !CLAUDE_CLI_EFFORTS.contains(&effort) {
                return Err(Error::input(format!(
                    "Profile {name} effort must be one of {}",
                    CLAUDE_CLI_EFFORTS.join(", ")
                )));
            }
            if !p["command"].as_array().is_some_and(|c| {
                !c.is_empty() && c.iter().all(|a| a.as_str().is_some_and(|a| !a.is_empty()))
            }) {
                return Err(Error::input(format!(
                    "Profile {name} command must be a list of non-empty strings"
                )));
            }
        } else {
            validate_url(p["base_url"].as_str().unwrap_or(""))?;
        }
        for (key, min, max) in [
            ("timeout", 1, 86400),
            ("retries", 0, 10),
            ("parallel", 1, MAX_PARALLEL),
            ("max_tokens", 1, 1_000_000),
        ] {
            if !p[key].as_u64().is_some_and(|v| v >= min && v <= max) {
                return Err(Error::input(format!("Invalid profile {name}.{key}")));
            }
        }
        if !p["temperature"]
            .as_f64()
            .is_some_and(|v| (0.0..=2.0).contains(&v))
        {
            return Err(Error::input(format!("Invalid profile {name}.temperature")));
        }
        if p.get("reasoning_enabled").is_some_and(|v| !v.is_boolean()) {
            return Err(Error::input(format!(
                "Invalid profile {name}.reasoning_enabled"
            )));
        }
        if !["json_object", "json_schema", "text"]
            .contains(&p["response_format"].as_str().unwrap_or(""))
        {
            return Err(Error::input("Invalid profile response_format"));
        }
        let key = p["api_key_env"].as_str().unwrap_or("");
        if !cli && (key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            return Err(Error::input(
                "api_key_env must name an environment variable",
            ));
        }
    }
    for stage in [
        "default",
        "proofread",
        "translate",
        "review",
        "ambiguity_review",
        "audit_review",
    ] {
        if let Some(name) = s["llm"][stage].as_str().filter(|v| !v.is_empty())
            && s["profiles"].get(name).is_none()
        {
            return Err(Error::input(format!("Unknown LLM profile {name}")));
        }
    }
    Ok(())
}
/// True when Blitzline launches the ASR service itself: the `http` backend with `asr.worker_command` set.
/// `local-worker` is the deprecated spelling of `http`; it is always treated as a launchable worker.
pub fn is_local_worker(asr: &Value) -> bool {
    asr["backend"] == "local-worker"
        || (asr["backend"] == "http"
            && asr["worker_command"]
                .as_array()
                .is_some_and(|c| !c.is_empty()))
}
/// The `host:port` of a loopback worker endpoint, or an error if it is missing or not loopback.
pub fn local_worker_address(asr: &Value) -> Result<String> {
    let url = asr["endpoint"]
        .as_str()
        .and_then(|u| reqwest::Url::parse(u).ok())
        .filter(|u| u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
        .ok_or_else(|| Error::input("Set asr.endpoint to an http URL on this computer (localhost, 127.0.0.1 or [::1]) so Blitzline can launch the ASR worker"))?;
    Ok(format!(
        "{}:{}",
        url.host_str().unwrap_or("127.0.0.1"),
        url.port_or_known_default().unwrap_or(80)
    ))
}
pub fn validate_url(url: &str) -> Result<()> {
    let u = reqwest::Url::parse(url).map_err(|_| Error::input("Invalid HTTP service URL"))?;
    if !["http", "https"].contains(&u.scheme())
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err(Error::input(
            "Use an HTTP(S) URL without embedded credentials or fragments",
        ));
    }
    Ok(())
}
pub fn export_settings(settings: &Value, language: &str) -> Result<Value> {
    let mut export = settings["export"].clone();
    let selected = export["deck_profile"].as_str().unwrap_or("");
    let decks = settings["decks"]
        .as_object()
        .ok_or_else(|| Error::input("Invalid deck profiles"))?;
    let name = if !selected.is_empty() {
        Some(selected.to_owned())
    } else if decks.is_empty() {
        None
    } else {
        let choices = decks
            .iter()
            .filter(|(_, v)| v["language"] == language)
            .map(|(n, _)| n.clone())
            .collect::<Vec<_>>();
        if choices.len() != 1 {
            return Err(Error::input(
                "Configure a matching language/deck profile, selecting --deck-profile when there is more than one",
            ));
        }
        Some(choices[0].clone())
    };
    if let Some(name) = name {
        let deck = decks
            .get(&name)
            .ok_or_else(|| Error::input(format!("Unknown deck profile {name}")))?;
        if deck["language"] != language {
            return Err(Error::input(
                "Deck profile language does not match the source",
            ));
        }
        if deck.get("model").is_none() {
            export["model"] = json!(format!(
                "{} [{name}]",
                export["model"].as_str().unwrap_or("Blitzer Basic")
            ));
        }
        for (k, v) in deck
            .as_object()
            .ok_or_else(|| Error::input("Deck profile must be a table"))?
        {
            if k != "language" {
                if export.get(k).is_none() {
                    return Err(Error::input(format!("Unknown deck setting {k}")));
                }
                export[k] = v.clone();
            }
        }
        export["deck_profile"] = json!(name);
        if deck.get("two_way").is_none()
            && deck["card_types"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("Recall")))
        {
            export["two_way"] = json!(true);
        }
    }
    export["language"] = json!(language);
    let mut unique = BTreeSet::new();
    for t in export["order_by"]
        .as_array()
        .ok_or_else(|| Error::input("order_by must be an array"))?
    {
        if !unique.insert(
            t.as_str()
                .ok_or_else(|| Error::input("Invalid sort rule"))?,
        ) {
            return Err(Error::input("Duplicate sort rule"));
        }
    }
    if let Some(p) = export["css_file"].as_str().filter(|p| !p.is_empty()) {
        if !export["css"].is_null() {
            return Err(Error::input("Choose css or css_file"));
        }
        export["css"] = json!(std::fs::read_to_string(p)?);
    }
    if let Some(rows) = export["templates"].as_array_mut() {
        for row in rows {
            for side in ["front", "back"] {
                if let Some(p) = row[format!("{side}_file")]
                    .as_str()
                    .filter(|p| !p.is_empty())
                {
                    if row.get(side).is_some() {
                        return Err(Error::input("Choose inline template or template file"));
                    }
                    row[side] = json!(std::fs::read_to_string(p)?);
                }
            }
        }
    }
    notes::definition(&export)?;
    Ok(export)
}
#[cfg(test)]
mod worker_tests {
    use super::*;
    #[test]
    fn http_with_a_command_is_a_launched_worker_and_must_be_loopback() {
        let mut s = defaults();
        s["asr"]["backend"] = json!("http");
        s["asr"]["endpoint"] = json!("https://asr.example/transcribe");
        assert!(!is_local_worker(&s["asr"]));
        validate(&s).unwrap();
        s["asr"]["worker_command"] = json!(["python3", "w.py"]);
        assert!(is_local_worker(&s["asr"]));
        assert!(validate(&s).is_err());
        s["asr"]["endpoint"] = json!("http://127.0.0.1:8765/transcribe");
        validate(&s).unwrap();
    }
    #[test]
    fn the_old_local_worker_name_still_validates() {
        let mut s = defaults();
        s["asr"]["backend"] = json!("local-worker");
        s["asr"]["endpoint"] = json!("http://localhost:8765/transcribe");
        assert!(is_local_worker(&s["asr"]));
        validate(&s).unwrap();
    }
}
#[cfg(test)]
mod parallel_tests {
    use super::*;
    fn with_parallel(provider: &str, n: Value) -> Result<()> {
        let model = if provider == CLAUDE_CLI {
            CLAUDE_CLI_MODELS[0]
        } else {
            "m"
        };
        let mut s = defaults();
        s["profiles"]["p"] =
            profile_defaults(&json!({"provider":provider,"model":model,"parallel":n}))?;
        validate(&s)
    }
    #[test]
    fn parallel_is_an_integer_from_1_to_16() {
        for provider in ["openrouter", "claude-cli"] {
            for ok in [1, 4, 16] {
                with_parallel(provider, json!(ok)).unwrap();
            }
            for bad in [json!(0), json!(17), json!("2"), json!(1.5)] {
                assert!(with_parallel(provider, bad).is_err());
            }
        }
    }
    #[test]
    fn parallel_defaults_to_one() {
        assert_eq!(
            profile_defaults(&json!({"provider":"claude-cli"})).unwrap()["parallel"],
            1
        );
    }
}
