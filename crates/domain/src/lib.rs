//! UI-independent desktop values, validation, and native pipeline contracts.
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Problem(pub String);
pub type Result<T> = std::result::Result<T, Problem>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    #[default]
    Browser,
    Api,
    Manual,
}
impl Method {
    pub const ALL: [Self; 3] = [Self::Browser, Self::Api, Self::Manual];
    pub fn label(self) -> &'static str {
        match self {
            Self::Browser => "Use a chat website",
            Self::Api => "Connect an AI service",
            Self::Manual => "Review it myself",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    #[default]
    Source,
    Transcript,
    Translation,
    Words,
    Cards,
    Export,
}
impl Stage {
    pub const ALL: [Self; 6] = [
        Self::Source,
        Self::Transcript,
        Self::Translation,
        Self::Words,
        Self::Cards,
        Self::Export,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Source => "Source",
            Self::Transcript => "Transcript / text",
            Self::Translation => "English translation",
            Self::Words => "Words",
            Self::Cards => "Study cards",
            Self::Export => "Export",
        }
    }
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|x| *x == self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Queued,
    Running,
    Waiting,
    Paused,
    Failed,
    Ready,
    Finished,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Processing",
            Self::Waiting => "Needs your attention",
            Self::Paused => "Paused",
            Self::Failed => "Needs a fix",
            Self::Ready => "Ready to preview",
            Self::Finished => "Finished",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub method: Method,
    pub proofread: bool,
    pub translate: bool,
    pub proof_method: Option<Method>,
    pub translation_method: Option<Method>,
    pub recall: bool,
    pub video: bool,
    pub automatic_export: bool,
    pub model: String,
    pub local_transcription: bool,
    pub batch_size: usize,
    pub profile_name: String,
}
impl Settings {
    pub fn for_stage(&self, stage: Stage) -> Method {
        match stage {
            Stage::Transcript => self.proof_method.unwrap_or(self.method),
            Stage::Translation => self.translation_method.unwrap_or(self.method),
            _ => self.method,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub path: Option<String>,
    pub text: String,
    pub media: bool,
    pub language: String,
    pub subtitles: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub step: usize,
    pub sources: Vec<Source>,
    pub pasted_text: String,
    pub title: String,
    pub language: String,
    pub deck: String,
    pub method_chosen: bool,
    pub settings: Settings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    #[default]
    Pending,
    Keep,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub id: String,
    pub term: String,
    pub meaning: String,
    pub example: String,
    pub frequency: u32,
    pub decision: Decision,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub id: String,
    pub revision: u64,
    pub stage: Stage,
    pub requested: Vec<String>,
    pub accepted: Vec<String>,
    pub draft: String,
    pub feedback: String,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Receipt {
    pub label: String,
    pub revision: u64,
    pub simulated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Only projects explicitly created through Try an example may simulate work.
    #[serde(default)]
    pub example: bool,
    #[serde(default)]
    pub native_run: Option<String>,
    #[serde(default)]
    pub native_profile: Option<Profile>,
    #[serde(default)]
    pub native_prompt: String,
    #[serde(default)]
    pub native_dirty: bool,
    #[serde(default)]
    pub native_card_count: Option<usize>,
    pub id: u64,
    pub name: String,
    pub source: Source,
    pub deck: String,
    pub settings: Settings,
    pub revision: u64,
    pub stage: Stage,
    pub status: Status,
    pub transcript: String,
    pub transcript_draft: String,
    pub translation: String,
    pub words: Vec<Word>,
    pub exchange: Option<Exchange>,
    pub exchange_sequence: u64,
    pub history: Vec<Exchange>,
    pub receipts: Vec<Receipt>,
    pub activity: Vec<String>,
    pub error: Option<String>,
    pub archived: bool,
    pub cleaned: bool,
    pub previewed_revision: Option<u64>,
}
impl Project {
    pub fn is_example(&self) -> bool {
        self.example && self.native_run.is_none()
    }
    pub fn included(&self) -> usize {
        self.words
            .iter()
            .filter(|w| w.decision == Decision::Keep)
            .count()
    }
    pub fn pending(&self) -> usize {
        self.words
            .iter()
            .filter(|w| w.decision == Decision::Pending)
            .count()
    }
    pub fn card_count(&self) -> usize {
        if self.native_run.is_some() {
            return if self.native_dirty {
                0
            } else {
                self.native_card_count.unwrap_or(0)
            };
        }
        self.included() * if self.settings.recall { 2 } else { 1 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub language: String,
    pub deck: String,
    pub exact_words: String,
    pub word_families: String,
    /// Path to an Anki collection whose seen cards define known words; empty means unused.
    #[serde(default)]
    pub anki_collection: String,
    #[serde(default)]
    pub anki_deck: String,
    /// Comma-separated names of the fields that contain the studied language.
    #[serde(default)]
    pub anki_fields: String,
    /// The one listed field whose words count as word families, on note types with a Gender field.
    #[serde(default)]
    pub anki_family_field: String,
    pub settings: Settings,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Slovenian".into(),
            language: "slv".into(),
            deck: "Languages::Slovenian".into(),
            exact_words: String::new(),
            word_families: String::new(),
            anki_collection: String::new(),
            anki_deck: String::new(),
            anki_fields: String::new(),
            anki_family_field: String::new(),
            settings: Settings::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub native_config: Option<String>,
    pub appearance: String,
    pub scale: f32,
    pub website: String,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub timeout: u32,
    pub retries: u32,
    pub batch_size: usize,
    pub max_chars: usize,
    pub ambiguity: bool,
    pub audit: bool,
    pub card_front: String,
    pub card_back: String,
    pub card_css: String,
    pub local_model_installed: bool,
    /// Run the AI steps through the locally installed Claude CLI and its subscription login.
    #[serde(default)]
    pub use_claude_cli: bool,
    /// sonnet, opus or haiku; empty means sonnet.
    #[serde(default)]
    pub claude_cli_model: String,
    /// low, medium, high, xhigh or max; empty leaves the Claude CLI default.
    #[serde(default)]
    pub claude_cli_effort: String,
    /// Claude CLI requests sent at once; 0 (unset) and 1 both mean one at a time.
    #[serde(default)]
    pub claude_cli_parallel: usize,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            native_config: None,
            appearance: "System".into(),
            scale: 1.0,
            website: String::new(),
            provider: "OpenRouter".into(),
            model: String::new(),
            endpoint: String::new(),
            timeout: 120,
            retries: 2,
            batch_size: 3,
            max_chars: 24000,
            ambiguity: false,
            audit: false,
            card_front: "{{Word}}\n{{Sentence1}}".into(),
            card_back: "{{Translation}}".into(),
            card_css: ".card { font-size: 24px; }".into(),
            local_model_installed: false,
            use_claude_cli: false,
            claude_cli_model: "sonnet".into(),
            claude_cli_effort: String::new(),
            claude_cli_parallel: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub schema: u32,
    pub next_id: u64,
    pub projects: Vec<Project>,
    pub profiles: Vec<Profile>,
    pub preferences: Preferences,
    pub setup: Setup,
    pub selected_project: Option<u64>,
    pub selected_stage: Stage,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            schema: 1,
            next_id: 1,
            projects: vec![],
            profiles: vec![Profile::default()],
            preferences: Preferences::default(),
            setup: Setup {
                language: "slv".into(),
                deck: "Languages::Slovenian".into(),
                settings: Settings {
                    local_transcription: true,
                    batch_size: 3,
                    ..Default::default()
                },
                ..Default::default()
            },
            selected_project: None,
            selected_stage: Stage::Source,
        }
    }
}

/// Accept a single JSON object, optionally in one fenced block, without guessing.
pub fn parse_reply<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    let trimmed = raw.trim();
    let body = if trimmed.starts_with("```") {
        let (header, rest) = trimmed
            .split_once('\n')
            .ok_or_else(|| Problem("Copy the complete answer, including its contents.".into()))?;
        if header != "```json" && header != "```" {
            return Err(Problem(
                "The answer should be JSON. Use the prepared follow-up request.".into(),
            ));
        }
        let body = rest.strip_suffix("```").ok_or_else(|| {
            Problem(
                "The answer looks unfinished. Wait for the chat to finish and copy it again."
                    .into(),
            )
        })?;
        if body.contains("```") {
            return Err(Problem(
                "Paste just one complete answer, not several code blocks.".into(),
            ));
        }
        body
    } else {
        trimmed
    };
    serde_json::from_str(body).map_err(|_| Problem("This answer could not be read. Paste the complete answer or copy a follow-up request; you do not need to edit its formatting.".into()))
}

pub mod pipeline;
