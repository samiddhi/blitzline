//! Native production adapters shared by CLI and desktop workers.
mod anki;
pub mod config;
mod exports;
mod media;
mod process;
mod providers;
pub mod resources;
use blitzline_domain::pipeline::*;
use blitzline_engine::pipeline::{Backend, Pipeline};
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub const TEXT_INPUT_LIMIT: u64 = 128 * 1024 * 1024;

pub struct Runtime {
    pub root: PathBuf,
    _lock: File,
    pub cancel: Arc<AtomicBool>,
    pub verbose: bool,
    pub credentials: BTreeMap<String, String>,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn file_hash(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::input("Missing destination directory"))?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|e| Error::operation(format!("Could not save {}: {}", path.display(), e.error)))?;
    Ok(())
}
pub fn write_json<T: serde::Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?)
}
impl Runtime {
    pub fn open(root: &Path, cancel: Arc<AtomicBool>) -> Result<Self> {
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".lock"))?;
        lock.try_lock_exclusive().map_err(|_| {
            Error::operation(format!("Another process is using run {}", root.display()))
        })?;
        Ok(Self {
            root,
            _lock: lock,
            cancel,
            verbose: true,
            credentials: BTreeMap::new(),
        })
    }
    pub fn path(&self, relative: &str) -> Result<PathBuf> {
        let p = Path::new(relative);
        if p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(Error::input("Invalid path in run manifest"));
        }
        let mut out = self.root.clone();
        for c in p.components() {
            out.push(c);
            if fs::symlink_metadata(&out).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(Error::operation(format!(
                    "Refusing a symlinked run path: {}",
                    out.display()
                )));
            }
        }
        Ok(out)
    }
    pub fn load(&self) -> Result<Job> {
        let job: Job = serde_json::from_slice(&self.read("manifest.json")?)?;
        if job.implementation != "blitzline-rust" || job.schema_version != 1 {
            return Err(Error::input(
                "This is not a supported Rust run. Python runs remain with blitzline-py.",
            ));
        }
        if !["text", "media"].contains(&job.request.source_kind.as_str())
            || job.request.language.len() != 3
            || !job.request.language.chars().all(|c| c.is_ascii_lowercase())
        {
            return Err(Error::input("Invalid source type/language in manifest"));
        }
        config::validate(&job.settings)?;
        Ok(job)
    }
    pub fn cleanup(&self, job: &Job, apply: bool) -> Result<Value> {
        if !["complete", "cleaning"].contains(&job.state.as_str()) {
            return Err(Error::input("Finish a run before cleaning it"));
        }
        let mut keep = Vec::new();
        for (format, files) in &job.exports {
            if !["tsv", "apkg"].contains(&format.as_str()) {
                continue;
            }
            for (p, h) in files {
                let path = self.path(p)?;
                if !p.starts_with("exports/") || file_hash(&path)? != *h {
                    return Err(Error::operation(
                        "An export changed or is missing; cleanup was refused",
                    ));
                }
                keep.push(p.clone());
            }
        }
        if keep.is_empty() {
            return Err(Error::input(
                "Cleanup requires a verified portable TSV or APKG export",
            ));
        }
        let mut files = 0u64;
        let mut bytes = 0u64;
        let mut remove = Vec::new();
        for n in ["artifacts", "media", "logs", ".cache"] {
            let path = self.path(n)?;
            if path.exists() {
                if !path.is_dir() {
                    return Err(Error::operation("Cleanup target is not a directory"));
                }
                for item in walkdir::WalkDir::new(&path).follow_links(false) {
                    let item = item.map_err(|e| Error::operation(e.to_string()))?;
                    if item.file_type().is_symlink() {
                        return Err(Error::operation(
                            "Cleanup refuses symlinked intermediate files",
                        ));
                    }
                    if item.file_type().is_file() {
                        files += 1;
                        bytes += item
                            .metadata()
                            .map_err(|e| Error::operation(e.to_string()))?
                            .len();
                    }
                }
                remove.push(n);
            }
        }
        let plan = json!({"run":self.root,"remove":remove,"keep":keep,"files":files,"bytes":bytes});
        if apply {
            let mut cleaned = job.clone();
            cleaned.state = "cleaning".into();
            self.save(&cleaned)?;
            for n in &remove {
                fs::remove_dir_all(self.path(n)?)?;
            }
            cleaned.state = "cleaned".into();
            cleaned.pending = None;
            cleaned.history.clear();
            cleaned.decisions.clear();
            cleaned.stages.clear();
            self.save(&cleaned)?;
        }
        Ok(plan)
    }
}
pub fn start(
    root: &Path,
    source: &Path,
    language: &str,
    kind: &str,
    settings: Value,
    cancel: Arc<AtomicBool>,
) -> Result<Pipeline<Runtime>> {
    config::validate(&settings)?;
    config::export_settings(&settings, language)?;
    if language.len() != 3
        || !language.chars().all(|c| c.is_ascii_lowercase())
        || !["text", "media"].contains(&kind)
    {
        return Err(Error::input(
            "Use a lowercase three-letter Blitzer language code and text or media source",
        ));
    }
    let backend = Runtime::open(root, cancel)?;
    if backend.root.join("manifest.json").exists() {
        return Err(Error::input(
            "A run already exists here. Use resume or choose another --output directory.",
        ));
    }
    let source = source
        .canonicalize()
        .map_err(|e| Error::input(format!("Cannot open source: {e}")))?;
    let source_hash = file_hash(&source)?;
    let job = Job {
        implementation: "blitzline-rust".into(),
        schema_version: 1,
        created: now(),
        revision: 1,
        request: Request {
            source: source.to_string_lossy().into(),
            source_kind: kind.into(),
            source_hash,
            language: language.into(),
            srt: None,
            asr_json: None,
            text_file: None,
            vocabulary_json: None,
        },
        settings,
        state: "paused".into(),
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
    backend.save(&job)?;
    Ok(Pipeline { backend, job })
}
pub fn open(root: &Path, cancel: Arc<AtomicBool>) -> Result<Pipeline<Runtime>> {
    let backend = Runtime::open(root, cancel)?;
    let job = backend.load()?;
    Ok(Pipeline { backend, job })
}
impl Backend for Runtime {
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.path(path)?)?)
    }
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()> {
        atomic(&self.path(path)?, bytes)
    }
    fn fingerprint(&self, path: &str) -> Result<String> {
        file_hash(&self.path(path)?)
    }
    fn external(&self, path: &str) -> Result<Vec<u8>> {
        let path = Path::new(path);
        if fs::metadata(path)?.len() > TEXT_INPUT_LIMIT {
            return Err(Error::input("Input text/JSON exceeds the 128 MiB limit"));
        }
        Ok(fs::read(path)?)
    }
    fn external_hash(&self, path: &str) -> Result<String> {
        file_hash(Path::new(path))
    }
    fn save(&self, job: &Job) -> Result<()> {
        write_json(&self.path("manifest.json")?, job)
    }
    fn dependencies(&self, kind: &str, job: &Job) -> Result<Value> {
        process::dependencies(kind, job)
    }
    fn probe(&self, request: &Request, settings: &Value) -> Result<MediaInfo> {
        media::probe(self, Path::new(&request.source), settings)
    }
    fn transcribe(&self, request: &Request, settings: &Value, info: &MediaInfo) -> Result<Value> {
        media::transcribe(self, request, settings, info)
    }
    fn relisten(&self, request: &Request, settings: &Value, cue: &Cue) -> Result<String> {
        media::relisten(self, request, settings, cue)
    }
    fn extract(&self, request: &Request, settings: &Value) -> Result<Value> {
        process::extract(self, request, settings)
    }
    fn request(&self, profile: &Value, task: &Task) -> Result<Value> {
        providers::request(self, profile, task)
    }
    fn forms(
        &self,
        settings: &Value,
        candidates: &[Candidate],
    ) -> Result<BTreeMap<String, Vec<String>>> {
        let path = settings["llm"]["form_lexicon"].as_str().unwrap_or("");
        let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if path.is_empty() {
            return Ok(result);
        }
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| Error::input(format!("Cannot read form lexicon: {e}")))?;
        let mut stmt=db.prepare("SELECT forms.form_representation FROM lemmas JOIN forms ON forms.lemma_id = lemmas.id WHERE lemmas.lemma = ?1").map_err(|e|Error::input(e.to_string()))?;
        for c in candidates {
            let rows = stmt
                .query_map([&c.term], |r| r.get::<_, String>(0))
                .map_err(|e| Error::input(e.to_string()))?;
            for row in rows {
                result
                    .entry(c.term.clone())
                    .or_default()
                    .push(row.map_err(|e| Error::input(e.to_string()))?);
            }
        }
        Ok(result)
    }
    fn clips(
        &self,
        cards: &[Card],
        request: &Request,
        info: &MediaInfo,
        settings: &Value,
    ) -> Result<(Vec<Card>, Vec<Value>)> {
        media::clips(self, cards, request, info, settings)
    }
    fn export(&self, cards: &[Card], settings: &Value) -> Result<Vec<String>> {
        exports::export(self, cards, settings)
    }
    fn export_settings(&self, settings: &Value, language: &str) -> Result<Value> {
        config::export_settings(settings, language)
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.root.join(".pause").exists()
    }
    fn progress(&self, stage: &str, state: &str) {
        if self.verbose {
            eprintln!("{stage}: {state}");
        }
    }
}

pub use process::resolve as find_tool;

/// An explicit user action: open a local file/folder in its system application.
pub fn open_file(path: &Path) -> Result<()> {
    let path = path.canonicalize()?;
    let executable = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    std::process::Command::new(executable)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| {
            Error::operation(format!("Could not open the system application: {error}"))
        })?;
    Ok(())
}

/// Install a bundled executable atomically, retaining its executable permissions.
pub fn install_binary(source: &Path, destination: &Path) -> Result<()> {
    let source = find_tool(&source.to_string_lossy())?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::input("Choose a destination folder"))?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut File::open(&source)?, &mut staged)?;
    staged.flush()?;
    staged
        .as_file()
        .set_permissions(fs::metadata(&source)?.permissions())?;
    staged.as_file().sync_all()?;
    staged
        .persist(destination)
        .map_err(|error| Error::operation(error.error.to_string()))?;
    Ok(())
}
