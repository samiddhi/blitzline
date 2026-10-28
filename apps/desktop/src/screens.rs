use super::*;
use std::path::Path;

impl Desktop {
    pub(super) fn add_sources(&mut self, paths: &[std::path::PathBuf]) {
        self.source_import_errors.clear();
        let mut added = 0;
        for path in paths {
            match blitzline_adapters::load_source(path) {
                Ok(source) => {
                    self.engine.state.setup.sources.push(source);
                    added += 1;
                }
                Err(error) => {
                    self.source_import_errors
                        .push(format!("{}: {}", path.display(), error))
                }
            }
        }
        self.error = self.source_import_errors.join("\n");
        self.notice = format!(
            "{added} file{} added.{}",
            if added == 1 { "" } else { "s" },
            if self.source_import_errors.is_empty() {
                String::new()
            } else {
                format!(
                    " {} could not be added; see the import report.",
                    self.source_import_errors.len()
                )
            }
        );
        self.engine.state.setup.step = 0;
        self.ui.page = Page::Setup;
    }
    pub(super) fn save_setup_for_later(&mut self) {
        if self.save_now() {
            self.notice = "Project setup saved. Choose Continue saved setup on the Projects page to return to it.".into();
            self.ui.page = Page::Projects;
        }
    }
    pub(super) fn pick_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Choose recordings or text files")
            .pick_files()
        {
            self.add_sources(&paths);
        }
    }
    pub(super) fn projects(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "Your projects",
            "Turn something you enjoy into something you remember.",
        );
        if !self.engine.state.setup.sources.is_empty()
            || !self.engine.state.setup.pasted_text.trim().is_empty()
        {
            card(ui, |ui| {
                ui.heading("Saved project setup");
                let setup = &self.engine.state.setup;
                let count = setup.sources.len() + usize::from(!setup.pasted_text.trim().is_empty());
                metadata(
                    ui,
                    format!(
                        "{count} source{} · {} · {}",
                        if count == 1 { "" } else { "s" },
                        setup.language,
                        setup.deck
                    ),
                );
                ui.label("Your material and choices are saved. Processing starts only when you choose Start project.");
                if primary(ui, "Continue saved setup").clicked() {
                    self.ui.page = Page::Setup;
                }
            });
        }
        if self.engine.state.projects.is_empty() {
            card(ui, |ui| {
                ui.heading("A recording. A story. Your next words.");
                ui.label("Bring some text or a recording. Blitzline guides you from source to study cards.");
                ui.horizontal_wrapped(|ui| {
                    if primary(ui, "Start a project").clicked() {
                        self.ui.page = Page::Setup;
                    }
                    if ui.button("Try an example").clicked() {
                        match self.engine.example() {
                            Ok(id) => self.open_project(id),
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                    if ui.button("Import existing settings").clicked() {
                        self.import_settings();
                    }
                });
                ui.small("The example uses sample answers. No account or API key is needed.");
            });
            return;
        }
        ui.add(
            text_input(&mut self.ui.search)
                .hint_text("Find a project")
                .desired_width(ui.available_width()),
        );
        ui.horizontal_wrapped(|ui| {
            for filter in [
                "All",
                "Needs attention",
                "Processing",
                "Finished",
                "Archived",
            ] {
                ui.selectable_value(&mut self.ui.project_filter, filter.into(), filter);
            }
        });
        ui.add_space(12.0);
        let mut count = 0;
        for p in self.engine.state.projects.clone().into_iter().rev() {
            if !format!("{} {} {}", p.name, p.source.language, p.deck)
                .to_lowercase()
                .contains(&self.ui.search.to_lowercase())
            {
                continue;
            }
            if p.archived != (self.ui.project_filter == "Archived") {
                continue;
            }
            let visible = match self.ui.project_filter.as_str() {
                "Needs attention" => matches!(
                    p.status,
                    Status::Waiting | Status::Failed | Status::Paused | Status::Ready
                ),
                "Processing" => matches!(p.status, Status::Running | Status::Queued),
                "Finished" => p.status == Status::Finished,
                _ => true,
            };
            if !visible {
                continue;
            }
            count += 1;
            card(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&p.name).size(20.0).strong());
                    ui.label(RichText::new(p.status.label()).color(accent(ui)));
                });
                ui.label(format!(
                    "{}  ·  {}  ·  {}",
                    if p.source.media { "Recording" } else { "Text" },
                    p.source.language,
                    p.deck
                ));
                ui.horizontal_wrapped(|ui| {
                    if primary(
                        ui,
                        if p.status == Status::Waiting {
                            "Continue review"
                        } else {
                            "Open project"
                        },
                    )
                    .clicked()
                    {
                        self.open_project(p.id);
                    }
                    if p.archived {
                        if ui.button("Restore").clicked() {
                            self.engine.project_mut(p.id).unwrap().archived = false;
                        }
                    } else if ui.button("Archive").clicked() {
                        self.run(|e| e.pause(p.id));
                        self.engine.project_mut(p.id).unwrap().archived = true;
                    }
                    if ui.button("Delete…").clicked() {
                        self.confirm_delete = Some(p.id);
                    }
                });
            });
            ui.add_space(8.0);
        }
        if count == 0 {
            ui.label("No projects match this view.");
        }
    }
    pub(super) fn setup(&mut self, ui: &mut egui::Ui) {
        let step = self.engine.state.setup.step.min(3);
        title(
            ui,
            "Let's make some study cards",
            "Your setup is saved as you go.",
        );
        ui.horizontal_wrapped(|ui| {
            for (i, label) in [
                "1  Material",
                "2  Language & deck",
                "3  Your help",
                "4  Check & start",
            ]
            .iter()
            .enumerate()
            {
                ui.label(if i == step {
                    RichText::new(*label).strong().color(accent(ui))
                } else {
                    RichText::new(*label).weak()
                });
                if i < 3 {
                    ui.label("›");
                }
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| match step {
            0 => {
                setup_heading(
                    ui,
                    "What would you like to learn from?",
                    "Choose text, recordings, or both. Each file becomes its own project. You can also paste text. Recordings have no file-size limit; originals stay unchanged.",
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Choose files…").clicked() {
                        self.pick_files();
                    }
                    ui.label("or drop recordings and text files here");
                });
                if !self.source_import_errors.is_empty() {
                    ui.strong("Some files could not be added");
                    for error in &self.source_import_errors {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    ui.separator();
                }
                metadata(
                    ui,
                    "Choose several recordings and text files together. Recordings have no file-size limit.",
                );
                let sources = self.engine.state.setup.sources.clone();
                for (i, source) in sources.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} · {}",
                            source.name,
                            if source.media { "recording" } else { "text" }
                        ));
                        if ui.button("Remove").clicked() {
                            self.engine.state.setup.sources.remove(i);
                        }
                    });
                }
                ui.separator();
                let paste_label = ui.label("Or paste text");
                ui.add(
                    text_input(&mut self.engine.state.setup.title)
                        .hint_text("Give it a title (optional)")
                        .desired_width(f32::INFINITY),
                );
                ui.add(
                    text_area(&mut self.engine.state.setup.pasted_text)
                        .hint_text("Paste an article, story, or a few sentences…")
                        .desired_rows(7)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(paste_label.id);
                ui.collapsing("Already have subtitles?", |ui| {
                    ui.label("Attach an SRT or transcription JSON to each recording.");
                    for i in 0..self.engine.state.setup.sources.len() {
                        if !self.engine.state.setup.sources[i].media {
                            continue;
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(&self.engine.state.setup.sources[i].name);
                            if ui.button("Attach…").clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Subtitles", &["srt", "json"])
                                    .pick_file()
                            {
                                self.engine.state.setup.sources[i].subtitles =
                                    Some(path.to_string_lossy().into());
                            }
                            if let Some(path) = &self.engine.state.setup.sources[i].subtitles {
                                ui.small(path);
                            }
                        });
                    }
                });
                ui.small(
                    "Several files become separate projects. Your originals are kept unchanged.",
                );
            }
            1 => {
                setup_heading(
                    ui,
                    "Language and destination",
                    "Choose the language of your material and the Anki deck where your cards should go. A saved profile is an optional reusable set of language, deck, review preferences, and word lists. Choose No saved profile to configure this project yourself.",
                );
                let profiles = self.engine.state.profiles.clone();
                egui::ComboBox::from_label("Use a saved profile")
                    .selected_text(
                        if self.engine.state.setup.settings.profile_name.is_empty() {
                            "No saved profile"
                        } else {
                            &self.engine.state.setup.settings.profile_name
                        },
                    )
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(self.engine.state.setup.settings.profile_name.is_empty(), "No saved profile").on_hover_text("Keep your current language, deck, and options as manual choices. No profile word lists will be applied.").clicked() {
                            self.engine.state.setup.settings.profile_name.clear();
                        }
                        for profile in profiles {
                            if ui.selectable_label(profile.name == self.engine.state.setup.settings.profile_name, &profile.name).clicked() {
                                let setup = &mut self.engine.state.setup;
                                setup.language = profile.language;
                                setup.deck = profile.deck;
                                setup.settings = profile.settings;
                                setup.settings.profile_name = profile.name;
                            }
                        }
                    });
                ui.label("What language is this?");
                language_picker(ui, &mut self.engine.state.setup.language, "setup_language");
                ui.label("Destination deck");
                ui.add(text_input(&mut self.engine.state.setup.deck).desired_width(f32::INFINITY));
                if self.engine.state.setup.sources.len() > 1 {
                    ui.collapsing("Different languages for individual files", |ui| {
                        for (i, s) in self.engine.state.setup.sources.iter_mut().enumerate() {
                            ui.label(&s.name);
                            language_picker(ui, &mut s.language, ("source_language", i));
                        }
                    });
                }
                if self.engine.state.setup.sources.iter().any(|s| s.media) {
                    ui.separator();
                    ui.label("Make a transcript");
                    ui.radio_value(
                        &mut self.engine.state.setup.settings.local_transcription,
                        true,
                        local_engine_label(&self.engine.state.preferences),
                    );
                    ui.radio_value(
                        &mut self.engine.state.setup.settings.local_transcription,
                        false,
                        "Online service",
                    );
                    ui.small("Supplied subtitles bypass transcription. Online transcription sends audio to your configured service.");
                } else {
                    ui.label("Text needs no transcription tools or model download.");
                }
            }
            2 => {
                setup_heading(
                    ui,
                    "How would you like help?",
                    "Use a chat website to copy and paste requests and answers without an API key. Connect an AI service for automatic requests that may incur charges, or review words yourself. Online transcription is a separate choice for recordings.",
                );
                for (method, detail) in [
                    (
                        Method::Browser,
                        "Copy a prepared request into your chat website, then bring the reply back. No API key.",
                    ),
                    (
                        Method::Api,
                        "Let Blitzline send requests automatically through a connection.",
                    ),
                    (
                        Method::Manual,
                        "Enter meanings and choose what to keep yourself.",
                    ),
                ] {
                    let selected = self.engine.state.setup.method_chosen
                        && self.engine.state.setup.settings.method == method;
                    let mut chosen = false;
                    let frame = egui::Frame::new()
                        .fill(if selected {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().window_fill
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if selected {
                                accent(ui)
                            } else {
                                ui.visuals().widgets.inactive.bg_stroke.color
                            },
                        ))
                        .corner_radius(10)
                        .inner_margin(14)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let width = (ui.available_width() * 0.35).clamp(180.0, 260.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(width, 34.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(width);
                                        chosen |= ui
                                            .radio(selected, RichText::new(method.label()).strong())
                                            .clicked();
                                    },
                                );
                                chosen |= ui
                                    .add(
                                        egui::Label::new(RichText::new(detail).weak())
                                            .wrap()
                                            .sense(egui::Sense::click()),
                                    )
                                    .clicked();
                            });
                        });
                    chosen |= frame
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                    if chosen {
                        self.engine.state.setup.method_chosen = true;
                        self.engine.state.setup.settings.method = method;
                    }
                }
                ui.small("A browser chat may offer free access. Its own limits still apply.");
                ui.separator();
                ui.strong("Optional help");
                checkbox(
                    ui,
                    &mut self.engine.state.setup.settings.proofread,
                    "Proofread the source text",
                );
                checkbox(
                    ui,
                    &mut self.engine.state.setup.settings.translate,
                    "Create an English translation",
                );
                if self.engine.state.setup.settings.method == Method::Api
                    && ui.button("Configure an AI connection").clicked()
                {
                    self.settings_open = true;
                    self.ui.settings_category = "AI connections".into();
                }
                ui.collapsing("Advanced settings", |ui| {
                    stage_methods(ui, &mut self.engine.state.setup.settings);
                });
            }
            _ => {
                setup_heading(
                    ui,
                    "Ready when you are",
                    "Check the sources, language, destination, and processing choices before starting. Start project begins processing. Save for later keeps your setup without contacting services, and adds a Continue saved setup card to Projects.",
                );
                let setup = &self.engine.state.setup;
                let n = setup.sources.len() + usize::from(!setup.pasted_text.trim().is_empty());
                metadata(
                    ui,
                    format!(
                        "{n} separate project(s)  ·  {}  ·  {}",
                        setup.language, setup.deck
                    ),
                );
                metadata(ui, format!("Help: {}", setup.settings.method.label()));
                metadata(
                    ui,
                    format!(
                        "Proofread: {}  ·  English translation: {}",
                        yes(setup.settings.proofread),
                        yes(setup.settings.translate)
                    ),
                );
                ui.separator();
                checkbox(
                    ui,
                    &mut self.engine.state.setup.settings.recall,
                    "Also practice recalling the word from its meaning",
                );
                if self.engine.state.setup.sources.iter().any(|s| s.media) {
                    checkbox(
                        ui,
                        &mut self.engine.state.setup.settings.video,
                        "Use video examples instead of audio",
                    );
                }
                ui.label("You will preview the cards before exporting.");
                ui.label("Browser assistance uses copy and paste. Automatic processing sends content to your configured service and may incur charges. Local transcription requires installed native tools and a model.");
                if self.engine.state.setup.settings.method == Method::Browser {
                    ui.label("The example has two vocabulary requests. Optional proofreading and translation each add one.");
                }
            }
        });
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            if step > 0 && ui.button("Back").clicked() {
                self.engine.state.setup.step -= 1;
            }
            if step < 3 {
                if primary(ui, "Continue").clicked() {
                    let s = &self.engine.state.setup;
                    let problem = match step {
                        0 if s.sources.is_empty() && s.pasted_text.trim().is_empty() => {
                            Some("Choose a file or paste some text.")
                        }
                        1 if s.language.trim().is_empty() || s.deck.trim().is_empty() => {
                            Some("Choose a language and name your deck.")
                        }
                        2 if !s.method_chosen => Some("Choose the kind of help you prefer."),
                        _ => None,
                    };
                    if let Some(problem) = problem {
                        self.error = problem.into();
                    } else {
                        self.error.clear();
                        self.engine.state.setup.step += 1;
                    }
                }
            } else if primary(ui, "Start project").clicked() {
                let setup = self.engine.state.setup.clone();
                match self.engine.create(&setup) {
                    Ok(ids) => {
                        self.start_native_projects(&ids);
                        self.engine.state.setup = Workspace::default().setup;
                        self.engine.state.setup.settings.batch_size =
                            self.engine.state.preferences.batch_size;
                        self.open_project(ids[0]);
                    }
                    Err(e) => self.error = e.to_string(),
                }
            }
            if ui.button("Save for later").clicked() {
                self.save_setup_for_later();
            }
        });
    }
    pub(super) fn project(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected() else {
            self.ui.page = Page::Projects;
            return;
        };
        let Ok(p) = self.engine.project(id).cloned() else {
            self.ui.page = Page::Projects;
            return;
        };
        if !p.is_example() && p.native_run.is_none() {
            title(
                ui,
                &p.name,
                "Saved preview · real processing has not started",
            );
            ui.label("This project was saved by an earlier preview. Its sample answers are not real results. Start a native project from the original material.");
            if primary(ui, "Start real project from this setup").clicked() {
                self.engine.state.setup = Setup {
                    sources: vec![p.source.clone()],
                    language: p.source.language.clone(),
                    deck: p.deck.clone(),
                    settings: p.settings.clone(),
                    method_chosen: true,
                    ..Default::default()
                };
                self.ui.page = Page::Setup;
            }
            return;
        }
        title(
            ui,
            &p.name,
            &format!(
                "{}  ·  {}  ·  {}",
                p.source.language,
                p.deck,
                p.status.label()
            ),
        );
        ui.horizontal_wrapped(|ui| {
            if matches!(p.status, Status::Paused | Status::Failed)
                && primary(
                    ui,
                    if p.status == Status::Failed {
                        "Retry"
                    } else {
                        "Continue processing"
                    },
                )
                .clicked()
            {
                self.run(|e| e.resume(id));
            }
            if matches!(p.status, Status::Queued | Status::Running | Status::Waiting)
                && ui.button("Pause").clicked()
            {
                self.run(|e| e.pause(id));
            }
            if ui.button("Project settings").clicked() {
                self.project_settings = Some(id);
                self.project_options = None;
                self.error.clear();
            }
            ui.menu_button("More", |ui| {
                if ui.button("Duplicate setup").clicked() {
                    self.engine.state.setup = Setup {
                        sources: vec![p.source.clone()],
                        language: p.source.language.clone(),
                        deck: p.deck.clone(),
                        settings: p.settings.clone(),
                        method_chosen: true,
                        ..Default::default()
                    };
                    self.ui.page = Page::Setup;
                    ui.close();
                }
                if ui.button("Archive").clicked() {
                    self.run(|e| e.pause(id));
                    self.engine.project_mut(id).unwrap().archived = true;
                    self.ui.page = Page::Projects;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        p.status == Status::Finished && !p.cleaned,
                        egui::Button::new("Clean up…"),
                    )
                    .clicked()
                {
                    self.confirm_cleanup = true;
                    ui.close();
                }
                if ui.button("Delete project…").clicked() {
                    self.confirm_delete = Some(id);
                    ui.close();
                }
            });
        });
        if let Some(error) = &p.error {
            card(ui, |ui| {
                ui.label(error);
                if ui.button("Tools & downloads").clicked() {
                    self.ui.page = Page::Resources;
                }
            });
        }
        if p.native_run.is_some() && self.worker.busy() {
            spinner(ui);
            ui.label("Processing in the background. You can pause or browse other projects.");
            return;
        }
        if p.cleaned {
            ui.label("Working data was cleaned. Export receipts remain below.");
            for r in &p.receipts {
                ui.label(&r.label);
            }
            return;
        }
        let stage = self.engine.state.selected_stage;
        ui.add_space(15.0);
        if stage.index() > p.stage.index() {
            card(ui, |ui| {
                ui.heading(stage.label());
                ui.label(format!(
                    "First finish {}. Your progress is saved.",
                    p.stage.label()
                ));
                if primary(ui, "Go to current step").clicked() {
                    self.engine.state.selected_stage = p.stage;
                }
            });
            return;
        }
        match stage {
            Stage::Source => {
                card(ui, |ui| {
                    ui.heading("Your material");
                    ui.label(&p.source.name);
                    if let Some(path) = &p.source.path {
                        ui.small(path);
                        if !Path::new(path).exists() {
                            ui.label("The original file has moved.");
                            if ui.button("Locate file…").clicked()
                                && let Some(found) = rfd::FileDialog::new().pick_file()
                            {
                                self.notice="Source relinking requires fingerprint verification in the real engine. The original reference has been kept.".into();
                                self.notice
                                    .push_str(&format!(" Selected: {}", found.display()));
                            }
                        }
                    }
                    ui.label(if p.source.media {
                        "Recording · native transcription or supplied subtitles"
                    } else {
                        "Text · no audio tools needed"
                    });
                    if p.is_example() && p.source.path.is_some() {
                        ui.label("This example uses demonstration text and vocabulary; it has not analyzed a personal file.");
                    }
                    if primary(ui, "Go to current step").clicked() {
                        self.engine.state.selected_stage = p.stage;
                    }
                });
            }
            Stage::Transcript => {
                if p.source.media {
                    self.player(ui, &p);
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Edit text").clicked() {
                        self.edit_text = true;
                    }
                    if ui.button("Import replacement…").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .add_filter("Text", &["txt"])
                            .pick_file()
                    {
                        match std::fs::read_to_string(path) {
                            Ok(t) => {
                                self.engine.project_mut(id).unwrap().transcript_draft = t;
                                self.edit_text = true;
                            }
                            Err(e) => self.error = e.to_string(),
                        }
                    }
                });
                if self.edit_text {
                    ui.add(
                        text_area(&mut self.engine.project_mut(id).unwrap().transcript_draft)
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if primary(ui, "Apply text changes…").clicked() {
                            self.confirm_text = true;
                        }
                        if ui.button("Discard changes").clicked() {
                            self.engine.project_mut(id).unwrap().transcript_draft =
                                p.transcript.clone();
                            self.edit_text = false;
                        }
                    });
                } else {
                    card(ui, |ui| {
                        ui.label(&p.transcript);
                    });
                }
                if p.exchange
                    .as_ref()
                    .is_some_and(|e| e.stage == Stage::Transcript)
                {
                    self.assistant_entry(ui, &p);
                }
            }
            Stage::Translation => {
                if !p.settings.translate {
                    card(ui, |ui| {
                        ui.heading("English translation is off");
                        ui.label("An optional translated document or subtitle file, separate from vocabulary meanings.");
                        if ui.button("Enable translation").clicked() {
                            if p.native_run.is_some() {
                                let mut settings = p.settings.clone();
                                settings.translate = true;
                                self.run(|engine| engine.update_settings(id, settings));
                            } else {
                                let current = self.engine.project_mut(id).unwrap();
                                current.settings.translate = true;
                                current.stage = Stage::Translation;
                                current.status = Status::Paused;
                                current.revision += 1;
                                current.exchange = None;
                                current.words.clear();
                                current.previewed_revision = None;
                            }
                        }
                    });
                } else {
                    card(ui, |ui| {
                        ui.strong("Original");
                        ui.label(&p.transcript);
                        ui.separator();
                        ui.strong("English");
                        if p.translation.is_empty() {
                            ui.label("Translation will appear here.");
                        } else {
                            ui.label(&p.translation);
                        }
                    });
                    if p.exchange
                        .as_ref()
                        .is_some_and(|e| e.stage == Stage::Translation)
                    {
                        self.assistant_entry(ui, &p);
                    }
                }
            }
            Stage::Words => self.words(ui, &p),
            Stage::Cards => self.cards(ui, &p),
            Stage::Export => self.exports(ui, &p),
        }
        ui.add_space(16.0);
        ui.collapsing("Processing details", |ui| {
            ui.small(format!(
                "Project {} · revision {} · {:?}",
                p.id, p.revision, p.status
            ));
            for event in &p.activity {
                ui.label(event);
            }
        });
    }
    fn assistant_entry(&mut self, ui: &mut egui::Ui, p: &Project) {
        if p.exchange
            .as_ref()
            .is_some_and(|e| p.settings.for_stage(e.stage) == Method::Manual)
        {
            self.assistant(ui, p);
            return;
        }
        section(ui, "Browser assistance");
        metadata(
            ui,
            "Continue in the chat assistant. Your pasted answers are saved.",
        );
        if primary(ui, "Open chat assistant").clicked() {
            self.compact = Some(p.id);
        }
        ui.separator();
    }
    pub(super) fn assistant(&mut self, ui: &mut egui::Ui, p: &Project) {
        let Some(exchange) = &p.exchange else {
            ui.label("This exchange is complete. Return to the project to continue.");
            return;
        };
        let id = p.id;
        if p.is_example() && p.settings.for_stage(exchange.stage) == Method::Manual {
            if exchange.stage == Stage::Words {
                return;
            }
            card(ui, |ui| {
                ui.heading(if exchange.stage == Stage::Transcript {
                    "Edit the source text"
                } else {
                    "Write an English translation"
                });
                let mut text = if exchange.draft.is_empty() {
                    if exchange.stage == Stage::Transcript {
                        p.transcript.clone()
                    } else {
                        String::new()
                    }
                } else {
                    exchange.draft.clone()
                };
                if ui
                    .add(
                        text_area(&mut text)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.engine
                        .project_mut(id)
                        .unwrap()
                        .exchange
                        .as_mut()
                        .unwrap()
                        .draft = text.clone();
                }
                if primary(ui, "Save and continue").clicked() {
                    let raw=serde_json::json!({"exchange_id":exchange.id,"revision":exchange.revision,"text":text}).to_string();
                    self.run(|e| {
                        e.submit(id, &raw)?;
                        e.next_exchange(id)
                    });
                }
            });
            return;
        }
        ui.add_space(12.0);
        ui.scope(|ui| {
            ui.heading("Chat assistant");
            ui.label(format!(
                "{} · Request {} · {} of {} answers saved",
                exchange.stage.label(),
                p.exchange_sequence,
                exchange.accepted.len(),
                exchange.requested.len()
            ));
            ui.small("Your progress is saved. You can come back later.");
            ui.add_space(8.0);
            if ui.available_width() >= 620.0 {
                ui.columns(2, |columns| {
                    columns[0].with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        self.assistant_send(ui, p);
                    });
                    columns[1].with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        self.assistant_receive(ui, p);
                    });
                });
            } else {
                self.assistant_send(ui, p);
                self.assistant_receive(ui, p);
            }
            ui.collapsing("Need a different way?",|ui|{
                if ui.button("The chat says this is too long").clicked(){self.run(|e|e.shorten_request(id));}
                if ui.button("Finish these items myself").clicked(){self.run(|e|e.change_method(id,Method::Manual));}
                if ui.button("Use automatic processing").clicked(){self.run(|e|e.change_method(id,Method::Api));}

                ui.label("At a service limit? You can pause, change websites, or return later. Accepted answers stay saved.");
            });
            if p.is_example() { ui.collapsing("Try a sample reply — example only",|ui|{
                ui.small("Use this to explore the flow without an account. Sample data is not extracted from your source.");
                if ui.button("Fill sample reply").clicked(){
                    match self.engine.sample_reply(id){Ok(raw)=>self.engine.project_mut(id).unwrap().exchange.as_mut().unwrap().draft=raw,Err(e)=>self.error=e.to_string()}
                }
                if ui.button("Fill incomplete reply").clicked()
                    && let Ok(raw)=self.engine.sample_reply(id){
                        let mut value:serde_json::Value=serde_json::from_str(&raw).unwrap();
                        if let Some(rows)=value["decisions"].as_array_mut(){rows.pop();}
                        self.engine.project_mut(id).unwrap().exchange.as_mut().unwrap().draft=serde_json::to_string_pretty(&value).unwrap();
                    }
            }); }
        });
    }
    fn assistant_send(&mut self, ui: &mut egui::Ui, p: &Project) {
        let id = p.id;
        let exchange = p.exchange.as_ref().unwrap();
        ui.strong("1  Copy the request");
        if primary(
            ui,
            if exchange.accepted.is_empty() {
                "Copy request"
            } else {
                "Copy a follow-up request"
            },
        )
        .clicked()
        {
            match self.engine.prompt(id) {
                Ok(prompt) => {
                    ui.ctx().copy_text(prompt);
                    self.notice = "Copied. Paste this request into your chat website.".into();
                }
                Err(e) => self.error = e.to_string(),
            }
        }
        ui.collapsing("See what you're sending", |ui| {
            if exchange.stage == Stage::Words {
                for w in p.words.iter().filter(|w| {
                    exchange.requested.contains(&w.id) && !exchange.accepted.contains(&w.id)
                }) {
                    ui.label(format!("{} — {}", w.term, w.example));
                }
            } else {
                ui.label(&p.transcript);
            }
            ui.collapsing("Raw prompt", |ui| {
                if let Ok(prompt) = self.engine.prompt(id) {
                    ui.label(RichText::new(prompt).monospace());
                }
            });
        });
        ui.strong("2  Send it in your browser");
        ui.label(
            "Paste into a new chat and wait for the complete answer. Then copy the whole answer.",
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open chat website").clicked() {
                self.open_website(ui.ctx());
            }
            if ui.button("Choose website").clicked() {
                self.settings_open = true;
                self.ui.settings_category = "Browser assistance".into();
            }
        });
    }
    fn assistant_receive(&mut self, ui: &mut egui::Ui, p: &Project) {
        let id = p.id;
        let exchange = p.exchange.as_ref().unwrap();
        ui.strong("3  Bring the answer back");
        let answer_label = ui.label("Paste the chat's answer here");
        let mut draft = exchange.draft.clone();
        let response = ui
            .add(
                text_area(&mut draft)
                    .id_salt(("reply", id))
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .hint_text("Paste the complete reply…"),
            )
            .labelled_by(answer_label.id);
        if response.changed() {
            self.engine
                .project_mut(id)
                .unwrap()
                .exchange
                .as_mut()
                .unwrap()
                .draft = draft.clone();
        }
        ui.horizontal_wrapped(|ui|{
                if ui.button("Paste from clipboard").clicked(){
                    match arboard::Clipboard::new().and_then(|mut c|c.get_text()){
                        Ok(text)=>self.engine.project_mut(id).unwrap().exchange.as_mut().unwrap().draft=text,
                        Err(_)=>self.error="Clipboard text is unavailable. Click the answer box and use your usual Paste shortcut.".into(),
                    }
                }
                if primary(ui,"Check answer").clicked(){self.run(|e|{e.submit(id,&draft)?;Ok(())});}
            });
        if !exchange.feedback.is_empty() {
            ui.label(RichText::new(&exchange.feedback).color(accent(ui)));
        }
        if exchange.requested.len() == exchange.accepted.len()
            && primary(ui, "Continue to the next step").clicked()
        {
            self.run(|e| e.next_exchange(id));
        }
    }
    fn words(&mut self, ui: &mut egui::Ui, p: &Project) {
        let id = p.id;
        if p.words.is_empty() {
            ui.label(if p.native_run.is_some() {
                "Vocabulary will appear here after extraction."
            } else {
                "Sample vocabulary is being prepared."
            });
            return;
        }
        if p.exchange.is_some() && p.settings.method == Method::Browser {
            self.assistant_entry(ui, p);
        }
        ui.add_space(14.0);
        ui.heading("Your words");
        if p.is_example() {
            ui.small("Demonstration vocabulary — not extracted from your file.");
        }
        ui.horizontal_wrapped(|ui| {
            for filter in ["All", "Needs attention", "Included", "Excluded"] {
                ui.selectable_value(&mut self.ui.word_filter, filter.into(), filter);
            }
        });
        for (i, w) in p.words.iter().enumerate() {
            let show = match self.ui.word_filter.as_str() {
                "Needs attention" => w.decision == Decision::Pending,
                "Included" => w.decision == Decision::Keep,
                "Excluded" => w.decision == Decision::Reject,
                _ => true,
            };
            if !show {
                continue;
            }
            if ui
                .selectable_label(
                    self.ui.selected_word == i,
                    format!(
                        "{}    {}    · {}",
                        w.term,
                        if w.meaning.is_empty() {
                            "meaning needed"
                        } else {
                            &w.meaning
                        },
                        match w.decision {
                            Decision::Pending => "Needs attention",
                            Decision::Keep => "Included",
                            Decision::Reject => "Excluded",
                        }
                    ),
                )
                .clicked()
            {
                self.ui.selected_word = i;
            }
        }
        self.ui.selected_word = self.ui.selected_word.min(p.words.len() - 1);
        let word = &p.words[self.ui.selected_word];
        card(ui, |ui| {
            ui.heading(&word.term);
            ui.label(format!("Source frequency: {}", word.frequency));
            ui.label("English meaning");
            let mut meaning = word.meaning.clone();
            if ui
                .add(text_input(&mut meaning).desired_width(f32::INFINITY))
                .changed()
            {
                if p.native_run.is_some() {
                    let current = self.engine.project_mut(id).unwrap();
                    current
                        .words
                        .iter_mut()
                        .find(|entry| entry.id == word.id)
                        .unwrap()
                        .meaning = meaning;
                    current.previewed_revision = None;
                    current.native_dirty = true;
                } else {
                    self.run(|e| e.edit_word(id, &word.id, meaning, word.notes.clone()));
                }
            }
            ui.label("Example");
            ui.label(RichText::new(&word.example).italics());
            if p.source.media {
                self.player(ui, p);
            }
            let mut notes = word.notes.clone();
            ui.label("Notes");
            if ui.add(text_input(&mut notes)).changed() {
                if p.native_run.is_some() {
                    let current = self.engine.project_mut(id).unwrap();
                    current
                        .words
                        .iter_mut()
                        .find(|entry| entry.id == word.id)
                        .unwrap()
                        .notes = notes;
                    current.previewed_revision = None;
                    current.native_dirty = true;
                } else {
                    self.run(|e| e.edit_word(id, &word.id, word.meaning.clone(), notes));
                }
            }
            if p.native_run.is_some() && ui.button("Save word changes").clicked() {
                let current = self
                    .engine
                    .project(id)
                    .unwrap()
                    .words
                    .iter()
                    .find(|entry| entry.id == word.id)
                    .unwrap()
                    .clone();
                self.run(|engine| {
                    engine.edit_word(id, &current.id, current.meaning, current.notes)
                });
            }
            ui.horizontal_wrapped(|ui| {
                if primary(ui, "Include").clicked() {
                    self.run(|e| e.decide(id, &word.id, Decision::Keep));
                }
                if ui.button("Exclude from this project").clicked() {
                    self.run(|e| e.decide(id, &word.id, Decision::Reject));
                }
                if ui.button("Decide later").clicked() {
                    self.run(|e| e.decide(id, &word.id, Decision::Pending));
                }
            });
            ui.collapsing("Add to words I know…", |ui| {
                if let Some(profile) = self.engine.state.profiles.iter_mut().find(|profile| profile.name == p.settings.profile_name) {
                    ui.small(format!("Adds to {} for future projects using this profile. External lists are unchanged.", profile.name));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("This exact spelling").clicked() { profile.exact_words.push_str(&format!("{}\n", word.term)); self.notice = "Added to this profile's spelling list for future projects.".into(); }
                        if ui.button("This word family").clicked() { profile.word_families.push_str(&format!("{}\n", word.term)); self.notice = "Added to this profile's family list for future projects.".into(); }
                    });
                } else {
                    ui.label("This project uses no saved profile. Choose a profile when starting a project to keep a reusable list of known words.");
                    if ui.button("Manage saved profiles").clicked() { self.ui.page = Page::Profiles; }
                }
            });
        });
        ui.add_space(12.0);
        ui.label(format!(
            "{} included · {} still need attention",
            p.included(),
            p.pending()
        ));
        if p.pending() == 0 && primary(ui, "Prepare study cards").clicked() {
            self.run(|e| e.finish_manual(id));
            self.engine.state.selected_stage = Stage::Cards;
        }
    }
    fn cards(&mut self, ui: &mut egui::Ui, p: &Project) {
        title(
            ui,
            "A last look before you study",
            "Check a few meanings and examples. You can return to Words to make changes.",
        );
        if !matches!(p.status, Status::Ready | Status::Finished) {
            ui.label("Preparing example cards…");
            return;
        }
        ui.label(format!(
            "{} words · {} cards · {} unresolved",
            p.included(),
            p.card_count(),
            p.pending()
        ));
        let words: Vec<_> = p
            .words
            .iter()
            .filter(|w| w.decision == Decision::Keep)
            .collect();
        if words.is_empty() {
            card(ui, |ui| {
                ui.heading("No words to export");
                ui.label(
                    "All candidates were excluded. You can return to Words to change your choices.",
                );
            });
        } else {
            self.ui.card_index = self.ui.card_index.min(words.len() - 1);
            let w = words[self.ui.card_index];
            card(ui, |ui| {
                ui.set_min_height(200.0);
                ui.vertical_centered(|ui| {
                    ui.small(if self.ui.card_back { "ANSWER" } else { "FRONT" });
                    ui.add_space(15.0);
                    ui.label(RichText::new(&w.term).size(32.0).strong());
                    ui.label(&w.example);
                    if self.ui.card_back {
                        ui.add_space(10.0);
                        ui.label(RichText::new(&w.meaning).size(24.0).color(accent(ui)));
                    }
                    ui.add_space(12.0);
                    if ui
                        .button(if self.ui.card_back {
                            "Show front"
                        } else {
                            "Show answer"
                        })
                        .clicked()
                    {
                        self.ui.card_back = !self.ui.card_back;
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.ui.card_index > 0, egui::Button::new("Previous"))
                    .clicked()
                {
                    self.ui.card_index -= 1;
                }
                ui.label(format!("{} / {}", self.ui.card_index + 1, words.len()));
                if ui
                    .add_enabled(
                        self.ui.card_index + 1 < words.len(),
                        egui::Button::new("Next"),
                    )
                    .clicked()
                {
                    self.ui.card_index += 1;
                }
                if ui.button("Edit this word").clicked() {
                    self.ui.selected_word = p.words.iter().position(|x| x.id == w.id).unwrap();
                    self.engine.state.selected_stage = Stage::Words;
                }
            });
        }
        if p.source.media {
            self.player(ui, p);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Card design…").clicked() {
                self.open_design();
            }
            if primary(ui, "Continue to export").clicked() {
                self.run(|e| e.preview(p.id));
                if self.error.is_empty() {
                    self.engine.state.selected_stage = Stage::Export;
                }
            }
        });
        ui.small("Preview shows card content as plain text. Exported decks use the configured Anki HTML and CSS.");
    }
    fn exports(&mut self, ui: &mut egui::Ui, p: &Project) {
        title(
            ui,
            "Take your words with you",
            "Choose how you want to use your study cards.",
        );
        ui.label(format!(
            "{} · {} words · {} cards",
            p.deck,
            p.included(),
            p.card_count()
        ));
        if p.previewed_revision != Some(p.revision) {
            ui.label("Preview the current cards first.");
            if primary(ui, "Preview cards").clicked() {
                self.engine.state.selected_stage = Stage::Cards;
            }
            return;
        }
        if p.native_run.is_some() {
            card(ui, |ui| {
                ui.heading("Create a portable deck");
                ui.label("The deck includes your reviewed cards and their media. Export files are kept inside this project's folder.");
                if primary(ui, "Create Anki deck file").clicked() {
                    self.run(|engine| engine.export_demo(p.id, "APKG"));
                }
                if ui.button("Create table and media folder").clicked() {
                    self.run(|engine| engine.export_demo(p.id, "TSV"));
                }
            });
            card(ui, |ui| {
                ui.heading("Send to Anki");
                ui.label("Open Anki with AnkiConnect installed. Existing cards follow the configured skip/update policy; scheduling is preserved.");
                if ui.button("Send reviewed cards to Anki").clicked() {
                    self.run(|engine| engine.export_demo(p.id, "Anki"));
                }
            });
            for receipt in &p.receipts {
                ui.label(&receipt.label);
            }
            if !p.receipts.is_empty()
                && ui.button("Show exported files").clicked()
                && let Some(directory) = &p.native_run
                && let Err(error) = blitzline_adapters::pipeline::open_file(
                    &std::path::Path::new(directory).join("exports"),
                )
            {
                self.error = error.message;
            }
            return;
        }
        card(ui, |ui| {
            ui.heading("Save an Anki deck file");
            ui.label("This example records a practice export. Start a real project to create an Anki deck.");
            if primary(ui, "Simulate deck export").clicked() {
                self.run(|e| e.export_demo(p.id, "APKG"));
            }
        });
        card(ui, |ui| {
            ui.heading("Send to Anki");
            ui.label("Examples do not send cards to Anki. Real projects support AnkiConnect.");
        });
        ui.collapsing("Export a table and media", |ui| {
            ui.label("Save the example words as a real TSV. No media is generated.");
            if ui.button("Save sample TSV…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .set_file_name("blitzline-sample.tsv")
                    .save_file()
            {
                match self.engine.tsv(p.id) {
                    Ok(text) => match std::fs::write(&path, text) {
                        Ok(()) => {
                            self.run(|e| e.export_demo(p.id, "Sample TSV"));
                            self.notice = format!("Sample table saved to {}", path.display());
                        }
                        Err(e) => self.error = e.to_string(),
                    },
                    Err(e) => self.error = e.to_string(),
                }
            }
        });
        if !p.receipts.is_empty() {
            ui.heading("Export history");
            for receipt in &p.receipts {
                ui.label(format!("{} · revision {}", receipt.label, receipt.revision));
            }
        }
    }
    fn player(&mut self, ui: &mut egui::Ui, project: &Project) {
        if !project.is_example() {
            if ui.button("Open recording in your media player").clicked()
                && let Some(path) = &project.source.path
                && let Err(error) =
                    blitzline_adapters::pipeline::open_file(std::path::Path::new(path))
            {
                self.error = error.message;
            }
            metadata(ui, "Playback opens in your system's media player.");
            return;
        }
        card(ui, |ui| {
            ui.small("EXAMPLE PLAYER · No audio is played");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(if self.playback {
                        "Pause example"
                    } else {
                        "Play example"
                    })
                    .clicked()
                {
                    self.playback = !self.playback;
                }
                slider(
                    ui,
                    egui::Slider::new(&mut self.position, 0.0..=12.0).text("seconds"),
                );
            });
        });
    }
    fn open_website(&mut self, ctx: &egui::Context) {
        let url = self.engine.state.preferences.website.trim();
        if url.starts_with("https://") && url.len() > 8 && !url.chars().any(char::is_whitespace) {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        } else {
            self.error="Choose a chat website with a full https:// address in Settings. You can also use your browser directly.".into();
            self.settings_open = true;
            self.ui.settings_category = "Browser assistance".into();
        }
    }
}

