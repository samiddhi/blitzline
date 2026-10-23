use super::screens::{language_picker, stage_methods};
use super::*;

const CATEGORIES: [&str; 8] = [
    "General & appearance",
    "AI connections",
    "Browser assistance",
    "Transcription",
    "Words & review",
    "Cards & export",
    "Processing & storage",
    "Import & diagnostics",
];
impl Desktop {
    pub(super) fn import_settings(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Blitzline configuration", &["toml"])
            .pick_file()
        {
            match blitzline_adapters::import_profiles(&path) {
                Ok((profiles, report)) => {
                    for mut p in profiles {
                        if self.engine.state.profiles.iter().any(|s| s.name == p.name) {
                            p.name.push_str(" (imported)");
                        }
                        self.engine.state.profiles.push(p);
                    }
                    self.import_report = report;
                    self.settings_open = true;
                    self.ui.settings_category = "Import & diagnostics".into();
                }
                Err(e) => self.error = e.to_string(),
            }
        }
    }
    pub(super) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "Settings",
            "Simple defaults. More control when you need it.",
        );
        ui.add(
            text_input(&mut self.ui.settings_search)
                .hint_text("Search settings, including advanced options")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);
        let wide = ui.available_width() >= 680.0;
        ui.with_layout(if wide { egui::Layout::left_to_right(egui::Align::Min) } else { egui::Layout::top_down(egui::Align::Min) }, |ui| {
            if wide {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                ui.strong("Preferences");
                for category in CATEGORIES {
                    let selected = self.ui.settings_category == category || (self.ui.settings_category.is_empty() && category == CATEGORIES[0]);
                    if ui.add_sized([200.0, 38.0], egui::Button::new(category).selected(selected).right_text("›")).clicked() {
                        self.ui.settings_category = category.into();
                        self.ui.settings_search.clear();
                    }
                }
            });
            } else {
                let selected = if self.ui.settings_category.is_empty() { CATEGORIES[0] } else { &self.ui.settings_category };
                egui::ComboBox::from_label("Preferences").selected_text(selected).show_ui(ui, |ui| {
                    for category in CATEGORIES {
                        if ui.selectable_label(self.ui.settings_category == category, category).clicked() {
                            self.ui.settings_category = category.into(); self.ui.settings_search.clear();
                        }
                    }
                });
            }
            ui.vertical(|ui| {
            ui.set_width(ui.available_width());
        let query = self.ui.settings_search.to_lowercase();
        let category = if self.ui.settings_category.is_empty() {
            CATEGORIES[0]
        } else {
            &self.ui.settings_category
        };
        let groups: Vec<_> = CATEGORIES
            .into_iter()
            .filter(|name| {
                if query.is_empty() {
                    *name == category
                } else {
                    let keywords = match *name {
                        "General & appearance" => "theme light dark size scale",
                        "AI connections" => {
                            "api key model endpoint timeout retries provider openrouter deepseek"
                        }
                        "Browser assistance" => "website chat prompt clipboard",
                        "Transcription" => "audio model local device download",
                        "Words & review" => "batch limit audit ambiguity character",
                        "Cards & export" => "template html css preset recall fields anki",
                        "Processing & storage" => "folder files cache queue concurrency",
                        _ => "import python diagnostics logs config",
                    };
                    format!("{name} {keywords}").to_lowercase().contains(&query)
                }
            })
            .collect();
        egui::ScrollArea::vertical().id_salt("settings_scroll").show(ui,|ui|{
            if groups.is_empty(){ui.label("No settings match. Try a broader word.");}
            for group in groups{
                ui.heading(group);
                ui.add_space(6.0);
                ui.separator();
                match group{
                    "General & appearance"=>{
                        let prefs=&mut self.engine.state.preferences;
                        let mut changed=false;
                        ui.strong("Appearance");
                        ui.horizontal_wrapped(|ui|{
                            for value in ["System","Light","Dark"] {
                                if ui.add(egui::Button::new(value).selected(prefs.appearance == value)).clicked() {
                                    prefs.appearance = value.into(); changed = true;
                                }
                            }
                        });
                        section(ui, "Text and interface size");
                        metadata(ui, "A little smaller or larger, throughout Blitzline.");
                        changed|=slider(ui, egui::Slider::new(&mut prefs.scale,0.8..=1.7).suffix("×")).changed();
                        if changed{self.configure(ui.ctx());}
                    }
                    "AI connections"=>{
                        ui.label("An API key lets an app send requests to a service for you.");
                        let prefs=&mut self.engine.state.preferences;
                        let previous_provider = prefs.provider.clone();
                        egui::ComboBox::from_label("Provider").selected_text(&prefs.provider).show_ui(ui,|ui|{
                            for name in ["OpenRouter","DeepSeek","Custom compatible service"]{ui.selectable_value(&mut prefs.provider,name.into(),name);}
                        });
                        if prefs.provider != previous_provider { self.secret.clear(); prefs.endpoint.clear(); prefs.model.clear(); }
                        section(ui, "Connection details");ui.label("Model");ui.add(text_input(&mut prefs.model).hint_text("Model identifier from your service"));
                        section(ui, "API key");metadata(ui, "Session only; never saved to project files");
                        ui.add(text_input(&mut self.secret).password(true).hint_text("Paste key"));
                        ui.small("Keys are never written to project or settings files. Enter the key again after reopening the app.");
                        if ui.button("Check setup (no network)").clicked(){
                            match live::connection(prefs) {
                                Ok(profile) => { self.notice = if self.secret.trim().is_empty() && !profile["api_key_env"].as_str().is_some_and(|name| std::env::var(name).is_ok_and(|key| !key.trim().is_empty())) { "Enter an API key or set the configured credential environment variable.".into() } else { "Connection settings are valid. No service was contacted; the credential has not been verified.".into() }; }
                                Err(error) => self.error = error.message,
                            }
                        }
                        ui.separator();
                        egui::CollapsingHeader::new("Advanced connection settings").default_open(!query.is_empty()).show(ui,|ui|{
                            ui.label("Endpoint");if ui.add(text_input(&mut prefs.endpoint)).changed() { self.secret.clear(); }
                            ui.add(egui::DragValue::new(&mut prefs.timeout).range(1..=7200).suffix(" seconds timeout"));
                            ui.add(egui::DragValue::new(&mut prefs.retries).range(0..=10).suffix(" retries"));
                            ui.small("Response modes, reasoning, and routing can be selected in a native configuration file.");
                        });
                    }
                    "Browser assistance"=>{
                        ui.label("Use any chat website you already know. Blitzline only opens the page; you send the request yourself.");
                        ui.label("Preferred chat website");
                        ui.add(text_input(&mut self.engine.state.preferences.website).hint_text("https://…").desired_width(f32::INFINITY));
                        ui.label("Copy request → paste into a new chat → copy its complete answer → paste here → Check answer.");
                        ui.small("No API key is needed. Free access depends on the chat service's limits.");
                        if ui.button("Open the walkthrough").clicked(){self.help_open=true;self.ui.help_search="browser".into();}
                    }
                    "Transcription"=>{
                        ui.label("Local transcription uses whisper.cpp; online transcription uses the configured Groq or HTTP service. Imported subtitles bypass transcription.");
                        match blitzline_adapters::pipeline::config::load(self.engine.state.preferences.native_config.as_deref().map(std::path::Path::new)) {
                            Ok(config) => {
                                let path = config["asr"]["model_file"].as_str().filter(|path|!path.is_empty()).map(std::path::PathBuf::from).unwrap_or_else(||blitzline_adapters::pipeline::config::data_directory().join(format!("resources/models/ggml-{}.bin", config["asr"]["model"].as_str().unwrap_or("small"))));
                                metadata(ui, format!("Local model: {} · {}", if path.is_file(){"file found"}else{"not installed"}, path.display()));
                            }
                            Err(error) => { ui.label(error.message); }
                        }
                        section(ui, "Online transcription API key");
                        ui.add(text_input(&mut self.transcription_secret).password(true).hint_text("Groq or configured HTTP worker key"));
                        metadata(ui, "Session only, separate from the model key. Only used when Online service is selected. No key is needed for local transcription or imported subtitles.");
                        if ui.button("Open tools & downloads").clicked(){self.ui.page=Page::Resources;self.settings_open=false;}
                    }
                    "Words & review"=>{
                        let prefs=&mut self.engine.state.preferences;
                        ui.strong("Words per request");
                        metadata(ui, "Smaller batches are easier to review in a browser chat.");
                        if slider(ui, egui::Slider::new(&mut prefs.batch_size,1..=20).suffix(" words")).changed() { self.engine.state.setup.settings.batch_size = prefs.batch_size; }
                        ui.separator();
                        egui::CollapsingHeader::new("Advanced review settings").default_open(!query.is_empty()).show(ui,|ui|{
                            ui.add(egui::DragValue::new(&mut prefs.max_chars).range(1000..=100000).suffix(" characters per request"));
                            checkbox(ui, &mut prefs.ambiguity,"Additional ambiguity review");
                            checkbox(ui, &mut prefs.audit,"Final correction-only audit");
                            ui.small("These independent passes use your configured model and may incur additional charges.");
                        });
                    }
                    "Cards & export"=>{
                        ui.label("Sentence cards by default. Optional recall is selected per project.");
                        if ui.button("Open card design editor").clicked(){self.open_design();}
                        ui.label("Exports support APKG, TSV with media, and AnkiConnect. Preview cards before exporting.");
                    }
                    "Processing & storage"=>{
                        ui.label("One project processes at a time. Waiting projects release the queue.");
                        if let Some(store)=&self.store{ui.label(format!("Application data: {}",store.directory.display()));}
                        ui.label("Reopening the app pauses unfinished processing. Nothing runs after quitting.");
                        ui.small("Each project has a locked run folder shared with the CLI. Configure native tools in Tools & downloads.");
                    }
                    _=>{
                        section(ui, "Command-line tool");
                        ui.label("Install the bundled Rust CLI in a folder you choose. It runs without Python or this app being open.");
                        if ui.button("Install command-line tool…").clicked(){self.install_cli();}
                        if ui.button("Import Python settings…").clicked(){self.import_settings();}
                        for line in &self.import_report{ui.label(line);}
                        if ui.button("Copy redacted diagnostics").clicked(){
                            let text=format!("Blitzline 0.1.0\nOS: {}\nProjects: {}\nEngine: Rust pipeline; explicit examples\nCredentials: session/environment only\n",std::env::consts::OS,self.engine.state.projects.len());
                            ui.ctx().copy_text(text);self.notice="Diagnostics copied without keys, source text, or paths.".into();
                        }
                    }
                }
                ui.add_space(18.0);
            }
        });
            });
        });
        ui.separator();
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if ui.button("Done").clicked() {
            self.settings_open = false;
            self.save_now();
        }
    }
    pub(super) fn profiles(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "Languages & decks",
            "Reusable choices for each language you study.",
        );
        if ui.button("Add profile").clicked() {
            self.engine.state.profiles.push(Profile {
                name: format!("New profile {}", self.engine.state.profiles.len() + 1),
                ..Default::default()
            });
        }
        for (i, profile) in self.engine.state.profiles.iter_mut().enumerate() {
            ui.push_id(i,|ui|{
                card(ui,|ui|{
                    ui.label("Profile name");ui.add(text_input(&mut profile.name));
                    language_picker(ui,&mut profile.language,("profile_language",i));
                    ui.label("Anki deck");ui.add(text_input(&mut profile.deck));
                    checkbox(ui, &mut profile.settings.recall,"Include recall cards");
                    ui.collapsing("Words I know",|ui|{
                        ui.label("Exact spellings — skips only these written forms, such as 'am'.");
                        ui.add(text_area(&mut profile.exact_words).desired_rows(3).desired_width(f32::INFINITY));
                        ui.label("Word families — skips dictionary-connected forms, such as the forms of 'be'.");
                        ui.add(text_area(&mut profile.word_families).desired_rows(3).desired_width(f32::INFINITY));
                        ui.small("Managed lists are captured when a new project selects this profile. External Blitzer lists remain unchanged.");
                    });
                    ui.collapsing("Advanced defaults",|ui|{
                        for method in Method::ALL{ui.radio_value(&mut profile.settings.method,method,method.label());}
                        stage_methods(ui,&mut profile.settings);
                    });
                });
            });
            ui.add_space(12.0);
        }
        ui.label("Changes apply to new projects that select this profile. Existing projects keep their saved settings.");
    }
    pub(super) fn resources(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "Tools & downloads",
            "Everything a project needs, in one place.",
        );
        let path = self
            .engine
            .state
            .preferences
            .native_config
            .as_deref()
            .map(std::path::Path::new);
        match blitzline_adapters::pipeline::config::load(path) {
            Ok(config) => {
                for (label, section_name, key) in [
                    ("Blitzer", "blitzer", "executable"),
                    ("FFmpeg", "media", "ffmpeg"),
                    ("FFprobe", "media", "ffprobe"),
                    ("Local transcription", "asr", "executable"),
                ] {
                    card(ui, |ui| {
                        ui.strong(label);
                        match blitzline_adapters::pipeline::find_tool(
                            config[section_name][key].as_str().unwrap_or(""),
                        ) {
                            Ok(path) => {
                                metadata(ui, format!("Installed · {}", path.display()));
                            }
                            Err(error) => {
                                ui.label(error.message);
                            }
                        }
                    });
                }
                let model = config["asr"]["model_file"]
                    .as_str()
                    .filter(|path| !path.is_empty())
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| {
                        blitzline_adapters::pipeline::config::data_directory().join(format!(
                            "resources/models/ggml-{}.bin",
                            config["asr"]["model"].as_str().unwrap_or("small")
                        ))
                    });
                section(ui, "Transcription model");
                metadata(
                    ui,
                    format!(
                        "{} · {}",
                        if model.is_file() {
                            "Installed"
                        } else {
                            "Not installed"
                        },
                        model.display()
                    ),
                );
            }
            Err(error) => {
                ui.label(error.message);
            }
        }
        section(ui, "Advanced configuration");
        ui.label("A native release can bundle these tools and a preconverted model. Text projects need only compiled Blitzer. Python-based tools are rejected.");
        ui.label("Choose an existing configuration to use its tool paths, service profiles, dictionary settings, and note templates. Relative paths stay relative to that file.");
        if ui.button("Choose configuration file…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Blitzline configuration", &["toml"])
                .pick_file()
        {
            match blitzline_adapters::pipeline::config::load(Some(&path)) {
                Ok(_) => {
                    self.transcription_secret.clear();
                    self.engine.state.preferences.native_config =
                        Some(path.to_string_lossy().into());
                    self.notice = "Configuration selected for new projects.".into();
                }
                Err(error) => self.error = error.message,
            }
        }
        if let Some(path) = &self.engine.state.preferences.native_config {
            metadata(ui, path);
        }
        if ui.button("Use default configuration").clicked() {
            self.transcription_secret.clear();
            self.engine.state.preferences.native_config = None;
        }
        if let Some(id) = self.selected()
            && ui.button("Return to project").clicked()
        {
            self.open_project(id);
        }
    }
    pub(super) fn open_design(&mut self) {
        let prefs = &self.engine.state.preferences;
        self.design_front = prefs.card_front.clone();
        self.design_back = prefs.card_back.clone();
        self.design_css = prefs.card_css.clone();
        self.design_open = true;
    }
    pub(super) fn design_ui(&mut self, ui: &mut egui::Ui) {
        title(
            ui,
            "Card design",
            "Advanced · draft changes stay separate until you apply them.",
        );
        egui::ScrollArea::vertical().id_salt("design_scroll").show(ui,|ui|{
            ui.label("Common note fields: Word, Translation, Sentence1, Sentence2, Comments, Frequency, Sentence1 Audio, Sentence2 Audio.");
            ui.label("Front template");
            ui.add(text_area(&mut self.design_front).code_editor().desired_rows(4).desired_width(f32::INFINITY));
            ui.label("Back template");
            ui.add(text_area(&mut self.design_back).code_editor().desired_rows(4).desired_width(f32::INFINITY));
            ui.collapsing("Anki card styling",|ui|{ui.add(text_area(&mut self.design_css).code_editor().desired_rows(5).desired_width(f32::INFINITY));});
            card(ui,|ui|{
                ui.strong("Safe text preview");
                for source in [&self.design_front,&self.design_back] {
                    ui.label(source.replace("{{Word}}","mačka").replace("{{Translation}}","cat").replace("{{Sentence1}}","Mačka spi."));
                    ui.separator();
                }
                ui.small("This preview shows text. Anki applies the templates and CSS when importing a deck. Select a native configuration file in Tools & downloads to configure custom fields and assets.");
            });
            ui.horizontal_wrapped(|ui|{
                if primary(ui,"Apply design").clicked(){
                    if self.design_front.trim().is_empty()||self.design_back.trim().is_empty(){self.error="Both sides of the card need content.".into();}
                    else{
                        let prefs=&mut self.engine.state.preferences;
                        prefs.card_front=self.design_front.clone();prefs.card_back=self.design_back.clone();prefs.card_css=self.design_css.clone();
                        self.design_open=false;self.notice="Card design saved for new projects. Exported decks use this HTML and CSS.".into();
                    }
                }
                if ui.button("Cancel").clicked(){self.design_open=false;}
                if ui.button("Reset draft").clicked(){let prefs=Preferences::default();self.design_front=prefs.card_front;self.design_back=prefs.card_back;self.design_css=prefs.card_css;}
            });
        });
    }
    pub(super) fn project_settings_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.project_settings else {
            self.project_settings = None;
            return;
        };
        let Ok(p) = self.engine.project(id).cloned() else {
            self.project_settings = None;
            return;
        };
        if self.project_options.is_none() {
            self.project_options = Some((p.settings.clone(), p.name.clone()));
        }
        let (mut settings, mut name) = self.project_options.clone().unwrap();
        let mut done = false;
        let closed = secondary_window(
            ctx,
            "project_settings",
            "Blitzline · Project settings",
            [660.0, 700.0],
            |ui| {
                title(ui, "Project settings", &p.name);
                ui.label("Changes affect only this project. Processing will pause; review may need rebuilding.");
                section(ui, "Project");
                ui.label("Project name");
                ui.add(text_input(&mut name));
                section(ui, "Help with vocabulary");
                for method in Method::ALL {
                    ui.radio_value(&mut settings.method, method, method.label());
                }
                section(ui, "Optional steps");
                checkbox(ui, &mut settings.proofread, "Proofread");
                checkbox(ui, &mut settings.translate, "English translation");
                checkbox(ui, &mut settings.recall, "Recall cards");
                ui.collapsing("Advanced settings", |ui| {
                    stage_methods(ui, &mut settings);
                });
                ui.separator();
                if !self.error.is_empty() {
                    ui.colored_label(ui.visuals().error_fg_color, &self.error);
                }
                ui.horizontal_wrapped(|ui| {
                    if primary(ui, "Apply to this project").clicked() {
                        if name.trim().is_empty() {
                            self.error = "Give the project a name.".into();
                        } else {
                            match self.engine.update_settings(id, settings.clone()) {
                                Ok(()) => {
                                    self.engine.project_mut(id).unwrap().name = name.trim().into();
                                    self.error.clear();
                                    done = true;
                                }
                                Err(error) => self.error = error.to_string(),
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        done = true;
                    }
                    if ui.button("Save as reusable profile").clicked() {
                        self.engine.state.profiles.push(Profile {
                            name: format!("{} settings", p.name),
                            language: p.source.language.clone(),
                            deck: p.deck.clone(),
                            settings: settings.clone(),
                            ..Default::default()
                        });
                        self.notice =
                            "Saved a new profile. Other projects were not changed.".into();
                    }
                });
            },
        );
        if done || closed {
            self.project_settings = None;
            self.project_options = None;
        } else {
            self.project_options = Some((settings, name));
        }
    }
}
