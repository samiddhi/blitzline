//! Shared production workflow. All effects are supplied by the application's backend.
use blitzline_domain::pipeline::*;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
/// Requests run on scoped threads, so the backend must be shareable between them.
pub trait Backend: Sync {
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    fn write(&self, path: &str, bytes: &[u8]) -> Result<()>;
    fn fingerprint(&self, path: &str) -> Result<String>;
    fn external(&self, path: &str) -> Result<Vec<u8>>;
    fn external_hash(&self, path: &str) -> Result<String>;
    fn save(&self, job: &Job) -> Result<()>;
    fn dependencies(&self, kind: &str, job: &Job) -> Result<Value>;
    fn probe(&self, request: &Request, settings: &Value) -> Result<MediaInfo>;
    fn transcribe(&self, request: &Request, settings: &Value, info: &MediaInfo) -> Result<Value>;
    fn relisten(&self, request: &Request, settings: &Value, cue: &Cue) -> Result<String>;
    fn extract(&self, request: &Request, settings: &Value) -> Result<Value>;
    fn request(&self, profile: &Value, task: &Task) -> Result<Value>;
    fn forms(
        &self,
        settings: &Value,
        candidates: &[Candidate],
    ) -> Result<BTreeMap<String, Vec<String>>>;
    fn clips(
        &self,
        cards: &[Card],
        request: &Request,
        info: &MediaInfo,
        settings: &Value,
    ) -> Result<(Vec<Card>, Vec<Value>)>;
    fn export(&self, cards: &[Card], settings: &Value) -> Result<Vec<String>>;
    fn export_settings(&self, settings: &Value, language: &str) -> Result<Value>;
    fn cancelled(&self) -> bool;
    fn progress(&self, stage: &str, state: &str);
}
pub struct Pipeline<B> {
    pub backend: B,
    pub job: Job,
}
impl<B: Backend> Pipeline<B> {
    fn checkpoint(&self) -> Result<()> {
        self.backend.save(&self.job)?;
        if self.backend.cancelled() {
            Err(Error::paused())
        } else {
            Ok(())
        }
    }
    fn load<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_slice(&self.backend.read(path)?)?)
    }
    fn store<T: serde::Serialize + ?Sized>(&self, path: &str, value: &T) -> Result<()> {
        self.backend.write(path, &serde_json::to_vec_pretty(value)?)
    }
    fn stage(
        &mut self,
        name: &str,
        inputs: Value,
        action: impl FnOnce(&mut Self) -> Result<(Value, Vec<String>)>,
    ) -> Result<Value> {
        self.checkpoint()?;
        let key = digest(&json!({"engine":1,"inputs":inputs}));
        if let Some(saved) = self.job.stages.get(name) {
            if saved.key == key
                && saved.state == "complete"
                && !saved.outputs.is_empty()
                && saved
                    .outputs
                    .iter()
                    .all(|(p, h)| self.backend.fingerprint(p).is_ok_and(|v| v == *h))
            {
                self.backend.progress(name, "reused");
                return self.load(&format!("artifacts/{name}.json"));
            }
            if saved.key != key && name < "05-review" {
                self.job.revision += 1;
                self.job.pending = None;
                self.job.decisions.clear();
            }
        }
        self.job.stages.retain(|n, _| n.as_str() < name);
        self.job.stages.insert(
            name.into(),
            StageReceipt {
                key: key.clone(),
                state: "running".into(),
                outputs: BTreeMap::new(),
            },
        );
        self.job.state = "running".into();
        self.checkpoint()?;
        self.backend.progress(name, "running");
        match action(self) {
            Ok((value, mut paths)) => {
                let path = format!("artifacts/{name}.json");
                self.store(&path, &value)?;
                paths.push(path);
                let mut outputs = BTreeMap::new();
                for p in paths {
                    outputs.insert(p.clone(), self.backend.fingerprint(&p)?);
                }
                self.job.stages.insert(
                    name.into(),
                    StageReceipt {
                        key,
                        state: "complete".into(),
                        outputs,
                    },
                );
                self.checkpoint()?;
                self.backend.progress(name, "complete");
                Ok(value)
            }
            Err(e) => {
                self.job.state = if e.code == "needs_review" {
                    "needs-review"
                } else if e.code == "paused" {
                    "paused"
                } else {
                    "failed"
                }
                .into();
                self.job.message = e.message.clone();
                if let Some(s) = self.job.stages.get_mut(name) {
                    s.state = self.job.state.clone();
                }
                self.backend.save(&self.job)?;
                Err(e)
            }
        }
    }
    pub fn process(&mut self, stop_after: Option<&str>) -> Result<()> {
        let result = self.process_inner(stop_after);
        if let Err(error) = &result
            && !["cleaned", "cleaning"].contains(&self.job.state.as_str())
        {
            self.job.state = match error.code.as_str() {
                "needs_review" => "needs-review",
                "paused" => "paused",
                _ => "failed",
            }
            .into();
            self.job.message = error.message.clone();
            self.backend.save(&self.job)?;
        }
        result
    }
    fn process_inner(&mut self, stop_after: Option<&str>) -> Result<()> {
        if ["cleaned", "cleaning"].contains(&self.job.state.as_str()) {
            return Err(Error::input(
                "This run is cleaned or cleanup is in progress. Finish interrupted cleanup with clean --apply; processing cannot resume.",
            ));
        }
        if stop_after.is_some_and(|s| !STAGES.contains(&s)) {
            return Err(Error::input("Unknown stop stage"));
        }
        if self.backend.external_hash(&self.job.request.source)? != self.job.request.source_hash {
            return Err(Error::operation(
                "The original source input changed. Restore it or start a new run; use --text-file for a corrected text version with the original note identities.",
            ));
        }
        if self
            .job
            .pending
            .as_ref()
            .is_some_and(|t| t.accepted.len() == t.requested.len())
        {
            self.job.history.push(self.job.pending.take().unwrap());
        }
        let mut hashes = BTreeMap::new();
        hashes.insert(
            self.job.request.source.clone(),
            self.job.request.source_hash.clone(),
        );
        for p in [
            &self.job.request.text_file,
            &self.job.request.srt,
            &self.job.request.asr_json,
            &self.job.request.vocabulary_json,
        ]
        .into_iter()
        .flatten()
        {
            hashes.insert(p.clone(), self.backend.external_hash(p)?);
        }
        self.job.input_hashes = hashes.clone();
        let source = self.job.request.clone();
        let config = self.job.settings.clone();
        let media = source.source_kind == "media";
        if media {
            self.capture_dependencies("media")?;
            if source.srt.is_none() && source.asr_json.is_none() {
                self.capture_dependencies("asr")?;
            }
        }
        let info = self.stage(
            "00-source",
            json!([
                source.source_hash,
                media,
                if media {
                    config["media"].clone()
                } else {
                    json!(null)
                },
                if media {
                    self.backend.dependencies("media", &self.job)?
                } else {
                    json!(null)
                }
            ]),
            |p| {
                let info = if media {
                    Some(p.backend.probe(&source, &config)?)
                } else {
                    None
                };
                Ok((json!(info), vec![]))
            },
        )?;
        let info: Option<MediaInfo> = serde_json::from_value(info)?;
        let transcript = self.stage(
            "01-transcript",
            json!([
                source.source_hash,
                source.text_file.as_ref().and_then(|p| hashes.get(p)),
                source.srt.as_ref().and_then(|p| hashes.get(p)),
                source.asr_json.as_ref().and_then(|p| hashes.get(p)),
                if media {
                    config["asr"].clone()
                } else {
                    json!(null)
                },
                if media && source.srt.is_none() && source.asr_json.is_none() {
                    self.backend.dependencies("asr", &self.job)?
                } else {
                    json!(null)
                }
            ]),
            |p| {
                let cues = if !media {
                    let path = source.text_file.as_ref().unwrap_or(&source.source);
                    text::import(
                        &String::from_utf8(p.backend.external(path)?)
                            .map_err(|_| Error::input("Source must be UTF-8 plain text"))?,
                        1000,
                    )?
                } else if let Some(path) = &source.srt {
                    text::parse_srt(
                        &String::from_utf8(p.backend.external(path)?)
                            .map_err(|_| Error::input("SRT must be UTF-8"))?,
                        info.as_ref().unwrap().duration_ms,
                    )?
                } else {
                    let data = if let Some(path) = &source.asr_json {
                        serde_json::from_slice(&p.backend.external(path)?)?
                    } else {
                        p.backend
                            .transcribe(&source, &config, info.as_ref().unwrap())?
                    };
                    text::normalize_asr(&data, info.as_ref().unwrap().duration_ms, &config["asr"])?
                };
                if cues.is_empty() {
                    return Err(Error::input("The source contains no usable text"));
                }
                Ok((json!(cues), vec![]))
            },
        )?;
        if stop_after == Some("transcript") {
            return self.stopped();
        }
        let original: Vec<Cue> = serde_json::from_value(transcript.clone())?;
        if media && config["run"]["proofread"] == true && config["asr"]["relisten"] == true {
            self.capture_dependencies("asr")?;
        }
        let proof = self.stage(
            "02-proofread",
            json!([
                transcript,
                config["run"]["proofread"],
                config["llm"],
                config["profiles"],
                config["run"]["method"],
                config["run"]["proofread_method"],
                if media && config["asr"]["relisten"] == true {
                    json!([config["asr"], self.backend.dependencies("asr", &self.job)?])
                } else {
                    json!(null)
                }
            ]),
            |p| {
                let cues = if config["run"]["proofread"].as_bool().unwrap_or(false) {
                    p.cue_review(&original, "proofread")?
                } else {
                    original.clone()
                };
                Ok((json!(cues), vec![]))
            },
        )?;
        let cues: Vec<Cue> = serde_json::from_value(proof.clone())?;
        let (plain, _) = text::plaintext(&cues);
        self.backend
            .write("artifacts/source.txt", text::render_text(&cues).as_bytes())?;
        self.backend
            .write("artifacts/canonical.txt", plain.as_bytes())?;
        if media {
            self.backend.write(
                "artifacts/source.srt",
                text::render_srt(
                    &cues,
                    config["asr"]["line_length"].as_u64().unwrap_or(42) as usize,
                )?
                .as_bytes(),
            )?;
        }
        if stop_after == Some("proofread") {
            return self.stopped();
        }
        self.stage(
            "03-translate",
            json!([
                proof,
                config["run"]["translate"],
                config["llm"],
                config["profiles"],
                config["run"]["method"],
                config["run"]["translate_method"]
            ]),
            |p| {
                if !config["run"]["translate"].as_bool().unwrap_or(false) {
                    return Ok((json!({"skipped":true}), vec![]));
                }
                let translated = p.cue_review(&cues, "translate")?;
                let path = if media {
                    "artifacts/english.srt"
                } else {
                    "artifacts/english.txt"
                };
                let raw = if media {
                    text::render_srt(&translated, 42)?
                } else {
                    text::render_text(&translated)
                };
                p.backend.write(path, raw.as_bytes())?;
                Ok((json!(translated), vec![path.into()]))
            },
        )?;
        if stop_after == Some("translate") {
            return self.stopped();
        }
        self.capture_dependencies("blitzer")?;
        let vocab = self.stage(
            "04-vocabulary",
            json!([
                proof,
                source.language,
                config["blitzer"],
                source.vocabulary_json.as_ref().and_then(|p| hashes.get(p)),
                self.backend.dependencies("blitzer", &self.job)?
            ]),
            |p| {
                let rows = if let Some(path) = &source.vocabulary_json {
                    serde_json::from_slice(&p.backend.external(path)?)?
                } else {
                    p.backend.extract(&source, &config)?
                };
                let candidates = vocabulary::map_candidates(&rows, &cues)?;
                Ok((json!(candidates), vec![]))
            },
        )?;
        let candidates: Vec<Candidate> = serde_json::from_value(vocab.clone())?;
        if stop_after == Some("vocabulary") {
            return self.stopped();
        }
        self.capture_dependencies("forms")?;
        let reviewed=self.stage("05-review",json!([vocab,config["llm"],config["profiles"],config["run"]["method"],config["run"]["review_method"],self.backend.dependencies("forms",&self.job)?]),|p|{
            p.vocabulary_review(&candidates)?;let review=json!({"decisions":candidates.iter().filter_map(|c|p.job.decisions.get(&c.id)).collect::<Vec<_>>()});p.store("artifacts/review.json",&review)?;Ok((review,vec!["artifacts/review.json".into()]))
        })?;
        let decisions = reviewed["decisions"]
            .as_array()
            .ok_or_else(|| Error::operation("Corrupt review artifact"))?
            .iter()
            .map(|r| {
                serde_json::from_value::<Decision>(r.clone()).map(|d| (d.candidate_id.clone(), d))
            })
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        self.job.decisions = decisions;
        let unresolved = candidates
            .iter()
            .filter(|c| {
                self.job
                    .decisions
                    .get(&c.id)
                    .is_none_or(|d| d.decision == "needs-review")
            })
            .count();
        self.job.omitted = unresolved;
        if stop_after == Some("review") {
            return self.stopped();
        }
        if unresolved > 0 && !config["run"]["allow_partial"].as_bool().unwrap_or(false) {
            self.job.state = "needs-review".into();
            self.job.message = format!(
                "{unresolved} candidates still need review. Edit artifacts/review.json, then resume with --review FILE, or explicitly select --allow-partial."
            );
            self.backend.save(&self.job)?;
            return Err(Error::review(&self.job.message));
        }
        let mut cards = vocabulary::build_cards(&self.job.decisions, &candidates, &source);
        let resolved = self.backend.export_settings(&config, &source.language)?;
        let max_words = resolved["max_sentence_words"].as_u64().unwrap_or(25) as usize;
        for card in &mut cards {
            for context in &mut card.contexts {
                *context = vocabulary::shorten(context, max_words);
                vocabulary::retime(context, &cues)?;
            }
        }
        let mut media_config = config.clone();
        media_config["export"]["max_sentence_words"] = json!(max_words);
        let built = self.stage(
            "06-media",
            json!([
                cards,
                if media {
                    config["media"].clone()
                } else {
                    json!(null)
                },
                if media {
                    self.backend.dependencies("media", &self.job)?
                } else {
                    json!(null)
                }
            ]),
            |p| {
                let (cards, errors) = if media {
                    p.backend
                        .clips(&cards, &source, info.as_ref().unwrap(), &media_config)?
                } else {
                    (cards.clone(), vec![])
                };
                let paths = cards
                    .iter()
                    .flat_map(|card| &card.media)
                    .flat_map(|name| [format!("media/{name}"), format!("media/{name}.json")])
                    .collect();
                Ok((json!({"cards":cards,"errors":errors}), paths))
            },
        )?;
        let cards: Vec<Card> = serde_json::from_value(built["cards"].clone())?;
        self.job.cards = cards.len();
        self.job.omitted += built["errors"].as_array().map_or(0, Vec::len);
        if built["errors"].as_array().is_some_and(|e| !e.is_empty())
            && !config["run"]["allow_partial"].as_bool().unwrap_or(false)
        {
            self.job.state = "needs-review".into();
            self.backend.save(&self.job)?;
            return Err(Error::review(
                "Some examples could not produce valid clips. Review artifacts/06-media.json before exporting.",
            ));
        }
        if stop_after == Some("media") {
            return self.stopped();
        }
        self.export_current()?;
        self.job.state = "complete".into();
        self.job.message.clear();
        self.backend.save(&self.job)
    }
    fn capture_dependencies(&mut self, kind: &str) -> Result<()> {
        let dependencies = self.backend.dependencies(kind, &self.job)?;
        for (path, checksum) in dependencies.as_object().into_iter().flatten() {
            if let Some(checksum) = checksum.as_str().filter(|h| h.len() == 64) {
                self.job.input_hashes.insert(path.clone(), checksum.into());
            }
        }
        Ok(())
    }
    fn stopped(&mut self) -> Result<()> {
        self.job.state = "paused".into();
        self.job.message = "Stopped at the requested stage.".into();
        self.backend.save(&self.job)
    }
    fn profile(&self, stage: &str) -> Result<Option<Value>> {
        let method = self.job.settings["run"]
            .get(format!("{stage}_method"))
            .and_then(Value::as_str)
            .filter(|method| *method != "auto")
            .or_else(|| self.job.settings["run"]["method"].as_str())
            .unwrap_or("auto");
        if method != "auto" && method != "api" {
            return Ok(None);
        }
        let llm = &self.job.settings["llm"];
        let name = llm[stage]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| llm["default"].as_str().filter(|s| !s.is_empty()));
        let Some(name) = name else {
            if method == "api" {
                return Err(Error::input(format!(
                    "Configure an LLM profile for {stage}"
                )));
            }
            return Ok(None);
        };
        Ok(Some(
            self.job.settings["profiles"]
                .get(name)
                .ok_or_else(|| Error::input(format!("Unknown LLM profile {name}")))?
                .clone(),
        ))
    }
    /// Runs a stage's requests, up to the profile's `parallel` count at once, and returns each
    /// accepted answer in batch order. Answers are applied one batch at a time, as before.
    fn run_batches(&mut self, stage: &str, batches: Vec<Batch>) -> Result<Vec<Value>> {
        let mut answers = self.prefetch(stage, &batches)?;
        batches
            .into_iter()
            .zip(answers.iter_mut())
            .map(|(batch, answer)| self.task(stage, batch, answer.take()))
            .collect()
    }
    /// Sends the first request of each batch concurrently. A failed request is kept as its error
    /// and raised when its batch is reached, so a failure stops the run where it always did.
    fn prefetch(&self, stage: &str, batches: &[Batch]) -> Result<Vec<Option<Result<Value>>>> {
        let mut answers: Vec<_> = batches.iter().map(|_| None).collect();
        let Some(profile) = self.profile(stage)? else {
            return Ok(answers);
        };
        let width = profile["parallel"].as_u64().unwrap_or(1).max(1) as usize;
        if width == 1 {
            return Ok(answers);
        }
        let max = self.job.settings["llm"]["max_chars"]
            .as_u64()
            .unwrap_or(24000) as usize;
        let work = batches
            .iter()
            .enumerate()
            .map(|(index, batch)| {
                let id = task_id(self.job.revision, stage, batch.sequence, &batch.requested);
                (index, new_task(id, self.job.revision, stage, batch))
            })
            .filter(|(_, task)| !self.job.history.iter().any(|t| t.id == task.id))
            .filter(|(_, task)| canonical(&task.request_data()).chars().count() <= max)
            .collect::<Vec<_>>();
        let next = AtomicUsize::new(0);
        let done = Mutex::new(Vec::new());
        let backend = &self.backend;
        thread::scope(|scope| {
            for _ in 0..width.min(work.len()) {
                scope.spawn(|| {
                    while let Some((index, task)) = work.get(next.fetch_add(1, Ordering::Relaxed)) {
                        if backend.cancelled() {
                            break;
                        }
                        let answer = backend.request(&profile, task);
                        done.lock().unwrap().push((*index, answer));
                    }
                });
            }
        });
        for (index, answer) in done.into_inner().unwrap() {
            answers[index] = Some(answer);
        }
        Ok(answers)
    }
    fn task(
        &mut self,
        stage: &str,
        batch: Batch,
        mut answer: Option<Result<Value>>,
    ) -> Result<Value> {
        let id = task_id(self.job.revision, stage, batch.sequence, &batch.requested);
        if let Some(saved) = self.job.history.iter().find(|t| t.id == id) {
            return Ok(task_value(saved));
        }
        if self.job.pending.as_ref().is_none_or(|p| p.id != id) {
            self.job.pending = Some(new_task(id, self.job.revision, stage, &batch));
        }
        let limit = self.job.settings["llm"]["repair_attempts"]
            .as_u64()
            .unwrap_or(2) as usize;
        loop {
            // This happens only during explicit processing, never while checking
            // a pasted reply. Hosted ASR can incur a charge on resume.
            if stage == "proofread"
                && self.job.settings["asr"]["relisten"] == true
                && self.job.request.source_kind == "media"
            {
                let task = self.job.pending.as_ref().unwrap();
                let ids = task
                    .errors
                    .iter()
                    .filter(|error| error["uncertain"] == true)
                    .filter_map(|error| error["cue_id"].as_str())
                    .filter(|id| task.data["second_asr"].get(*id).is_none())
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>();
                if !ids.is_empty() {
                    let cues: Vec<Cue> = self.load("artifacts/01-transcript.json")?;
                    for id in ids {
                        let cue = cues
                            .iter()
                            .find(|cue| cue.id == id)
                            .ok_or_else(|| Error::input("Missing source cue for second ASR"))?;
                        self.capture_dependencies("asr")?;
                        let evidence =
                            self.backend
                                .relisten(&self.job.request, &self.job.settings, cue)?;
                        let task = self.job.pending.as_mut().unwrap();
                        task.data["second_asr"][&id] = json!(evidence);
                        task.attempts = 0;
                        self.checkpoint()?;
                    }
                }
            }
            let task = self.job.pending.as_ref().unwrap();
            if task.accepted.len() == task.requested.len() {
                let task = self.job.pending.take().unwrap();
                let value = task_value(&task);
                self.job.history.push(task);
                self.checkpoint()?;
                return Ok(value);
            }
            if canonical(&task.request_data()).chars().count()
                > self.job.settings["llm"]["max_chars"]
                    .as_u64()
                    .unwrap_or(24000) as usize
            {
                return Err(Error::input(
                    "The request exceeds llm.max_chars including its context. Reduce batch_size or increase max_chars; related candidates must stay together.",
                ));
            }
            self.store("artifacts/pending.json", task)?;
            self.backend
                .write("artifacts/prompt.txt", task.prompt().as_bytes())?;
            self.checkpoint()?;
            let Some(profile) = self.profile(stage)? else {
                return Err(Error::review(
                    "Review is ready. Use `blitzline prompt RUN_DIR`, paste the request into a browser chat, then `blitzline submit RUN_DIR --file REPLY`. The GUI opens the same saved exchange.",
                ));
            };
            if task.attempts > limit {
                return Err(Error::review(
                    "The model answer needs correction; accepted items are saved. Submit a corrected reply or explicitly retry the review stage.",
                ));
            }
            let result = match answer.take() {
                Some(answer) => answer?,
                None => self.backend.request(&profile, task)?,
            };
            self.job.pending.as_mut().unwrap().attempts += 1;
            if let Err(error) = self.apply_task_response(result, false) {
                if error.code != "invalid_input" {
                    return Err(error);
                }
                self.job.pending.as_mut().unwrap().errors = vec![json!({"error":error.message})];
                self.backend.save(&self.job)?;
            }
        }
    }
    pub fn submit(&mut self, raw: &str) -> Result<()> {
        let value: Value = blitzline_domain::parse_reply(raw).map_err(|e| Error::input(e.0))?;
        self.apply_task_response(value, true)?;
        self.backend.save(&self.job)
    }
    fn apply_task_response(&mut self, mut value: Value, envelope: bool) -> Result<()> {
        let task = self
            .job
            .pending
            .clone()
            .ok_or_else(|| Error::input("This run has no pending exchange"))?;
        if envelope
            && (value["exchange_id"] != task.id
                || value["revision"].as_u64() != Some(task.revision))
        {
            return Err(Error::input(
                "This reply belongs to an old or different exchange. Copy the current request and try again.",
            ));
        }
        if let Some(m) = value.as_object_mut() {
            m.remove("exchange_id");
            m.remove("revision");
        }
        let root = match task.stage.as_str() {
            "review" | "ambiguity_review" => "decisions",
            "audit_review" => "corrections",
            _ => "cues",
        };
        if !value
            .as_object()
            .is_some_and(|object| object.len() == 1 && object.contains_key(root))
        {
            return Err(Error::input(format!("Response must contain only {root}")));
        }
        let mut accepted = task.accepted.clone();
        let mut decisions = self.job.decisions.clone();
        let mut errors = Vec::new();
        if task.stage == "review" || task.stage == "ambiguity_review" {
            let all: Vec<Candidate> = serde_json::from_value(task.data["candidates"].clone())?;
            let expected = all
                .into_iter()
                .filter(|c| task.requested.contains(&c.id))
                .collect::<Vec<_>>();
            let rows = value["decisions"]
                .as_array()
                .ok_or_else(|| Error::input("Reply requires a decisions array"))?;
            let mut seen = BTreeSet::new();
            if rows.iter().any(|r| {
                r["candidate_id"]
                    .as_str()
                    .is_none_or(|id| !task.requested.iter().any(|s| s == id) || !seen.insert(id))
            }) {
                return Err(Error::input(
                    "Duplicate or unknown candidate IDs; no answers were changed",
                ));
            }
            let (valid, failures) = vocabulary::validate_decisions(&value, &expected);
            errors = failures;
            for d in valid {
                let row = serde_json::to_value(&d)?;
                if accepted.get(&d.candidate_id).is_some_and(|v| *v != row) {
                    return Err(Error::input(
                        "An accepted answer was changed in an old reply. Edit the word explicitly instead.",
                    ));
                }
                accepted.insert(d.candidate_id.clone(), row);
                decisions.insert(d.candidate_id.clone(), d);
            }
        } else if task.stage == "audit_review" {
            let patches = value["corrections"]
                .as_array()
                .ok_or_else(|| Error::input("Audit response needs corrections"))?;
            let candidates: Vec<Candidate> =
                serde_json::from_value(task.data["original_candidates"].clone())?;
            let mut seen = BTreeSet::new();
            let mut replacements = Vec::new();
            for patch in patches {
                let fields = [
                    "index",
                    "decision",
                    "reason",
                    "english",
                    "context_indices",
                    "notes",
                ];
                if !patch.as_object().is_some_and(|object| {
                    object.len() == fields.len()
                        && object.keys().all(|key| fields.contains(&key.as_str()))
                }) {
                    return Err(Error::input("Invalid audit correction fields"));
                }
                let index = patch["index"]
                    .as_u64()
                    .ok_or_else(|| Error::input("Invalid audit index"))?
                    as usize;
                if !seen.insert(index) {
                    return Err(Error::input("Duplicate audit index"));
                }
                let c = candidates
                    .get(index)
                    .ok_or_else(|| Error::input("Unknown audit candidate"))?;
                let ids = patch["context_indices"]
                    .as_array()
                    .ok_or_else(|| Error::input("Missing audit contexts"))?
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .and_then(|i| c.contexts.get(i as usize))
                            .map(|ctx| ctx.id.clone())
                            .ok_or_else(|| Error::input("Invalid audit example"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let d: Decision = serde_json::from_value(
                    json!({"candidate_id":c.id,"decision":patch["decision"],"english":patch["english"],"reason":patch["reason"],"notes":patch["notes"],"context_ids":ids}),
                )?;
                vocabulary::validate_decision(&d, c)?;
                replacements.push(d);
            }
            for d in replacements {
                decisions.insert(d.candidate_id.clone(), d);
            }
            accepted.insert("audit".into(), value);
        } else {
            let rows = value["cues"]
                .as_array()
                .ok_or_else(|| Error::input("Reply needs a cues array"))?;
            let mut seen = BTreeSet::new();
            let proof = task.stage == "proofread";
            for row in rows {
                let id = row["cue_id"]
                    .as_str()
                    .ok_or_else(|| Error::input("Missing cue ID"))?;
                if !task.requested.contains(&id.to_owned()) || !seen.insert(id) {
                    return Err(Error::input(
                        "Unknown or duplicate cue IDs; no changes were applied",
                    ));
                }
            }
            for row in rows {
                let id = row["cue_id"].as_str().unwrap();
                let fields = if proof {
                    vec!["cue_id", "text", "uncertain", "reason"]
                } else {
                    vec!["cue_id", "text"]
                };
                let valid = row.as_object().is_some_and(|r| {
                    r.len() == fields.len() && r.keys().all(|k| fields.contains(&k.as_str()))
                }) && row["text"]
                    .as_str()
                    .is_some_and(|s| nonblank(s, "Cue text").is_ok())
                    && (!proof
                        || (row["uncertain"].is_boolean()
                            && row["reason"].is_string()
                            && (row["uncertain"] != true
                                || row["reason"]
                                    .as_str()
                                    .is_some_and(|reason| !reason.trim().is_empty()))));
                if !valid {
                    errors.push(json!({"cue_id":id,"error":"Invalid cue text or fields"}));
                    continue;
                }
                if proof && row["uncertain"] == true {
                    errors.push(json!({"cue_id":id,"uncertain":true,"error":"The correction is uncertain. Review the source and submit a confirmed correction."}));
                    continue;
                }
                if accepted.get(id).is_some_and(|v| v != row) {
                    return Err(Error::input(
                        "An already accepted cue was changed; start a fresh text revision instead",
                    ));
                }
                accepted.insert(id.into(), row.clone());
            }
        }
        self.job.decisions = decisions;
        errors.retain(|error| {
            error["candidate_id"]
                .as_str()
                .is_none_or(|id| !accepted.contains_key(id))
        });
        let t = self.job.pending.as_mut().unwrap();
        t.accepted = accepted;
        t.errors = errors;
        t.draft.clear();
        self.backend.save(&self.job)
    }
    fn cue_review(&mut self, cues: &[Cue], stage: &str) -> Result<Vec<Cue>> {
        let size = self.job.settings["llm"]["batch_size"]
            .as_u64()
            .unwrap_or(20) as usize;
        let max = self.job.settings["llm"]["max_chars"]
            .as_u64()
            .unwrap_or(24000) as usize;
        let proof = stage == "proofread";
        let mut props = json!({"cue_id":{"type":"string"},"text":{"type":"string"}});
        if proof {
            props["uncertain"] = json!({"type":"boolean"});
            props["reason"] = json!({"type":"string"});
        }
        let schema = vocabulary::object_schema(
            json!({"cues":{"type":"array","items":vocabulary::object_schema(props)}}),
        );
        let instruction = if proof {
            "Proofread clear errors, preserving the original language, meaning, and wording. Never alter IDs or timing. Mark uncertain with a reason when unsupported. Text documents have no audio; do not claim to listen."
        } else {
            "Translate each requested cue into English without summarizing, merging, omitting, or truncating. Return exactly the requested cue IDs. Neighbours provide context only."
        };
        let mut out = cues.to_vec();
        let mut batches = Vec::new();
        let mut current = Vec::new();
        for cue in cues {
            let row = json!({"cue_id":cue.id,"text":cue.text});
            if canonical(&row).chars().count() > max {
                return Err(Error::input("A cue exceeds llm.max_chars"));
            }
            let mut proposed = current.clone();
            proposed.push(row.clone());
            if !current.is_empty()
                && (current.len() >= size || canonical(&proposed).chars().count() > max)
            {
                batches.push(std::mem::take(&mut current));
            }
            current.push(row);
        }
        if !current.is_empty() {
            batches.push(current);
        }
        let mut requests = Vec::new();
        for (i, chunk) in batches.into_iter().enumerate() {
            let requested = chunk
                .iter()
                .map(|r| r["cue_id"].as_str().unwrap().to_owned())
                .collect();
            requests.push(Batch {
                sequence: i,
                instruction,
                data: json!({"language":self.job.request.language,"requested":chunk,"neighbours":cues.iter().enumerate().filter(|(index,_)| { let first=cues.iter().position(|c| chunk[0]["cue_id"]==c.id).unwrap_or(0); *index>=first.saturating_sub(1)&&*index<=first+chunk.len() }).map(|(_,c)|json!({"cue_id":c.id,"text":c.text})).collect::<Vec<_>>()}),
                schema: schema.clone(),
                requested,
            });
        }
        for value in self.run_batches(stage, requests)? {
            for row in value["cues"].as_array().unwrap() {
                if let Some(c) = out.iter_mut().find(|c| row["cue_id"] == c.id) {
                    let t = row["text"].as_str().unwrap();
                    if c.text != t {
                        c.words.clear();
                        c.text = t.into();
                    }
                }
            }
        }
        Ok(out)
    }
    fn vocabulary_review(&mut self, candidates: &[Candidate]) -> Result<()> {
        let forms = self.backend.forms(&self.job.settings, candidates)?;
        let llm = self.job.settings["llm"].clone();
        let size = llm["batch_size"].as_u64().unwrap_or(20) as usize;
        let max = llm["max_chars"].as_u64().unwrap_or(24000) as usize;
        let groups = vocabulary::groups(candidates, &forms);
        let remaining = groups
            .iter()
            .map(|g| {
                g.iter()
                    .filter(|c| {
                        self.job
                            .decisions
                            .get(&c.id)
                            .is_none_or(|d| d.decision == "needs-review")
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect::<Vec<_>>();
        // Retain the complete related group as evidence even when repairing one item.
        let mut requests = Vec::new();
        for (i, chunk) in vocabulary::batches(remaining, size, max)?
            .into_iter()
            .enumerate()
        {
            let ids = chunk.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
            let evidence = groups
                .iter()
                .filter(|g| g.iter().any(|c| ids.contains(&c.id)))
                .flatten()
                .cloned()
                .collect::<Vec<_>>();
            requests.push(Batch {
                sequence: i,
                instruction: "Review unknown lemma candidates as a language educator. Check every highlighted occurrence in its full original sentence against this exact lemma and part of speech. Exclude names, duplicates, and false matches. Keep valid unknown dictionary forms, with one or two owned context IDs and an English meaning supported by those examples. Never invent terms, sentences, counts, IDs, or timing. Notes are useful learner notes, otherwise empty. When evidence is insufficient use needs-review with a reason.",
                data: json!({"language":self.job.request.language,"requested_ids":ids,"candidates":evidence}),
                schema: vocabulary::review_schema(),
                requested: ids,
            });
        }
        self.run_batches("review", requests)?;
        if llm["ambiguity_review"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            let groups = groups.into_iter().filter(|g| g.len() > 1).collect();
            let mut requests = Vec::new();
            for (i, chunk) in vocabulary::batches(
                groups,
                llm["ambiguity_batch_size"].as_u64().unwrap_or(5) as usize,
                max,
            )?
            .into_iter()
            .enumerate()
            {
                let ids = chunk.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
                requests.push(Batch {
                    sequence: i,
                    instruction: "Independently recheck every candidate in this competing lemma group, including rejected candidates. First-pass decisions are untrusted. Check exact word forms and parts of speech in each full sentence; return complete corrected decisions without changing source evidence.",
                    data: json!({"language":self.job.request.language,"requested_ids":ids,"candidates":chunk,"first_pass_decisions":self.job.decisions.values().filter(|decision|ids.contains(&decision.candidate_id)).collect::<Vec<_>>()}),
                    schema: vocabulary::review_schema(),
                    requested: ids,
                });
            }
            self.run_batches("ambiguity_review", requests)?;
        }
        if llm["audit_review"].as_str().is_some_and(|s| !s.is_empty()) && !candidates.is_empty() {
            let item = vocabulary::object_schema(
                json!({"index":{"type":"integer","minimum":0},"decision":{"type":"string","enum":["keep","reject","needs-review"]},"reason":{"type":"string"},"english":{"type":"string"},"context_indices":{"type":"array","items":{"type":"integer","minimum":0},"maxItems":2},"notes":{"type":"string"}}),
            );
            let batches = vocabulary::batches(vocabulary::groups(candidates, &forms), size, max)?;
            let mut requests = Vec::new();
            for (sequence, chunk) in batches.into_iter().enumerate() {
                let audit = chunk.iter().enumerate().map(|(i,c)|json!({"index":i,"term":c.term,"current":self.job.decisions.get(&c.id),"examples":c.contexts})).collect::<Vec<_>>();
                requests.push(Batch {
                    sequence,
                    instruction: "Independently audit EVERY indexed candidate and highlighted occurrence. Return only wrong decisions as complete replacements with candidate index and zero-based context_indices. Omit unchanged candidates. Do not alter source terms, examples, frequency, or timing. Return corrections even when empty.",
                    data: json!({"language":self.job.request.language,"candidate_count":chunk.len(),"candidates":audit,"original_candidates":chunk}),
                    schema: vocabulary::object_schema(json!({"corrections":{"type":"array","items":item}})),
                    requested: vec!["audit".into()],
                });
            }
            self.run_batches("audit_review", requests)?;
        }
        Ok(())
    }
    pub fn import_review(&mut self, raw: &str) -> Result<()> {
        let candidates: Vec<Candidate> = self.load("artifacts/04-vocabulary.json")?;
        let value: Value = blitzline_domain::parse_reply(raw).map_err(|e| Error::input(e.0))?;
        if !value.as_object().is_some_and(|object| {
            object.len() == 1 && object.get("decisions").is_some_and(Value::is_array)
        }) {
            return Err(Error::input(
                "Review file must contain only a decisions array",
            ));
        }
        let (valid, errors) = vocabulary::validate_decisions(&value, &candidates);
        // A review file is an explicit edit. Reject invalid edits atomically,
        // rather than retaining an older answer behind an invalid replacement.
        let supplied = value["decisions"].as_array().map_or(0, Vec::len);
        if valid.len() != supplied || errors.iter().any(|e| e["candidate_id"] == "") {
            return Err(Error::input(
                "Invalid review decisions; no answers were changed",
            ));
        }
        for d in valid {
            self.job.decisions.insert(d.candidate_id.clone(), d);
        }
        self.job.stages.retain(|n, _| n.as_str() < "05-review");
        self.job.pending = None;
        self.job.revision += 1;
        self.backend.save(&self.job)
    }
    pub fn export_current(&mut self) -> Result<Vec<String>> {
        if ["cleaned", "cleaning"].contains(&self.job.state.as_str()) {
            return Err(Error::input("This run was cleaned or is being cleaned"));
        }
        for (path, hash) in &self.job.input_hashes {
            if self.backend.external_hash(path)? != *hash {
                return Err(Error::operation(format!(
                    "Input file changed: {path}. Resume and review before exporting."
                )));
            }
        }
        for stage in [
            "01-transcript",
            "02-proofread",
            "03-translate",
            "04-vocabulary",
            "05-review",
            "06-media",
        ] {
            let receipt = self
                .job
                .stages
                .get(stage)
                .ok_or_else(|| Error::input(format!("Complete {stage} before exporting")))?;
            if receipt.state != "complete"
                || receipt
                    .outputs
                    .iter()
                    .any(|(p, h)| !self.backend.fingerprint(p).is_ok_and(|v| v == *h))
            {
                return Err(Error::operation(format!(
                    "{stage} is incomplete or changed. Resume before exporting."
                )));
            }
        }
        let data: Value = self.load("artifacts/06-media.json")?;
        let cards: Vec<Card> = serde_json::from_value(data["cards"].clone())?;
        if self.job.omitted > 0
            && !self.job.settings["run"]["allow_partial"]
                .as_bool()
                .unwrap_or(false)
        {
            return Err(Error::review(
                "Unresolved candidates or clips block export; review them or explicitly choose --allow-partial",
            ));
        }
        let settings = self
            .backend
            .export_settings(&self.job.settings, &self.job.request.language)?;
        let kind = settings["format"].as_str().unwrap_or("tsv").to_owned();
        let paths = self.backend.export(&cards, &settings)?;
        let mut outputs = BTreeMap::new();
        for path in &paths {
            outputs.insert(path.clone(), self.backend.fingerprint(path)?);
        }
        self.job.exports.insert(kind, outputs);
        self.backend.save(&self.job)?;
        Ok(paths)
    }
}
/// One request of a stage, collected before any of them is sent.
struct Batch {
    sequence: usize,
    instruction: &'static str,
    data: Value,
    schema: Value,
    requested: Vec<String>,
}
fn task_id(revision: u64, stage: &str, sequence: usize, requested: &[String]) -> String {
    format!(
        "exchange-{}",
        &digest(&json!([revision, stage, sequence, requested]))[..24]
    )
}
fn new_task(id: String, revision: u64, stage: &str, batch: &Batch) -> Task {
    Task {
        id,
        revision,
        stage: stage.into(),
        sequence: batch.sequence,
        instruction: batch.instruction.into(),
        data: batch.data.clone(),
        schema: batch.schema.clone(),
        requested: batch.requested.clone(),
        accepted: BTreeMap::new(),
        draft: String::new(),
        errors: vec![],
        attempts: 0,
    }
}
fn task_value(task: &Task) -> Value {
    if task.stage == "review" || task.stage == "ambiguity_review" {
        json!({"decisions":task.requested.iter().filter_map(|id|task.accepted.get(id)).collect::<Vec<_>>()})
    } else if task.stage == "audit_review" {
        task.accepted
            .get("audit")
            .cloned()
            .unwrap_or_else(|| json!({"corrections":[]}))
    } else {
        json!({"cues":task.requested.iter().filter_map(|id|task.accepted.get(id)).collect::<Vec<_>>()})
    }
}
#[cfg(test)]
mod parallel_tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
        time::Duration,
    };
    /// Answers every translate request with "T-<cue id>"; only `request` and the bookkeeping
    /// calls a cue stage makes are real. Earlier cues answer slower, so completions arrive reversed.
    #[derive(Default)]
    struct Fake {
        calls: AtomicUsize,
        running: AtomicUsize,
        peak: AtomicUsize,
        fail_on: Option<&'static str>,
        cancel_after_first: bool,
        cancelled: AtomicBool,
    }
    impl Backend for Fake {
        fn write(&self, _: &str, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn save(&self, _: &Job) -> Result<()> {
            Ok(())
        }
        fn cancelled(&self) -> bool {
            self.cancelled.load(SeqCst)
        }
        fn request(&self, _: &Value, task: &Task) -> Result<Value> {
            self.calls.fetch_add(1, SeqCst);
            let now = self.running.fetch_add(1, SeqCst) + 1;
            self.peak.fetch_max(now, SeqCst);
            let id = task.requested[0].clone();
            let n: u64 = id.trim_start_matches("c").parse().unwrap();
            std::thread::sleep(Duration::from_millis(60 - n * 8));
            self.running.fetch_sub(1, SeqCst);
            if self.cancel_after_first {
                self.cancelled.store(true, SeqCst);
            }
            if self.fail_on == Some(id.as_str()) {
                return Err(Error::operation("boom"));
            }
            Ok(json!({"cues":[{"cue_id":id,"text":format!("T-{id}")}]}))
        }
        fn read(&self, _: &str) -> Result<Vec<u8>> {
            unimplemented!()
        }
        fn fingerprint(&self, _: &str) -> Result<String> {
            unimplemented!()
        }
        fn external(&self, _: &str) -> Result<Vec<u8>> {
            unimplemented!()
        }
        fn external_hash(&self, _: &str) -> Result<String> {
            unimplemented!()
        }
        fn dependencies(&self, _: &str, _: &Job) -> Result<Value> {
            unimplemented!()
        }
        fn probe(&self, _: &Request, _: &Value) -> Result<MediaInfo> {
            unimplemented!()
        }
        fn transcribe(&self, _: &Request, _: &Value, _: &MediaInfo) -> Result<Value> {
            unimplemented!()
        }
        fn relisten(&self, _: &Request, _: &Value, _: &Cue) -> Result<String> {
            unimplemented!()
        }
        fn extract(&self, _: &Request, _: &Value) -> Result<Value> {
            unimplemented!()
        }
        fn forms(&self, _: &Value, _: &[Candidate]) -> Result<BTreeMap<String, Vec<String>>> {
            unimplemented!()
        }
        fn clips(
            &self,
            _: &[Card],
            _: &Request,
            _: &MediaInfo,
            _: &Value,
        ) -> Result<(Vec<Card>, Vec<Value>)> {
            unimplemented!()
        }
        fn export(&self, _: &[Card], _: &Value) -> Result<Vec<String>> {
            unimplemented!()
        }
        fn export_settings(&self, _: &Value, _: &str) -> Result<Value> {
            unimplemented!()
        }
        fn progress(&self, _: &str, _: &str) {}
    }
    fn pipeline(backend: Fake, parallel: u64) -> Pipeline<Fake> {
        let job = Job {
            implementation: String::new(),
            schema_version: 1,
            created: 0,
            revision: 1,
            request: Request {
                source: String::new(),
                source_kind: "text".into(),
                source_hash: String::new(),
                language: "sk".into(),
                srt: None,
                asr_json: None,
                text_file: None,
                vocabulary_json: None,
            },
            settings: json!({
                "run":{"method":"api"},
                "llm":{"default":"p","batch_size":1,"max_chars":24000,"repair_attempts":2},
                "profiles":{"p":{"parallel":parallel}},
            }),
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
        Pipeline { backend, job }
    }
    fn cues(n: usize) -> Vec<Cue> {
        (0..n)
            .map(|i| Cue {
                id: format!("c{i}"),
                start_ms: None,
                end_ms: None,
                text: format!("s{i}"),
                words: vec![],
            })
            .collect()
    }
    fn texts(cues: &[Cue]) -> Vec<&str> {
        cues.iter().map(|c| c.text.as_str()).collect()
    }
    #[test]
    fn parallel_requests_overlap_and_apply_in_batch_order() {
        let mut p = pipeline(Fake::default(), 3);
        let out = p.cue_review(&cues(6), "translate").unwrap();
        assert_eq!(
            texts(&out),
            ["T-c0", "T-c1", "T-c2", "T-c3", "T-c4", "T-c5"]
        );
        assert_eq!(p.backend.calls.load(SeqCst), 6);
        assert!(p.backend.peak.load(SeqCst) > 1);
        let history = p.job.history.iter().map(|t| t.sequence).collect::<Vec<_>>();
        assert_eq!(history, [0, 1, 2, 3, 4, 5]);
    }
    #[test]
    fn parallel_one_sends_one_request_at_a_time() {
        let mut p = pipeline(Fake::default(), 1);
        let out = p.cue_review(&cues(4), "translate").unwrap();
        assert_eq!(texts(&out), ["T-c0", "T-c1", "T-c2", "T-c3"]);
        assert_eq!(p.backend.calls.load(SeqCst), 4);
        assert_eq!(p.backend.peak.load(SeqCst), 1);
    }
    #[test]
    fn a_failed_request_stops_at_its_batch_and_keeps_earlier_ones() {
        let backend = Fake {
            fail_on: Some("c3"),
            ..Fake::default()
        };
        let mut p = pipeline(backend, 4);
        let error = p.cue_review(&cues(6), "translate").unwrap_err();
        assert_eq!(error.message, "boom");
        assert_eq!(p.job.history.len(), 3);
        assert_eq!(p.job.history.last().unwrap().requested, ["c2"]);
    }
    #[test]
    fn cancelling_stops_launching_requests() {
        let backend = Fake {
            cancel_after_first: true,
            ..Fake::default()
        };
        let mut p = pipeline(backend, 2);
        let error = p.cue_review(&cues(8), "translate").unwrap_err();
        assert_eq!(error.code, "paused");
        assert!(p.backend.calls.load(SeqCst) <= 2);
        assert!(p.job.history.is_empty());
    }
}