pub(super) fn language_picker(ui: &mut egui::Ui, value: &mut String, id: impl std::hash::Hash) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(match value.as_str() {
            "slv" => "Slovenian",
            "pol" => "Polish",
            "eng" => "English",
            "" => "Use project language",
            _ => value,
        })
        .show_ui(ui, |ui| {
            for (code, label) in [("slv", "Slovenian"), ("pol", "Polish"), ("eng", "English")] {
                ui.selectable_value(value, code.into(), label);
            }
        });
}
pub(super) fn stage_methods(ui: &mut egui::Ui, settings: &mut Settings) {
    for (name, override_method) in [
        ("Proofreading", &mut settings.proof_method),
        ("Translation", &mut settings.translation_method),
    ] {
        egui::ComboBox::from_label(name)
            .selected_text(
                override_method
                    .map(Method::label)
                    .unwrap_or("Use project choice"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(override_method, None, "Use project choice");
                for method in Method::ALL {
                    ui.selectable_value(override_method, Some(method), method.label());
                }
            });
    }
    slider(
        ui,
        egui::Slider::new(&mut settings.batch_size, 1..=20).text("Words per request"),
    );
    checkbox(
        ui,
        &mut settings.automatic_export,
        "Automatically export when every word is resolved",
    );
}
fn yes(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}

fn setup_heading(ui: &mut egui::Ui, title: &str, help: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(title);
        let response = ui.small_button("?").on_hover_text(help);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_max_width(360.0);
            ui.label(help);
        });
    });
}

fn local_engine_label(preferences: &Preferences) -> String {
    let path = preferences.native_config.as_deref().map(std::path::Path::new);
    let engine = match blitzline_adapters::pipeline::config::load(path) {
        Ok(c) if blitzline_adapters::pipeline::config::is_local_worker(&c["asr"]) => {
            let program = c["asr"]["worker_command"][0].as_str().and_then(|p| std::path::Path::new(p).file_name()?.to_str()).unwrap_or("service");
            format!("ASR worker: {program}")
        }
        _ => "whisper.cpp".into(),
    };
    format!("On this computer ({engine})")
}
