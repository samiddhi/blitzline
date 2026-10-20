//! Native GUI skeleton. Services and delivery are deliberately simulated.
#[cfg(target_os = "macos")]
mod macos;
mod screens;
mod settings;
use blitzline_adapters::Store;
use blitzline_domain::*;
use blitzline_engine::{Engine, Repository};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const ACCENT: Color32 = Color32::from_rgb(48, 99, 204);
const INK: Color32 = Color32::from_rgb(35, 39, 47);

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Page {
    #[default]
    Projects,
    Setup,
    Project,
    Profiles,
    Resources,
}
#[derive(Default, Serialize, Deserialize)]
pub struct UiState {
    page: Page,
    project_filter: String,
    search: String,
    word_filter: String,
    selected_word: usize,
    card_index: usize,
    card_back: bool,
    settings_category: String,
    settings_search: String,
    help_search: String,
}

pub struct Desktop {
    #[cfg(target_os = "macos")]
    native: Option<macos::NativeChrome>,
    pub engine: Engine,
    store: Option<Store>,
    ui: UiState,
    settings_open: bool,
    help_open: bool,
    design_open: bool,
    compact: Option<u64>,
    compact_top: bool,
    project_settings: Option<u64>,
    project_options: Option<(Settings, String)>,
    notice: String,
    error: String,
    secret: String,
    import_report: Vec<String>,
    edit_text: bool,
    confirm_text: bool,
    confirm_cleanup: bool,
    confirm_delete: Option<u64>,
    confirm_quit: bool,
    quit_allowed: bool,
    last_tick: Instant,
    last_save: Instant,
    persisted: Vec<u8>,
    design_front: String,
    design_back: String,
    design_css: String,
    downloading: Option<Instant>,
    playback: bool,
    position: f32,
}

