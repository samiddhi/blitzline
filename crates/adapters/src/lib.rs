//! Desktop library storage and profile import. Native jobs use per-job pipeline stores.
use blitzline_domain::*;
use blitzline_engine::Repository;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
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
        lock.try_lock_exclusive().map_err(|_| Problem("This Blitzline library is open in another process. Close it first, or choose a different --data-dir.".into()))?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }
    pub fn default_directory() -> PathBuf {
        // Retain the original library location so upgrading does not hide saved work.
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

/// Inspect a source without loading recordings into memory or applying a media size limit.
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
    let metadata = fs::metadata(path).map_err(io_problem)?;
    if !metadata.is_file() {
        return Err(Problem("Choose a file rather than a folder.".into()));
    }
    if metadata.len() == 0 {
        return Err(Problem("This file is empty.".into()));
    }
    let mut file = File::open(path).map_err(io_problem)?;
    let mut header = [0u8; 16];
    let read = file.read(&mut header).map_err(io_problem)?;
    let header = &header[..read];
    let media = [
        "mp4", "mkv", "mov", "webm", "mp3", "m4a", "wav", "flac", "ogg", "aac", "opus", "aif",
        "aiff", "aifc", "m4b", "m4v", "avi", "mpeg", "mpg", "mpga", "mp2", "wma", "wmv", "3gp",
        "3g2", "oga", "ogv", "ts", "mts", "m2ts", "caf", "amr", "ac3", "mka", "weba", "au", "snd",
        "vob", "flv",
    ]
    .contains(&extension.as_str())
        || media_header(header);
    let text = if media {
        String::new()
    } else {
        if metadata.len() > pipeline::TEXT_INPUT_LIMIT {
            return Err(Problem("Text files can be up to 128 MiB. Split this document into smaller files. Recordings have no file-size limit.".into()));
        }
        // Validate text now, but keep file-backed content out of the library JSON.
        // The pipeline reads the original file when processing starts.
        let mut raw = Vec::new();
        File::open(path)
            .map_err(io_problem)?
            .take(pipeline::TEXT_INPUT_LIMIT + 1)
            .read_to_end(&mut raw)
            .map_err(io_problem)?;
        if raw.len() as u64 > pipeline::TEXT_INPUT_LIMIT {
            return Err(Problem(
                "The document exceeds the 128 MiB text limit. Recordings have no file-size limit."
                    .into(),
            ));
        }
        let text = std::str::from_utf8(&raw).map_err(|_|Problem("This file is neither a recognized recording nor UTF-8 text. Choose a supported recording or paste the document's text.".into()))?;
        if text.contains('\0') {
            return Err(Problem(
                "This is a binary file, not plain text or a recognized recording.".into(),
            ));
        }
        if text.trim_start_matches('\u{feff}').trim().is_empty() {
            return Err(Problem("This document contains no text.".into()));
        }
        String::new()
    };
    Ok(Source {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: Some(
            path.canonicalize()
                .map_err(io_problem)?
                .to_string_lossy()
                .into(),
        ),
        media,
        text,
        ..Default::default()
    })
}
fn media_header(header: &[u8]) -> bool {
    header.starts_with(b"fLaC")
        || header.starts_with(b"OggS")
        || header.starts_with(b"ID3")
        || header.starts_with(b"caff")
        || header.starts_with(b"#!AMR")
        || header.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || header.starts_with(&[0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11])
        || (header.starts_with(b"RIFF")
            && [b"WAVE".as_slice(), b"AVI "].contains(&header.get(8..12).unwrap_or_default()))
        || (header.starts_with(b"FORM")
            && [b"AIFF".as_slice(), b"AIFC"].contains(&header.get(8..12).unwrap_or_default()))
        || header.get(4..8) == Some(b"ftyp")
        || (header.len() >= 2 && header[0] == 0xff && header[1] & 0xe0 == 0xe0)
        || header.starts_with(&[0, 0, 1, 0xba])
        || header.starts_with(&[0, 0, 1, 0xb3])
}

/// Import safe profile values only. Preserve all unsupported options in a report.
pub fn import_profiles(path: &Path) -> Result<(Vec<Profile>, Vec<String>)> {
    let raw = fs::read_to_string(path).map_err(io_problem)?;
    let value: toml::Value = toml::from_str(&raw)
        .map_err(|_| Problem("This configuration is not valid TOML.".into()))?;
    let mut profiles = vec![];
    let mut report=vec!["Profile importer: only language/deck profiles are applied. Provider, path, and pipeline settings remain unchanged.".into()];
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

pub mod pipeline;
