//! Background execution of the shared native pipeline; no CLI subprocess or interpreter.
use super::*;
use blitzline_adapters::pipeline::{self as runtime, config};
use blitzline_domain::pipeline as data;
use blitzline_engine::{
    NativeAction,
    pipeline::{Backend, Pipeline},
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

#[derive(Default)]
pub(super) struct Worker {
    queue: VecDeque<(Project, NativeAction, Preferences)>,
    active: Option<Active>,
}
struct Active {
    id: u64,
    cancel: Arc<AtomicBool>,
    receiver: mpsc::Receiver<(Project, Option<String>)>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(active) = &mut self.active {
            active.cancel.store(true, Ordering::Relaxed);
            if let Some(thread) = active.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
impl Worker {
    pub(super) fn cancel_project(&mut self, id: u64) {
        self.queue.retain(|(project, _, _)| project.id != id);
        if let Some(active) = &self.active
            && active.id == id
        {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn finish(&mut self) {
        self.pause_all();
        if let Some(active) = &mut self.active
            && let Some(thread) = active.thread.take()
        {
            let _ = thread.join();
        }
    }
    pub(super) fn busy(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }
    pub(super) fn pause_all(&mut self) {
        self.queue.clear();
        if let Some(active) = &self.active {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn enqueue(&mut self, project: Project, action: NativeAction, preferences: Preferences) {
        if matches!(action, NativeAction::Pause) {
            self.queue.retain(|(p, _, _)| p.id != project.id);
            if let Some(active) = &self.active
                && active.id == project.id
            {
                active.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        self.queue.push_back((project, action, preferences));
    }
    fn poll(
        &mut self,
        secret: &str,
        transcription_secret: &str,
    ) -> Option<(Project, Option<String>)> {
        let finished = self
            .active
            .as_ref()
            .and_then(|active| active.receiver.try_recv().ok());
        if finished.is_some()
            && let Some(mut active) = self.active.take()
            && let Some(thread) = active.thread.take()
        {
            let _ = thread.join();
        }
        if self.active.is_none()
            && let Some((project, action, preferences)) = self.queue.pop_front()
        {
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let (sender, receiver) = mpsc::channel();
            let id = project.id;
            let secret = secret.to_owned();
            let transcription_secret = transcription_secret.to_owned();
            let thread = thread::spawn(move || {
                let fallback = project.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute_with_credentials(
                        project,
                        action,
                        preferences,
                        secret,
                        transcription_secret,
                        flag,
                    )
                }));
                let result = result.unwrap_or_else(|_| (fallback, Some("Processing stopped unexpectedly. Completed work is saved; retry to continue.".into())));
                let _ = sender.send(result);
            });
            self.active = Some(Active {
                id,
                cancel,
                receiver,
                thread: Some(thread),
            });
        }
        finished
    }
}
impl Desktop {
    pub(super) fn start_native_projects(&mut self, ids: &[u64]) {
        let Some(store) = &self.store else {
            return;
        };
        let directory = store.directory.clone();
        for id in ids {
            let profile = self
                .engine
                .state
                .profiles
                .iter()
                .find(|profile| {
                    profile.name == self.engine.project(*id).unwrap().settings.profile_name
                })
                .cloned();
            let p = self.engine.project_mut(*id).unwrap();
            p.example = false;
            p.native_profile = profile;
            p.native_run = Some(
                directory
                    .join("runs")
                    .join(format!("{}-{id}", runtime::now()))
                    .to_string_lossy()
                    .into(),
            );
            p.activity = vec!["Project created. Native processing queued.".into()];
            if p.source.media || p.source.path.is_some() {
                p.transcript.clear();
                p.transcript_draft.clear();
            }
            self.engine.native_actions.push((*id, NativeAction::Run));
        }
    }
    pub(super) fn poll_native(&mut self) {
        for (id, action) in std::mem::take(&mut self.engine.native_actions) {
            if let Ok(project) = self.engine.project(id).cloned() {
                if matches!(action, NativeAction::Pause) {
                    self.engine.project_mut(id).unwrap().status = Status::Paused;
                } else {
                    self.engine.project_mut(id).unwrap().status = Status::Running;
                }
                self.worker
                    .enqueue(project, action, self.engine.state.preferences.clone());
            }
        }
        if let Some((mut project, error)) =
            self.worker.poll(&self.secret, &self.transcription_secret)
            && let Ok(current) = self.engine.project_mut(project.id)
        {
            project.archived = current.archived;
            project.name = current.name.clone();
            // A draft belongs to its exchange, and survives unrelated refreshes.
            if let (Some(old), Some(new)) = (&current.exchange, &mut project.exchange)
                && old.id == new.id
                && !old.draft.is_empty()
            {
                new.draft = old.draft.clone();
            }
            if let Some(error) = error {
                project.error = Some(error.clone());
                project.status = Status::Failed;
                self.error = error;
            }
            *current = project;
        }
    }
}
fn method(method: Method) -> &'static str {
    match method {
        Method::Browser => "browser",
        Method::Manual => "manual",
        Method::Api => "api",
    }
}
pub(super) fn connection(preferences: &Preferences) -> data::Result<Value> {
    if preferences.use_claude_cli {
        let model = Some(preferences.claude_cli_model.as_str()).filter(|m| !m.is_empty()).unwrap_or("sonnet");
        let profile = json!({"provider":config::CLAUDE_CLI,"model":model,"effort":preferences.claude_cli_effort,"timeout":preferences.timeout});
        let profiles = config::merge(json!({"profiles":{"desktop":profile}}), Path::new("."))?;
        return Ok(profiles["profiles"]["desktop"].clone());
    }
    let provider = match preferences.provider.as_str() {
        "OpenRouter" => "openrouter",
        "DeepSeek" => "deepseek",
        _ => "compatible",
    };
    let mut profile = json!({"provider":provider,"model":preferences.model,"timeout":preferences.timeout,"retries":preferences.retries});
    if !preferences.endpoint.trim().is_empty() {
        profile["base_url"] = json!(preferences.endpoint);
    }
    let profiles = config::merge(json!({"profiles":{"desktop":profile}}), Path::new("."))?;
    Ok(profiles["profiles"]["desktop"].clone())
}
/// A run saved before an AI connection was chosen has no profile to send requests with. Fill it in
/// from the current Settings; an existing profile is never replaced (that needs "Apply to this project").
fn add_missing_connection(saved: &mut Value, project: &Project, preferences: &Preferences) -> data::Result<bool> {
    let chosen = preferences.use_claude_cli || !preferences.model.trim().is_empty();
    if !chosen || saved["profiles"].get("desktop").is_some() {
        return Ok(false);
    }
    *saved = settings(saved.clone(), project, preferences)?;
    Ok(true)
}
fn settings(base: Value, project: &Project, preferences: &Preferences) -> data::Result<Value> {
    let mut value = base;
    let s = &project.settings;
    value["run"]["method"] = json!(method(s.method));
    value["run"]["proofread"] = json!(s.proofread);
    value["run"]["translate"] = json!(s.translate);
    value["run"]["proofread_method"] = json!(method(s.for_stage(Stage::Transcript)));
    value["run"]["translate_method"] = json!(method(s.for_stage(Stage::Translation)));
    value["run"]["review_method"] = json!(method(s.method));
    value["llm"]["batch_size"] = json!(s.batch_size.max(1));
    value["llm"]["max_chars"] = json!(preferences.max_chars.max(100));
    value["media"]["kind"] = json!(if s.video { "video" } else { "audio" });
    value["export"]["deck"] = json!(project.deck);
    value["export"]["two_way"] = json!(s.recall);
    if !s.model.trim().is_empty() {
        value["asr"]["model"] = json!(s.model);
    }
    if s.local_transcription {
        value["asr"]["backend"] = json!("local");
    } else if value["asr"]["backend"] == "local" {
        value["asr"]["backend"] = json!("groq");
    }
    if preferences.use_claude_cli || !preferences.model.trim().is_empty() {
        value["profiles"]["desktop"] = connection(preferences)?;
        value["llm"]["default"] = json!("desktop");
        if preferences.ambiguity {
            value["llm"]["ambiguity_review"] = json!("desktop");
        }
        if preferences.audit {
            value["llm"]["audit_review"] = json!("desktop");
        }
    }
    let defaults = Preferences::default();
    if preferences.card_front != defaults.card_front
        || preferences.card_back != defaults.card_back
        || preferences.card_css != defaults.card_css
    {
        value["export"]["templates"] = json!([]);
        value["export"]["front_template"] = json!(preferences.card_front);
        value["export"]["back_template"] = json!(preferences.card_back);
        value["export"]["css"] = json!(preferences.card_css);
        value["export"]["css_file"] = json!("");
    }
    config::validate(&value)?;
    Ok(value)
}
fn execute_with_credentials(
    mut project: Project,
    action: NativeAction,
    preferences: Preferences,
    secret: String,
    transcription_secret: String,
    cancel: Arc<AtomicBool>,
) -> (Project, Option<String>) {
    let mut pipeline = None;
    let result = (|| -> data::Result<()> {
        let directory = project
            .native_run
            .as_ref()
            .ok_or_else(|| data::Error::input("Project has no native run"))?;
        let root = Path::new(directory);
        let p = if root.join("manifest.json").is_file() {
            runtime::open(root, cancel.clone())?
        } else {
            std::fs::create_dir_all(root)?;
            let source = if let Some(path) = &project.source.path {
                std::path::PathBuf::from(path)
            } else {
                let source = root.join("input.txt");
                runtime::atomic(&source, project.source.text.as_bytes())?;
                source
            };
            let mut settings = settings(
                config::load(preferences.native_config.as_deref().map(Path::new))?,
                &project,
                &preferences,
            )?;
            if let Some(profile) = &project.native_profile {
                for (name, words) in [
                    ("skip_exact_words_file", &profile.exact_words),
                    ("skip_word_families_file", &profile.word_families),
                ] {
                    if !words.trim().is_empty() {
                        let path = root.join(format!("{name}.txt"));
                        runtime::atomic(&path, words.as_bytes())?;
                        settings["blitzer"][name] = json!(path);
                    }
                }
                if !profile.anki_collection.trim().is_empty() {
                    let fields: Vec<_> = profile.anki_fields.split(',').map(str::trim).filter(|f| !f.is_empty()).collect();
                    settings["blitzer"]["anki_collection"] = json!(profile.anki_collection.trim());
                    settings["blitzer"]["anki_deck"] = json!(profile.anki_deck.trim());
                    settings["blitzer"]["anki_fields"] = json!(fields);
                }
            }
            // Resolve attachments before creating a manifest. Otherwise a failed
            // import could be retried as a new transcription request.
            let subtitles = project
                .source
                .subtitles
                .as_ref()
                .map(|path| std::path::PathBuf::from(path).canonicalize())
                .transpose()?;
            let mut p = runtime::start(
                root,
                &source,
                &project.source.language,
                if project.source.media {
                    "media"
                } else {
                    "text"
                },
                settings,
                cancel.clone(),
            )?;
            if let Some(input) = subtitles {
                if input
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                {
                    p.job.request.asr_json = Some(input.to_string_lossy().into());
                } else {
                    p.job.request.srt = Some(input.to_string_lossy().into());
                }
            }
            p.backend.save(&p.job)?;
            p
        };
        pipeline = Some(p);
        let p = pipeline.as_mut().unwrap();
        p.backend.verbose = false;
        let api = [
            project.settings.method,
            project.settings.for_stage(Stage::Transcript),
            project.settings.for_stage(Stage::Translation),
        ]
        .contains(&Method::Api);
        if api && matches!(action, NativeAction::Run | NativeAction::Shorten)
            && add_missing_connection(&mut p.job.settings, &project, &preferences)?
        {
            p.backend.save(&p.job)?;
        }
        if [
            project.settings.method,
            project.settings.for_stage(Stage::Transcript),
            project.settings.for_stage(Stage::Translation),
        ]
        .contains(&Method::Api)
            && matches!(action, NativeAction::Run | NativeAction::Shorten)
            && !secret.trim().is_empty()
            && p.job.settings["profiles"].get("desktop").is_some_and(|d| d["provider"] != config::CLAUDE_CLI)
        {
            let current = connection(&preferences)?;
            let saved = &p.job.settings["profiles"]["desktop"];
            if current["provider"] != saved["provider"]
                || current["base_url"] != saved["base_url"]
                || current["api_key_env"] != saved["api_key_env"]
            {
                return Err(data::Error::input(
                    "This project's saved AI connection differs from Settings. Apply Project settings to update the connection before using this session's key.",
                ));
            }
            if let Some(key) = saved["api_key_env"].as_str() {
                p.backend.credentials.insert(key.into(), secret);
            }
        }
        let uses_asr = project.source.media
            && ((p.job.request.srt.is_none() && p.job.request.asr_json.is_none())
                || p.job.settings["asr"]["relisten"] == true);
        if uses_asr && matches!(action, NativeAction::Run | NativeAction::Shorten) {
            let saved = &p.job.settings["asr"];
            if project.settings.local_transcription && saved["backend"] != "local" {
                return Err(data::Error::input(
                    "This saved run uses online transcription but On this computer is selected. Apply Project settings before continuing; no recording was uploaded.",
                ));
            }
            if !transcription_secret.trim().is_empty() && saved["backend"] != "local" {
                let current = config::load(preferences.native_config.as_deref().map(Path::new))?;
                let backend = if current["asr"]["backend"] == "local" {
                    json!("groq")
                } else {
                    current["asr"]["backend"].clone()
                };
                if saved["backend"] != backend
                    || saved["endpoint"] != current["asr"]["endpoint"]
                    || saved["api_key_env"] != current["asr"]["api_key_env"]
                {
                    return Err(data::Error::input(
                        "The saved transcription connection differs from Settings. Apply Project settings before using this session's transcription key.",
                    ));
                }
                let name = saved["api_key_env"].as_str().filter(|name| !name.is_empty()).or_else(||(saved["backend"] == "groq").then_some("GROQ_API_KEY")).ok_or_else(||data::Error::input("Set asr.api_key_env in the configuration to name this HTTP worker's credential."))?;
                if p.backend
                    .credentials
                    .get(name)
                    .is_some_and(|key| key != &transcription_secret)
                {
                    return Err(data::Error::input(
                        "Use distinct credential variable names for the model and transcription connections.",
                    ));
                }
                p.backend
                    .credentials
                    .insert(name.into(), transcription_secret);
            }
        }
        match action {
            NativeAction::Run => {
                if project.native_dirty {
                    return Err(data::Error::input(
                        "Save word changes before continuing processing.",
                    ));
                }
                let pause = p.backend.path(".pause")?;
                if pause.exists() {
                    std::fs::remove_file(pause)?;
                }
                p.process(if project.settings.automatic_export {
                    None
                } else {
                    Some("media")
                })?;
            }
            NativeAction::Submit(raw) => {
                p.submit(&raw)?;
            }
            NativeAction::Refresh | NativeAction::Pause => {}
            NativeAction::Review(words) => {
                if project.revision != p.job.revision {
                    return Err(data::Error::input(
                        "This run changed in another process. Reopen it and review the current words before saving.",
                    ));
                }
                let candidates: Vec<data::Candidate> =
                    serde_json::from_slice(&p.backend.read("artifacts/04-vocabulary.json")?)?;
                let decisions = words
                    .iter()
                    .map(|word| {
                        let candidate = candidates
                            .iter()
                            .find(|candidate| candidate.id == word.id)
                            .ok_or_else(|| {
                                data::Error::input("Word no longer belongs to this source revision")
                            })?;
                        let existing = p.job.decisions.get(&word.id);
                        Ok(data::Decision {
                            candidate_id: word.id.clone(),
                            decision: match word.decision {
                                Decision::Keep => "keep",
                                Decision::Reject => "reject",
                                Decision::Pending => "needs-review",
                            }
                            .into(),
                            reason: if word.decision == Decision::Keep {
                                String::new()
                            } else if word.notes.trim().is_empty() {
                                "Marked for review in the desktop app".into()
                            } else {
                                word.notes.clone()
                            },
                            english: word.meaning.clone(),
                            notes: word.notes.clone(),
                            context_ids: existing
                                .filter(|decision| !decision.context_ids.is_empty())
                                .map(|decision| decision.context_ids.clone())
                                .unwrap_or_else(|| {
                                    candidate
                                        .contexts
                                        .iter()
                                        .filter(|context| context.error.is_empty())
                                        .take(2)
                                        .map(|context| context.id.clone())
                                        .collect()
                                }),
                        })
                    })
                    .collect::<data::Result<Vec<_>>>()?;
                p.import_review(&json!({"decisions":decisions}).to_string())?;
                project.native_dirty = false;
            }
            NativeAction::Settings(new) => {
                project.settings = new;
                p.job.settings = settings(p.job.settings.clone(), &project, &preferences)?;
                p.job.state = "paused".into();
                p.backend.save(&p.job)?;
            }
            NativeAction::Text(text) => {
                if p.job.request.source_kind != "text" {
                    return Err(data::Error::input(
                        "For recordings, import corrected subtitles and start a new project. Whole-document editing is currently available for text projects.",
                    ));
                }
                data::text::import(&text, 1000)?;
                let corrected = p.backend.path("corrected.txt")?;
                runtime::atomic(&corrected, text.as_bytes())?;
                p.job.request.text_file = Some(corrected.to_string_lossy().into());
                p.job.revision += 1;
                p.job.stages.clear();
                p.job.decisions.clear();
                p.job.pending = None;
                p.job.history.clear();
                p.job.state = "paused".into();
                p.backend.save(&p.job)?;
            }
            NativeAction::Shorten => {
                if p.job
                    .pending
                    .as_ref()
                    .is_none_or(|task| task.stage != "review")
                {
                    return Err(data::Error::input(
                        "Only vocabulary requests can be split. Review text directly or choose another service.",
                    ));
                }
                let size = p.job.settings["llm"]["batch_size"].as_u64().unwrap_or(20);
                if size <= 1 {
                    return Err(data::Error::input("This request is already one word."));
                }
                p.job.settings["llm"]["batch_size"] = json!(size.div_ceil(2));
                p.job.revision += 1;
                p.job.pending = None;
                p.job.stages.retain(|stage, _| stage.as_str() < "05-review");
                p.backend.save(&p.job)?;
                p.process(Some("media"))?;
            }
            NativeAction::Export(format) => {
                if project.previewed_revision != Some(p.job.revision) {
                    return Err(data::Error::input(
                        "The run changed after your preview. Preview the current cards before exporting.",
                    ));
                }
                p.job.settings["export"]["format"] = json!(match format.as_str() {
                    "APKG" => "apkg",
                    "Anki" => "anki",
                    _ => "tsv",
                });
                p.export_current()?;
                p.job.state = "complete".into();
                p.backend.save(&p.job)?;
            }
            NativeAction::Clean => {
                p.backend.cleanup(&p.job, true)?;
                p.job = p.backend.load()?;
            }
        }
        Ok(())
    })();
    if let Some(pipeline) = pipeline {
        snapshot(&mut project, &pipeline);
    }
    let error = result
        .err()
        .filter(|error| !["needs_review", "paused"].contains(&error.code.as_str()))
        .map(|error| error.message);
    (project, error)
}
#[cfg(test)]
fn execute(
    project: Project,
    action: NativeAction,
    preferences: Preferences,
    secret: String,
    cancel: Arc<AtomicBool>,
) -> (Project, Option<String>) {
    execute_with_credentials(project, action, preferences, secret, String::new(), cancel)
}
fn artifact<T: serde::de::DeserializeOwned>(
    p: &Pipeline<runtime::Runtime>,
    path: &str,
) -> Option<T> {
    serde_json::from_slice(&p.backend.read(path).ok()?).ok()
}
fn snapshot(project: &mut Project, p: &Pipeline<runtime::Runtime>) {
    let job = &p.job;
    project.revision = job.revision;
    project.cleaned = job.state == "cleaned";
    let cues: Vec<data::Cue> = artifact(p, "artifacts/02-proofread.json")
        .or_else(|| artifact(p, "artifacts/01-transcript.json"))
        .unwrap_or_default();
    project.transcript = data::text::render_text(&cues);
    project.transcript_draft = project.transcript.clone();
    let translated: Vec<data::Cue> = artifact(p, "artifacts/03-translate.json").unwrap_or_default();
    project.translation = data::text::render_text(&translated);
    let candidates: Vec<data::Candidate> =
        artifact(p, "artifacts/04-vocabulary.json").unwrap_or_default();
    if !project.native_dirty {
        project.words = candidates
            .iter()
            .map(|candidate| {
                let decision = job.decisions.get(&candidate.id);
                Word {
                    id: candidate.id.clone(),
                    term: candidate.term.clone(),
                    meaning: decision.map(|d| d.english.clone()).unwrap_or_default(),
                    example: candidate
                        .contexts
                        .first()
                        .map(|c| c.text.clone())
                        .unwrap_or_default(),
                    frequency: candidate.source_frequency.min(u32::MAX as u64) as u32,
                    decision: match decision.map(|d| d.decision.as_str()) {
                        Some("keep") => Decision::Keep,
                        Some("reject") => Decision::Reject,
                        _ => Decision::Pending,
                    },
                    notes: decision.map(|d| d.notes.clone()).unwrap_or_default(),
                }
            })
            .collect();
    }
    let prepared = job
        .stages
        .get("06-media")
        .is_some_and(|stage| stage.state == "complete")
        && job.omitted == 0;
    project.native_card_count = if prepared {
        (|| -> data::Result<usize> {
            let artifact: Value =
                serde_json::from_slice(&p.backend.read("artifacts/06-media.json")?)?;
            let cards: Vec<data::Card> = serde_json::from_value(artifact["cards"].clone())?;
            let settings = p
                .backend
                .export_settings(&job.settings, &job.request.language)?;
            let model = data::notes::definition(&settings)?;
            cards.iter().try_fold(0, |total, card| {
                let fields = data::notes::render_fields(card, &settings, &model)?
                    .into_iter()
                    .collect();
                Ok(total
                    + model["templates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|template| {
                            data::notes::template_active(
                                template["front"].as_str().unwrap(),
                                &fields,
                            )
                        })
                        .count())
            })
        })()
        .ok()
    } else {
        None
    };
    project.stage = if job.state == "complete" {
        Stage::Export
    } else if prepared {
        Stage::Cards
    } else if !candidates.is_empty() {
        Stage::Words
    } else if job
        .pending
        .as_ref()
        .is_some_and(|task| task.stage == "translate")
    {
        Stage::Translation
    } else {
        Stage::Transcript
    };
    project.status = match job.state.as_str() {
        "complete" => Status::Finished,
        "failed" => Status::Failed,
        "needs-review" => Status::Waiting,
        _ if prepared => Status::Ready,
        _ => Status::Paused,
    };
    project.error = if job.state == "failed" {
        Some(job.message.clone())
    } else {
        None
    };
    project.exchange = job.pending.as_ref().map(|task| Exchange {
        id: task.id.clone(),
        revision: task.revision,
        stage: match task.stage.as_str() {
            "proofread" => Stage::Transcript,
            "translate" => Stage::Translation,
            _ => Stage::Words,
        },
        requested: task.requested.clone(),
        accepted: task.accepted.keys().cloned().collect(),
        draft: task.draft.clone(),
        feedback: if task.errors.is_empty() {
            format!(
                "{} answers saved. {} still need attention.",
                task.accepted.len(),
                task.requested.len() - task.accepted.len()
            )
        } else {
            serde_json::to_string_pretty(&task.errors).unwrap_or_default()
        },
        checked: !task.accepted.is_empty(),
    });
    project.native_prompt = job
        .pending
        .as_ref()
        .map(|task| task.prompt())
        .unwrap_or_default();
    project.exchange_sequence = job
        .pending
        .as_ref()
        .map_or(0, |task| task.sequence as u64 + 1);
    project.receipts = job
        .exports
        .iter()
        .map(|(format, paths)| Receipt {
            label: format!(
                "{}: {}",
                format.to_uppercase(),
                paths
                    .keys()
                    .map(|path| p.backend.root.join(path).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            revision: job.revision,
            simulated: false,
        })
        .collect();
    if project.previewed_revision != Some(job.revision) {
        project.previewed_revision = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../tests/fixtures/reference/text.json")).unwrap()
    }
    fn project(root: &Path) -> Project {
        let source = root.join("sample.txt");
        std::fs::write(&source, fixture()["raw"].as_str().unwrap()).unwrap();
        let vocabulary = root.join("vocabulary.json");
        std::fs::write(&vocabulary, fixture()["rows"].to_string()).unwrap();
        let mut settings = config::defaults();
        settings["run"]["method"] = json!("browser");
        let mut p = runtime::start(
            &root.join("run"),
            &source,
            "slv",
            "text",
            settings,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        p.job.request.vocabulary_json = Some(vocabulary.to_string_lossy().into());
        p.backend.save(&p.job).unwrap();
        let mut engine = Engine::new(Workspace::default());
        let id = engine.example().unwrap();
        let mut project = engine.project(id).unwrap().clone();
        project.native_run = Some(p.backend.root.to_string_lossy().into());
        project.source.path = Some(source.to_string_lossy().into());
        project
    }
    fn apply(project: Project, action: NativeAction) -> Project {
        let (project, error) = execute(
            project,
            action,
            Preferences::default(),
            String::new(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(error.is_none(), "{error:?}");
        project
    }
    #[test]
    fn desktop_uses_persisted_native_exchanges_and_real_exports() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = apply(project(directory.path()), NativeAction::Run);
        assert_eq!(project.status, Status::Waiting);
        assert_eq!(project.words.len(), 2);
        assert!(project.native_prompt.contains("context_ids"));
        let task = project.exchange.as_ref().unwrap();
        let reply = json!({"exchange_id":task.id,"revision":task.revision,"decisions":fixture()["decisions"]}).to_string();
        project = apply(project, NativeAction::Submit(reply));
        assert_eq!(project.exchange.as_ref().unwrap().accepted.len(), 2);
        assert!(!directory.path().join("run/exports").exists());
        project = apply(project, NativeAction::Run);
        assert_eq!(project.status, Status::Ready);
        assert_eq!(project.words[0].meaning, "cat");
        let mut engine = Engine::new(Workspace::default());
        let id = project.id;
        engine.state.projects.push(project);
        engine.preview(id).unwrap();
        engine.export_demo(id, "APKG").unwrap();
        let (_, action) = engine.native_actions.pop().unwrap();
        let project = apply(engine.project(id).unwrap().clone(), action);
        assert_eq!(project.status, Status::Finished);
        assert!(project.receipts.iter().all(|receipt| !receipt.simulated));
        assert!(
            directory
                .path()
                .join("run/exports/apkg/cards.apkg")
                .is_file()
        );
    }
    #[test]
    fn direct_edits_invalidate_native_review_and_rebuild_cards() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = apply(project(directory.path()), NativeAction::Run);
        for word in &mut project.words {
            word.decision = Decision::Keep;
            word.meaning = format!("Meaning of {}", word.term);
        }
        let words = project.words.clone();
        project = apply(project, NativeAction::Review(words));
        assert!(project.exchange.is_none());
        project = apply(project, NativeAction::Run);
        assert_eq!(project.status, Status::Ready);
        assert!(project.words[0].meaning.starts_with("Meaning of"));
    }
    #[test]
    fn native_projects_cannot_enter_the_demo_tick_or_generate_sample_answers() {
        let directory = tempfile::tempdir().unwrap();
        let mut p = project(directory.path());
        p.status = Status::Queued;
        let mut engine = Engine::new(Workspace::default());
        let id = p.id;
        engine.state.projects.push(p.clone());
        engine.tick();
        assert_eq!(&p, engine.project(id).unwrap());
        assert!(engine.sample_reply(id).is_err());
        engine.resume(id).unwrap();
        assert!(matches!(engine.native_actions[0], (_, NativeAction::Run)));
    }
    #[test]
    fn reopening_refreshes_saved_results_without_continuing_processing() {
        let directory = tempfile::tempdir().unwrap();
        let p = apply(project(directory.path()), NativeAction::Run);
        let p = apply(p, NativeAction::Refresh);
        assert_eq!(p.status, Status::Waiting);
        assert!(p.exchange.is_some());
        assert!(!directory.path().join("run/exports").exists());
    }
    #[test]
    fn unsaved_word_drafts_survive_refresh_and_block_preview() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = apply(project(directory.path()), NativeAction::Run);
        project.words[0].meaning = "An unsaved meaning".into();
        project.native_dirty = true;
        let project = apply(project, NativeAction::Refresh);
        assert_eq!(project.words[0].meaning, "An unsaved meaning");
        assert!(project.native_dirty);
        let id = project.id;
        let mut engine = Engine::new(Workspace::default());
        engine.state.projects.push(project);
        assert!(engine.preview(id).unwrap_err().0.contains("Save"));
    }
    #[test]
    fn missing_subtitle_attachment_cannot_create_an_asr_ready_manifest() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = project(directory.path());
        project.native_run = Some(directory.path().join("new-run").to_string_lossy().into());
        project.source.media = true;
        project.source.subtitles = Some(
            directory
                .path()
                .join("missing.srt")
                .to_string_lossy()
                .into(),
        );
        let (_, error) = execute(
            project,
            NativeAction::Run,
            Preferences::default(),
            String::new(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(error.is_some());
        assert!(!directory.path().join("new-run/manifest.json").exists());
    }
    #[test]
    fn session_credentials_are_not_sent_to_a_previous_provider() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = project(directory.path());
        project.settings.method = Method::Api;
        let root = std::path::PathBuf::from(project.native_run.as_ref().unwrap());
        let mut pipeline = runtime::open(&root, Arc::new(AtomicBool::new(false))).unwrap();
        pipeline.job.settings["profiles"]["desktop"] = connection(&Preferences {
            provider: "OpenRouter".into(),
            model: "model".into(),
            ..Default::default()
        })
        .unwrap();
        pipeline.backend.save(&pipeline.job).unwrap();
        drop(pipeline);
        let (_, error) = execute(
            project,
            NativeAction::Run,
            Preferences {
                provider: "DeepSeek".into(),
                model: "model".into(),
                ..Default::default()
            },
            "NEVER-SEND-TO-THE-OLD-SERVICE".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(error.unwrap().contains("saved AI connection differs"));
        let saved = std::fs::read_to_string(root.join("manifest.json")).unwrap();
        assert!(!saved.contains("NEVER-SEND"));
        assert!(!root.join("artifacts").exists());
    }
    #[test]
    fn a_run_saved_without_a_connection_gets_the_claude_cli_one() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = project(directory.path());
        project.settings.method = Method::Api;
        let mut saved = settings(config::defaults(), &project, &Preferences::default()).unwrap();
        assert_eq!(saved["llm"]["default"], "");
        let cli = Preferences { use_claude_cli: true, claude_cli_model: "opus".into(), ..Default::default() };
        assert!(add_missing_connection(&mut saved, &project, &cli).unwrap());
        assert_eq!(saved["llm"]["default"], "desktop");
        assert_eq!(saved["profiles"]["desktop"]["provider"], "claude-cli");
        assert_eq!(saved["profiles"]["desktop"]["model"], "opus");
        // An existing connection is left alone.
        let other = Preferences { use_claude_cli: true, claude_cli_model: "haiku".into(), ..Default::default() };
        assert!(!add_missing_connection(&mut saved, &project, &other).unwrap());
        assert_eq!(saved["profiles"]["desktop"]["model"], "opus");
    }
    #[test]
    fn card_counts_use_the_actual_note_templates() {
        let directory = tempfile::tempdir().unwrap();
        let mut project = apply(project(directory.path()), NativeAction::Run);
        let task = project.exchange.as_ref().unwrap();
        let reply = json!({"exchange_id":task.id,"revision":task.revision,"decisions":fixture()["decisions"]}).to_string();
        project = apply(project, NativeAction::Submit(reply));
        project = apply(project, NativeAction::Run);
        assert_eq!(project.card_count(), 2);
        let mut settings = project.settings.clone();
        settings.recall = true;
        project = apply(project, NativeAction::Settings(settings));
        assert_eq!(project.card_count(), 4);
    }
    #[test]
    fn local_transcription_choice_overrides_a_remote_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let project = project(directory.path());
        let mut base = config::defaults();
        base["asr"]["backend"] = json!("http");
        base["asr"]["endpoint"] = json!("https://worker.example/transcribe");
        let actual = settings(base, &project, &Preferences::default()).unwrap();
        assert!(project.settings.local_transcription);
        assert_eq!(actual["asr"]["backend"], "local");
    }
}