impl Desktop {
    pub fn new(store: Option<Store>, state: Workspace) -> Self {
        let ui = store.as_ref().map(Store::load_ui).unwrap_or_default();
        Self {
            #[cfg(target_os = "macos")]
            native: None,
            engine: Engine::new(state),
            store,
            ui,
            settings_open: false,
            help_open: false,
            design_open: false,
            compact: None,
            compact_top: false,
            project_settings: None,
            project_options: None,
            notice: String::new(),
            error: String::new(),
            secret: String::new(),
            import_report: vec![],
            edit_text: false,
            confirm_text: false,
            confirm_cleanup: false,
            confirm_delete: None,
            confirm_quit: false,
            quit_allowed: false,
            last_tick: Instant::now(),
            last_save: Instant::now(),
            persisted: vec![],
            design_front: String::new(),
            design_back: String::new(),
            design_css: String::new(),
            downloading: None,
            playback: false,
            position: 0.0,
        }
    }
    pub fn configure(&self, ctx: &egui::Context) {
        ctx.set_zoom_factor(self.engine.state.preferences.scale);
        ctx.set_theme(match self.engine.state.preferences.appearance.as_str() {
            "Light" => egui::ThemePreference::Light,
            "Dark" => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        });
        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 12.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.spacing.interact_size.y = 34.0;
            style.spacing.slider_width = 220.0;
            style.spacing.slider_rail_height = 5.0;
            style.visuals.slider_trailing_fill = true;
            style.visuals.handle_shape = egui::style::HandleShape::Circle;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
            if !style.visuals.dark_mode {
                style.visuals.panel_fill = Color32::from_rgb(247, 248, 250);
                style.visuals.window_fill = Color32::WHITE;
                style.visuals.override_text_color = Some(INK);
                style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(237, 240, 246);
                style.visuals.weak_text_color = Some(Color32::from_rgb(98, 106, 121));
            } else {
                style.visuals.override_text_color = Some(Color32::from_rgb(233, 235, 241));
                style.visuals.weak_text_color = Some(Color32::from_rgb(163, 172, 189));
                style.visuals.panel_fill = Color32::from_rgb(25, 28, 34);
                style.visuals.window_fill = Color32::from_rgb(33, 37, 45);
                style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(45, 50, 61);
            }
            let dark = style.visuals.dark_mode;
            style.visuals.selection.bg_fill = if dark {
                Color32::from_rgb(49, 65, 92)
            } else {
                Color32::from_rgb(222, 232, 250)
            };
            style.visuals.selection.stroke = egui::Stroke::new(
                1.0,
                if dark {
                    Color32::from_rgb(221, 232, 255)
                } else {
                    Color32::from_rgb(37, 75, 146)
                },
            );
            let border = if dark {
                Color32::from_rgb(57, 63, 75)
            } else {
                Color32::from_rgb(225, 229, 237)
            };
            style.visuals.window_stroke = egui::Stroke::new(1.0, border);
            style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border);
            style.visuals.extreme_bg_color = if dark {
                Color32::from_rgb(24, 27, 33)
            } else {
                Color32::from_rgb(250, 251, 253)
            };
            for widget in [
                &mut style.visuals.widgets.inactive,
                &mut style.visuals.widgets.hovered,
                &mut style.visuals.widgets.active,
            ] {
                widget.corner_radius = egui::CornerRadius::same(8);
                widget.bg_stroke = egui::Stroke::new(
                    1.0,
                    if dark {
                        Color32::from_rgb(126, 139, 160)
                    } else {
                        Color32::from_rgb(132, 146, 166)
                    },
                );
            }
            style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, ACCENT);
            style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, ACCENT);
            style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(
                1.5,
                if dark {
                    Color32::from_rgb(222, 231, 248)
                } else {
                    INK
                },
            );
            style.visuals.widgets.hovered.bg_fill = if dark {
                Color32::from_rgb(58, 67, 84)
            } else {
                Color32::from_rgb(225, 233, 249)
            };
            style.visuals.widgets.active.bg_fill = if dark {
                Color32::from_rgb(63, 80, 112)
            } else {
                Color32::from_rgb(210, 224, 249)
            };
            style.visuals.widgets.inactive.weak_bg_fill = style.visuals.widgets.inactive.bg_fill;
        });
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            native.appearance(&self.engine.state.preferences.appearance);
        }
    }
    pub fn install_platform(&mut self, cc: &eframe::CreationContext<'_>) {
        #[cfg(target_os = "macos")]
        {
            self.native = macos::NativeChrome::install(cc);
            // Load the installed system typeface; never redistribute Apple's font.
            if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
                let mut fonts = egui::FontDefinitions::default();
                fonts
                    .font_data
                    .insert("system".into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "system".into());
                cc.egui_ctx.set_fonts(fonts);
            }
        }
        self.configure(&cc.egui_ctx);
    }
    fn toggle_appearance(&mut self, ctx: &egui::Context) {
        self.engine.state.preferences.appearance = if ctx.style().visuals.dark_mode {
            "Light"
        } else {
            "Dark"
        }
        .into();
        self.configure(ctx);
    }
    fn has_native_toolbar(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }
    fn selected(&self) -> Option<u64> {
        self.engine.state.selected_project
    }
    fn run(&mut self, op: impl FnOnce(&mut Engine) -> Result<()>) {
        match op(&mut self.engine) {
            Ok(()) => self.error.clear(),
            Err(e) => self.error = e.to_string(),
        }
    }
    fn open_project(&mut self, id: u64) {
        self.engine.state.selected_project = Some(id);
        if let Ok(p) = self.engine.project(id) {
            self.engine.state.selected_stage = p.stage;
        }
        self.ui.page = Page::Project;
        self.ui.selected_word = 0;
        self.ui.card_index = 0;
        self.edit_text = false;
        self.error.clear();
    }
    fn save_now(&mut self) -> bool {
        let bytes = serde_json::to_vec(&(&self.engine.state, &self.ui)).unwrap();
        if bytes == self.persisted {
            return true;
        }
        if let Some(store) = &self.store {
            match store
                .save(&self.engine.state)
                .and_then(|_| store.save_ui(&self.ui))
            {
                Ok(()) => {
                    self.persisted = bytes;
                    if self.error.starts_with("Your latest changes are not saved.") {
                        self.error.clear();
                    }
                }
                Err(e) => {
                    self.error = format!("Your latest changes are not saved. {e}");
                    return false;
                }
            }
        } else {
            self.persisted = bytes;
        }
        true
    }
    pub fn load_scenario(&mut self, name: &str) {
        let id = self.engine.example().unwrap();
        for _ in 0..4 {
            self.engine.tick();
        }
        self.open_project(id);
        match name {
            "setup" => self.ui.page = Page::Setup,
            "projects" => self.ui.page = Page::Projects,
            "settings" => self.settings_open = true,
            "help" => self.help_open = true,
            "project-settings" => self.project_settings = Some(id),
            "help-choice" | "ready" => {
                self.ui.page = Page::Setup;
                self.engine.state.setup.pasted_text = "A quiet morning.".into();
                self.engine.state.setup.step = if name == "ready" { 3 } else { 2 };
            }
            "chat-window" => {
                self.compact = Some(id);
                self.engine.state.selected_stage = Stage::Words;
            }
            "cards" | "export" => {
                while self.engine.project(id).unwrap().status != Status::Ready {
                    if self.engine.project(id).unwrap().exchange.is_some() {
                        let reply = self.engine.sample_reply(id).unwrap();
                        self.engine.submit(id, &reply).unwrap();
                        self.engine.next_exchange(id).unwrap();
                    }
                    self.engine.tick();
                }
                self.engine.state.selected_stage = if name == "export" {
                    self.engine.preview(id).unwrap();
                    Stage::Export
                } else {
                    Stage::Cards
                };
            }
            _ => self.engine.state.selected_stage = Stage::Words,
        }
    }
    /// Render without a native event loop too, for accessibility-driven tests.
    pub fn render(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
            self.ui.page = Page::Setup;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings_open = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
            self.pick_files();
        }
        if !self.has_native_toolbar() {
            egui::TopBottomPanel::top("app_toolbar").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("blitzline")
                            .size(23.0)
                            .strong()
                            .color(accent(ui)),
                    );
                    ui.separator();
                    ui.menu_button("File", |ui| {
                        if ui.button("New project    ⌘/Ctrl N").clicked() {
                            self.ui.page = Page::Setup;
                            ui.close();
                        }
                        if ui.button("Choose files…    ⌘/Ctrl O").clicked() {
                            self.pick_files();
                            ui.close();
                        }
                        if ui.button("Import Python settings…").clicked() {
                            self.import_settings();
                            ui.close();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        if ui.button("Projects").clicked() {
                            self.ui.page = Page::Projects;
                            ui.close();
                        }
                        if ui.button("Settings").clicked() {
                            self.settings_open = true;
                            ui.close();
                        }
                        if ui.button("Larger text").clicked() {
                            self.engine.state.preferences.scale =
                                (self.engine.state.preferences.scale + 0.1).min(1.7);
                            self.configure(ctx);
                        }
                        if ui.button("Smaller text").clicked() {
                            self.engine.state.preferences.scale =
                                (self.engine.state.preferences.scale - 0.1).max(0.8);
                            self.configure(ctx);
                        }
                    });
                    if ui.button("New project").clicked() {
                        self.ui.page = Page::Setup;
                    }
                    if ui.button("Help").clicked() {
                        self.help_open = true;
                    }
                    if ui.button("Light / dark").clicked() {
                        self.toggle_appearance(ctx);
                    }
                });
            });
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.weak("Prototype · Sample processing");
                if self
                    .engine
                    .state
                    .projects
                    .iter()
                    .any(|p| matches!(p.status, Status::Running | Status::Queued))
                {
                    ui.spinner();
                    ui.small("Processing example queue");
                } else {
                    ui.small("Progress saved locally");
                }
            });
        });
        egui::SidePanel::left("navigation")
            .default_width(216.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(16),
            )
            .show(ctx, |ui| {
                ui.add_space(18.0);
                if self.ui.page == Page::Project {
                    if ui.button("Back to projects").clicked() {
                        self.ui.page = Page::Projects;
                    }
                    ui.add_space(15.0);
                    if let Some(id) = self.selected()
                        && let Ok(p) = self.engine.project(id).cloned()
                    {
                        for stage in Stage::ALL {
                            let suffix = if stage == Stage::Translation && !p.settings.translate {
                                "  · off"
                            } else if stage.index() < p.stage.index() {
                                "  · done"
                            } else if stage == p.stage && p.status == Status::Waiting {
                                "  · your turn"
                            } else {
                                ""
                            };
                            let label = if stage == Stage::Transcript && !p.source.media {
                                "Text"
                            } else {
                                stage.label()
                            };
                            if ui
                                .add_sized(
                                    [ui.available_width(), 34.0],
                                    egui::Button::selectable(
                                        self.engine.state.selected_stage == stage,
                                        format!("{label}{suffix}"),
                                    )
                                    .right_text(""),
                                )
                                .clicked()
                            {
                                self.engine.state.selected_stage = stage;
                                self.edit_text = false;
                            }
                        }
                        ui.separator();
                        ui.small(format!("{}\n{}", p.source.language, p.deck));
                    }
                } else {
                    for (page, label) in [
                        (Page::Projects, "Projects"),
                        (Page::Profiles, "Languages & decks"),
                        (Page::Resources, "Tools & downloads"),
                    ] {
                        if ui
                            .add_sized(
                                [ui.available_width(), 34.0],
                                egui::Button::selectable(self.ui.page == page, label)
                                    .right_text(""),
                            )
                            .clicked()
                        {
                            self.ui.page = page;
                        }
                    }
                    ui.add_space(20.0);
                    if ui.button("Settings").clicked() {
                        self.settings_open = true;
                    }
                }
                ui.add_space(22.0);
                ui.small("One step at a time.\nYou can leave and return.");
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(28),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("page_scroll")
                    .show(ui, |ui| {
                        ui.add_space(16.0);
                        if !self.error.is_empty() {
                            card(ui, |ui| {
                                ui.colored_label(
                                    Color32::from_rgb(180, 61, 40),
                                    "Needs your attention",
                                );
                                ui.label(&self.error);
                            });
                            if ui.small_button("Dismiss message").clicked() {
                                self.error.clear();
                            }
                        }
                        if !self.notice.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(&self.notice);
                                if ui.small_button("Dismiss").clicked() {
                                    self.notice.clear();
                                }
                            });
                        }
                        match self.ui.page {
                            Page::Projects => self.projects(ui),
                            Page::Setup => self.setup(ui),
                            Page::Project => self.project(ui),
                            Page::Profiles => self.profiles(ui),
                            Page::Resources => self.resources(ui),
                        }
                        ui.add_space(24.0);
                    });
            });
        self.windows(ctx);
    }
    fn windows(&mut self, ctx: &egui::Context) {
        if self.settings_open {
            let closed = secondary_window(
                ctx,
                "settings",
                "Blitzline settings",
                [900.0, 760.0],
                |ui| self.settings_ui(ui),
            );
            if closed {
                self.settings_open = false;
            }
        }
        if let Some(id) = self.compact {
            let closed = secondary_window(
                ctx,
                "assistant",
                "Blitzline · Chat assistant",
                [820.0, 740.0],
                |ui| {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                            if self.compact_top {
                                egui::WindowLevel::AlwaysOnTop
                            } else {
                                egui::WindowLevel::Normal
                            },
                        ));
                    if let Ok(p) = self.engine.project(id).cloned() {
                        metadata(ui, &p.name);
                        self.assistant(ui, &p);
                    } else {
                        self.compact = None;
                    }
                    if !self.error.is_empty() {
                        ui.colored_label(ui.visuals().error_fg_color, &self.error);
                    }
                    if !self.notice.is_empty() {
                        ui.label(&self.notice);
                    }
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        checkbox(ui, &mut self.compact_top, "Keep above other windows");
                        if ui.button("Back to full project").clicked() {
                            self.open_project(id);
                            self.compact = None;
                        }
                    });
                },
            );
            if closed {
                self.compact = None;
            }
        }
        if self.design_open {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("design"),
                egui::ViewportBuilder::default()
                    .with_title("Blitzline · Card design")
                    .with_inner_size([900.0, 740.0]),
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        egui::Window::new("Card design").show(ctx, |ui| self.design_ui(ui));
                    } else {
                        egui::CentralPanel::default().show(ctx, |ui| self.design_ui(ui));
                        if ctx.input(|i| i.viewport().close_requested()) {
                            self.design_open = false;
                        }
                    }
                },
            );
        }
        if self.help_open {
            let closed = secondary_window(ctx, "help", "Blitzline Help", [680.0, 640.0], |ui| {
                title(ui, "A little help", "Find your way, one step at a time.");
                ui.label("Choose material → choose help → review words → preview → export.");
                ui.add(text_input(&mut self.ui.help_search).hint_text("Search help"));
                for (title, body) in [
                    (
                        "Using a browser chat",
                        "Copy the prepared request, paste it into a new conversation on your chat website, then copy the complete reply back here. Check answer saves valid results. Follow-up requests repair only the missing work.",
                    ),
                    (
                        "API key",
                        "A secret issued by an AI service that lets an app make requests for you. A chat subscription and API access may be separate. This prototype makes no requests and never saves keys.",
                    ),
                    (
                        "Word family",
                        "A dictionary word and its connected forms: a family list for 'be' can exclude 'am' and 'are'. An exact spelling list skips only written forms.",
                    ),
                    (
                        "Getting cards into Anki",
                        "The finished product will save an APKG or use AnkiConnect. This prototype records simulated exports and can save a sample TSV.",
                    ),
                    (
                        "Transcription",
                        "Turns speech into text. Local processing requires a model; online processing sends audio to your service. Both are simulated here.",
                    ),
                    (
                        "Leaving and returning",
                        "Projects, pasted answers, and setup drafts are saved. Restarted processing is paused until you Continue.",
                    ),
                ] {
                    if format!("{title} {body}")
                        .to_lowercase()
                        .contains(&self.ui.help_search.to_lowercase())
                    {
                        ui.collapsing(title, |ui| {
                            ui.label(body);
                        });
                    }
                }
                if ui.button("Close help").clicked() {
                    self.help_open = false;
                }
            });
            if closed {
                self.help_open = false;
            }
        }
        if self.project_settings.is_some() {
            self.project_settings_window(ctx);
        }
        if self.confirm_text {
            egui::Window::new("Apply text changes?").collapsible(false).show(ctx,|ui|{
                ui.label("Translation, word review, and cards need rebuilding. Existing exports stay in history. Processing will pause.");
                if let Some(id)=self.selected()
                    && primary(ui,"Apply changes").clicked(){
                        let text=self.engine.project(id).unwrap().transcript_draft.clone();
                        self.run(|e|e.edit_transcript(id,text));self.confirm_text=false;self.edit_text=false;
                    }
                if ui.button("Keep editing").clicked(){self.confirm_text=false;}
            });
        }
        if self.confirm_cleanup {
            egui::Window::new("Clean up example project?").collapsible(false).show(ctx,|ui|{
                ui.label("This simulation clears review drafts and sample cards. Export receipts and source references remain. It cannot be resumed. No original file is removed.");
                if ui.button("Apply simulated cleanup").clicked(){
                    if let Some(id)=self.selected() {let p=self.engine.project_mut(id).unwrap();p.words.clear();p.exchange=None;p.history.clear();p.cleaned=true;}
                    self.confirm_cleanup=false;
                }
                if ui.button("Cancel").clicked(){self.confirm_cleanup=false;}
            });
        }
        if let Some(id) = self.confirm_delete {
            egui::Window::new("Delete this prototype project?").collapsible(false).show(ctx,|ui|{
                ui.label("Its saved review and history will be removed. The original file is never deleted.");
                if ui.button("Delete project").clicked(){
                    self.engine.state.projects.retain(|p|p.id!=id);self.confirm_delete=None;
                    if self.selected()==Some(id){self.engine.state.selected_project=None;self.ui.page=Page::Projects;}
                    if self.compact==Some(id){self.compact=None;}
                }
                if ui.button("Cancel").clicked(){self.confirm_delete=None;}
            });
        }
        if self.confirm_quit {
            egui::Window::new("Pause before leaving?")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(
                        "Projects and drafts will be saved. No work runs after the app closes.",
                    );
                    if primary(ui, "Pause and quit").clicked() {
                        for p in &mut self.engine.state.projects {
                            if matches!(p.status, Status::Running | Status::Queued) {
                                p.status = Status::Paused;
                            }
                        }
                        if self.save_now() {
                            self.quit_allowed = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    if ui.button("Keep working").clicked() {
                        self.confirm_quit = false;
                    }
                });
        }
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            for command in native.commands() {
                match command {
                    macos::Command::Projects => self.ui.page = Page::Projects,
                    macos::Command::New => self.ui.page = Page::Setup,
                    macos::Command::Open => self.pick_files(),
                    macos::Command::Import => self.import_settings(),
                    macos::Command::Larger | macos::Command::Smaller => {
                        let delta = if matches!(command, macos::Command::Larger) {
                            0.1
                        } else {
                            -0.1
                        };
                        self.engine.state.preferences.scale =
                            (self.engine.state.preferences.scale + delta).clamp(0.8, 1.7);
                        self.configure(ctx);
                    }
                    macos::Command::Settings => self.settings_open = true,
                    macos::Command::Help => self.help_open = true,
                    macos::Command::Appearance => self.toggle_appearance(ctx),
                }
            }
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            if let Some(path) = file.path {
                self.add_source(&path);
                self.ui.page = Page::Setup;
            }
        }
        if self.last_tick.elapsed() > Duration::from_millis(850) {
            self.engine.tick();
            self.last_tick = Instant::now();
            if self.playback {
                self.position = (self.position + 0.85).min(12.0);
                if self.position >= 12.0 {
                    self.playback = false;
                }
            }
        }
        if self
            .downloading
            .is_some_and(|s| s.elapsed() > Duration::from_secs(3))
        {
            self.engine.state.preferences.local_model_installed = true;
            self.downloading = None;
            self.notice = "Example model marked available. No model was downloaded.".into();
        }
        self.render(ctx);
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native {
            native.report_capture_window();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_allowed {
            if self
                .engine
                .state
                .projects
                .iter()
                .any(|p| matches!(p.status, Status::Running | Status::Queued))
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_quit = true;
            } else if !self.save_now() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.last_save.elapsed() > Duration::from_millis(600) {
            self.save_now();
            self.last_save = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
    }
}
pub(crate) fn text_input(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(egui::Margin::symmetric(12, 9))
}
pub(crate) fn text_area(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text).margin(egui::Margin::symmetric(12, 10))
}
pub(crate) fn metadata(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).weak().italics());
}
pub(crate) fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    ui.strong(label);
}
pub(crate) fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        let dark = ui.visuals().dark_mode;
        let accent = accent(ui);
        ui.spacing_mut().interact_size.y = 28.0;
        let visuals = ui.visuals_mut();
        visuals.selection.bg_fill = accent;
        visuals.widgets.inactive.bg_fill = if dark {
            Color32::from_rgb(83, 95, 115)
        } else {
            Color32::from_rgb(184, 196, 214)
        };
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(2.0, accent);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, accent);
        ui.add(slider)
    })
    .inner
}
pub(crate) fn checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    label: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.corner_radius = egui::CornerRadius::same(3);
        ui.visuals_mut().widgets.hovered.corner_radius = egui::CornerRadius::same(3);
        ui.visuals_mut().widgets.active.corner_radius = egui::CornerRadius::same(3);
        ui.checkbox(checked, label)
    })
    .inner
}
/// Native utility window at runtime; embedded equivalent for headless tests.
/// Returns true only when the window's close control was used.
pub(crate) fn secondary_window(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    mut content: impl FnMut(&mut egui::Ui),
) -> bool {
    let mut open = true;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of(id),
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_active(true)
            .with_inner_size(size)
            .with_min_inner_size([420.0, 360.0]),
        |ctx, class| {
            let mut body = |ui: &mut egui::Ui| {
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .show(ui, |ui| content(ui));
            };
            if class == egui::ViewportClass::Embedded {
                egui::Window::new(title)
                    .id(egui::Id::new(id))
                    .open(&mut open)
                    .default_size(size)
                    .show(ctx, &mut body);
            } else {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(ctx.style().visuals.panel_fill)
                            .inner_margin(24),
                    )
                    .show(ctx, &mut body);
                open = !ctx.input(|i| i.viewport().close_requested());
            }
        },
    );
    !open
}
pub(crate) fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(Color32::WHITE))
            .fill(ACCENT)
            .min_size(egui::vec2(0.0, 34.0)),
    )
}
pub(crate) fn card<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::group(ui.style())
        .fill(ui.visuals().window_fill)
        .corner_radius(14)
        .inner_margin(22)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui)
        })
        .inner
}
pub(crate) fn title(ui: &mut egui::Ui, heading: &str, subtitle: &str) {
    ui.heading(heading);
    ui.label(RichText::new(subtitle).weak().italics());
    ui.add_space(14.0);
}

fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(157, 190, 255)
    } else {
        ACCENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    fn harness(app: Desktop, size: [f32; 2]) -> Harness<'static, Desktop> {
        Harness::builder().with_size(size).build_state(
            |ctx, app: &mut Desktop| {
                app.configure(ctx);
                app.render(ctx);
            },
            app,
        )
    }
    fn click(h: &mut Harness<'_, Desktop>, label: &str) {
        h.get_by_label(label).click_accesskit();
        h.run_steps(3);
    }
    #[test]
    fn guided_setup_requires_help_choice_and_creates_project() {
        let mut h = harness(Desktop::new(None, Workspace::default()), [1240.0, 1000.0]);
        click(&mut h, "Start a project");
        h.state_mut().engine.state.setup.pasted_text = "Mačka spi.".into();
        h.run_steps(2);
        click(&mut h, "Continue");
        click(&mut h, "Continue");
        assert!(!h.state().engine.state.setup.method_chosen);
        click(&mut h, "Continue");
        assert!(h.state().error.contains("Choose"));
        click(&mut h, "Use a chat website");
        click(&mut h, "Continue");
        click(&mut h, "Start project");
        assert_eq!(h.state().engine.state.projects.len(), 1);
        assert!(h.state().ui.page == Page::Project);
    }
    #[test]
    fn appearance_switch_preserves_browser_draft() {
        let mut app = Desktop::new(None, Workspace::default());
        app.engine.state.preferences.appearance = "Light".into();
        app.load_scenario("assistant");
        let id = app.selected().unwrap();
        app.engine
            .project_mut(id)
            .unwrap()
            .exchange
            .as_mut()
            .unwrap()
            .draft = "My unfinished answer".into();
        let mut h = harness(app, [1240.0, 1600.0]);
        click(&mut h, "Light / dark");
        assert_eq!(h.state().engine.state.preferences.appearance, "Dark");
        click(&mut h, "Light / dark");
        assert_eq!(h.state().engine.state.preferences.appearance, "Light");
        assert_eq!(
            h.state()
                .engine
                .project(id)
                .unwrap()
                .exchange
                .as_ref()
                .unwrap()
                .draft,
            "My unfinished answer"
        );
    }
    #[test]
    fn assistant_accepts_partial_answer_through_accessible_controls() {
        let mut app = Desktop::new(None, Workspace::default());
        app.load_scenario("assistant");
        let id = app.selected().unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&app.engine.sample_reply(id).unwrap()).unwrap();
        value["decisions"].as_array_mut().unwrap().pop();
        app.engine
            .project_mut(id)
            .unwrap()
            .exchange
            .as_mut()
            .unwrap()
            .draft = value.to_string();
        let mut h = harness(app, [1240.0, 1600.0]);
        click(&mut h, "Open chat assistant");
        click(&mut h, "Check answer");
        assert_eq!(h.state().engine.project(id).unwrap().included(), 2);
        assert!(h.query_by_label("Copy a follow-up request").is_some());
        let repair = h.state().engine.sample_reply(id).unwrap();
        h.state_mut()
            .engine
            .project_mut(id)
            .unwrap()
            .exchange
            .as_mut()
            .unwrap()
            .draft = repair;
        h.run_steps(2);
        click(&mut h, "Check answer");
        click(&mut h, "Continue to the next step");
        assert!(h.state().engine.project(id).unwrap().exchange.is_none());
    }
    #[test]
    fn narrow_layout_card_preview_gates_export() {
        let mut app = Desktop::new(None, Workspace::default());
        app.load_scenario("cards");
        let id = app.selected().unwrap();
        let mut h = harness(app, [1024.0, 768.0]);
        assert!(
            h.state()
                .engine
                .project(id)
                .unwrap()
                .previewed_revision
                .is_none()
        );
        click(&mut h, "Show answer");
        assert!(h.query_by_label("cat").is_some());
        click(&mut h, "Continue to export");
        click(&mut h, "Simulate deck export");
        assert_eq!(
            h.state().engine.project(id).unwrap().status,
            Status::Finished
        );
    }
    #[test]
    fn session_key_is_not_persisted_with_settings_or_projects() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().into()).unwrap();
        let mut app = Desktop::new(Some(store), Workspace::default());
        app.secret = "MUST_NOT_PERSIST".into();
        app.save_now();
        for file in ["workspace.json", "ui.json"] {
            assert!(
                !std::fs::read_to_string(directory.path().join(file))
                    .unwrap()
                    .contains("MUST_NOT_PERSIST")
            );
        }
    }
    #[test]
    fn project_settings_remain_bound_to_the_opened_project() {
        let mut app = Desktop::new(None, Workspace::default());
        app.load_scenario("assistant");
        let original = app.selected().unwrap();
        app.project_settings = Some(original);
        app.project_options = Some((
            app.engine.project(original).unwrap().settings.clone(),
            "Renamed original".into(),
        ));
        let other = app.engine.example().unwrap();
        let other_name = app.engine.project(other).unwrap().name.clone();
        app.open_project(other);
        let mut h = harness(app, [1400.0, 1200.0]);
        click(&mut h, "Apply to this project");
        assert_eq!(
            h.state().engine.project(original).unwrap().name,
            "Renamed original"
        );
        assert_eq!(h.state().engine.project(other).unwrap().name, other_name);
        assert!(h.state().project_settings.is_none());
    }
    #[test]
    fn closing_assistant_retains_reply_for_reopening() {
        let mut app = Desktop::new(None, Workspace::default());
        app.load_scenario("assistant");
        let id = app.selected().unwrap();
        app.engine
            .project_mut(id)
            .unwrap()
            .exchange
            .as_mut()
            .unwrap()
            .draft = "Unfinished reply".into();
        let mut h = harness(app, [1400.0, 1200.0]);
        click(&mut h, "Open chat assistant");
        assert_eq!(h.state().compact, Some(id));
        click(&mut h, "Back to full project");
        assert_eq!(h.state().compact, None);
        click(&mut h, "Open chat assistant");
        assert_eq!(
            h.state()
                .engine
                .project(id)
                .unwrap()
                .exchange
                .as_ref()
                .unwrap()
                .draft,
            "Unfinished reply"
        );
    }
    #[test]
    fn search_reveals_advanced_settings() {
        let mut app = Desktop::new(None, Workspace::default());
        app.settings_open = true;
        app.ui.settings_search = "timeout".into();
        let h = harness(app, [1240.0, 1000.0]);
        assert!(h.query_by_label("Advanced connection settings").is_some());
    }
}
