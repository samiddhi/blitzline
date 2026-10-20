//! Prototype storage and import adapters. Production jobs will use per-job stores.
use blitzline_domain::*;
use blitzline_engine::Repository;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub struct Store {
    pub directory: PathBuf,
    _lock: File,
}
impl Store {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory).map_err(io_problem)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("workspace.lock"))
            .map_err(io_problem)?;
        lock.try_lock_exclusive().map_err(|_| Problem("This prototype workspace is open in another process. Close it first, or choose a different --data-dir.".into()))?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
    pub fn default_directory() -> PathBuf {
        directories::ProjectDirs::from("org", "Blitzline", "Blitzline Prototype")
            .map(|p| p.data_local_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".prototype"))
    }
    pub fn save_ui<T: serde::Serialize>(&self, value: &T) -> Result<()> {
        self.write_json("ui.json", value)
    }
    pub fn load_ui<T: serde::de::DeserializeOwned + Default>(&self) -> T {
        fs::read(self.directory.join("ui.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    fn write_json<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| Problem(e.to_string()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory).map_err(io_problem)?;
        temporary.write_all(&bytes).map_err(io_problem)?;
        temporary.as_file().sync_all().map_err(io_problem)?;
        temporary
            .persist(self.directory.join(name))
            .map_err(|e| io_problem(e.error))?;
        Ok(())
    }
}
impl Repository for Store {
    fn load(&self) -> Result<Workspace> {
        let path = self.directory.join("workspace.json");
        if !path.exists() {
            return Ok(Workspace::default());
        }
        let bytes = fs::read(path).map_err(io_problem)?;
        let state:Workspace=serde_json::from_slice(&bytes).map_err(|_|Problem("Saved projects could not be read. The file has been kept intact; restore a backup or choose another data directory.".into()))?;
        if state.schema != 1 {
            return Err(Problem(
                "This workspace needs a different application version.".into(),
            ));
        }
        Ok(state)
    }
    fn save(&self, state: &Workspace) -> Result<()> {
        self.write_json("workspace.json", state)
    }
}
fn io_problem(e: std::io::Error) -> Problem {
    Problem(format!("Could not access the file: {e}"))
}

pub fn load_source(path: &Path) -> Result<Source> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ["pdf", "doc", "docx", "rtf", "odt"].contains(&extension.as_str()) {
        return Err(Problem(
            "This document format is not supported yet. Copy its text and use Paste text.".into(),
        ));
    }
    let media = [
        "mp4", "mkv", "mov", "webm", "mp3", "m4a", "wav", "flac", "ogg", "aac",
    ]
    .contains(&extension.as_str());
    let text = if media {
        String::new()
    } else {
        let metadata = fs::metadata(path).map_err(io_problem)?;
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(Problem(
                "This prototype accepts text files up to 2 MB.".into(),
            ));
        }
        fs::read_to_string(path).map_err(|_| {
            Problem("This file is not UTF-8 text. Choose a text file or paste its contents.".into())
        })?
    };
    Ok(Source {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: Some(path.to_string_lossy().into()),
        media,
        text,
        ..Default::default()
    })
}

/// Import safe profile values only. Preserve all unsupported options in a report.
pub fn import_profiles(path: &Path) -> Result<(Vec<Profile>, Vec<String>)> {
    let raw = fs::read_to_string(path).map_err(io_problem)?;
    let value: toml::Value = toml::from_str(&raw)
        .map_err(|_| Problem("This configuration is not valid TOML.".into()))?;
    let mut profiles = vec![];
    let mut report=vec!["Prototype importer: only language/deck profiles are applied. Provider, path, and pipeline settings remain unchanged.".into()];
    if let Some(decks) = value.get("decks").and_then(toml::Value::as_table) {
        for (name, deck) in decks {
            match (
                deck.get("language").and_then(toml::Value::as_str),
                deck.get("deck").and_then(toml::Value::as_str),
            ) {
                (Some(language), Some(destination)) => profiles.push(Profile {
                    name: name.clone(),
                    language: language.into(),
                    deck: destination.into(),
                    ..Default::default()
                }),
                _ => report.push(format!(
                    "Skipped {name}: language or destination is missing."
                )),
            }
        }
    }
    if value.get("asr").is_some() {
        report.push("Transcription options need review. WhisperX and direct Modal settings are not imported.".into());
    }
    if profiles.is_empty() {
        report.push("No usable deck profiles were found.".into());
    }
    Ok((profiles, report))
}
