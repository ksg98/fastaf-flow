use eframe::egui::{self, Color32, RichText, Vec2};
use fastaf_flow::{
    audio::Recording,
    catalog::{self, Kind, Memory, Model},
    runtime::{self, Event, Service},
    settings::Settings,
    system,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const INK: Color32 = Color32::from_rgb(22, 33, 59);
const MUTED: Color32 = Color32::from_rgb(89, 103, 124);
const BLUE: Color32 = Color32::from_rgb(50, 92, 223);
const RED: Color32 = Color32::from_rgb(188, 52, 81);
const PAPER: Color32 = Color32::from_rgb(248, 250, 252);

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Dictate,
    Models,
    Setup,
}
#[derive(Clone, Copy, PartialEq)]
enum JobKind {
    Setup,
    Check,
    Transcribe,
    Rewrite,
    Download,
    Catalog,
    Unload,
}
struct Job {
    id: u64,
    kind: JobKind,
    started: Instant,
}

pub struct FlowApp {
    settings: Settings,
    service: Service,
    models: Vec<Model>,
    memory: Memory,
    memory_checked: Instant,
    tab: Tab,
    recording: Option<Recording>,
    audio_file: Option<tempfile::NamedTempFile>,
    job: Option<Job>,
    next_id: u64,
    text: String,
    raw: String,
    status: String,
    error: Option<String>,
    target_pid: Option<i32>,
    query: String,
    downloaded_only: bool,
    library_kind: Kind,
    ready: bool,
    elapsed: f64,
    #[cfg(target_os = "macos")]
    desktop: crate::desktop::Desktop,
    hotkey_down: bool,
    quitting: bool,
    #[cfg(feature = "screenshot")]
    capture_requested: bool,
}
impl FlowApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut fonts = egui::FontDefinitions::default();
        for (name, path) in [
            ("system", "/System/Library/Fonts/SFNS.ttf"),
            ("symbols", "/System/Library/Fonts/Apple Symbols.ttf"),
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert(name.into(), egui::FontData::from_owned(bytes).into());
                let family = fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default();
                if name == "system" {
                    family.insert(0, name.into());
                } else {
                    family.push(name.into());
                }
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = PAPER;
        visuals.window_fill = Color32::WHITE;
        visuals.override_text_color = Some(INK);
        visuals.selection.bg_fill = Color32::from_rgb(219, 228, 255);
        visuals.selection.stroke = egui::Stroke::new(1.0, BLUE);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = Vec2::new(10.0, 12.0);
            style.spacing.button_padding = Vec2::new(12.0, 8.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(28.0));
        });
        let (settings, mut error) = match Settings::load() {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(e.to_string())),
        };
        #[cfg(target_os = "macos")]
        let desktop = {
            let (desktop, desktop_error) = crate::desktop::Desktop::new(&cc.egui_ctx);
            if desktop_error.is_some() {
                error = desktop_error;
            }
            desktop
        };
        let models = catalog::all_models(&settings.model_roots);
        let memory = Memory::read();
        let mut app = Self {
            ready: false,
            settings,
            service: Service::new(move || ctx.request_repaint()),
            models,
            memory,
            memory_checked: Instant::now(),
            tab: Tab::Dictate,
            recording: None,
            audio_file: None,
            job: None,
            next_id: 0,
            text: String::new(),
            raw: String::new(),
            status: "Ready when you are".into(),
            error,
            target_pid: None,
            query: String::new(),
            downloaded_only: false,
            library_kind: Kind::Speech,
            elapsed: 0.0,
            #[cfg(target_os = "macos")]
            desktop,
            hotkey_down: false,
            quitting: false,
            #[cfg(feature = "screenshot")]
            capture_requested: false,
        };
        app.select_defaults();
        if runtime::installed() {
            app.submit(JobKind::Check, json!({"op": "ping"}));
        } else {
            app.tab = Tab::Setup;
        }
        #[cfg(feature = "screenshot")]
        match std::env::var("FASTAF_PREVIEW_TAB").as_deref() {
            Ok("models") => {
                app.tab = Tab::Models;
                app.library_kind = Kind::Rewrite;
            }
            Ok("setup") => app.tab = Tab::Setup,
            _ => {}
        }
        app
    }
    fn select_defaults(&mut self) {
        for kind in [Kind::Speech, Kind::Rewrite] {
            let selected = if kind == Kind::Speech {
                &mut self.settings.speech_model
            } else {
                &mut self.settings.rewrite_model
            };
            if !self
                .models
                .iter()
                .any(|m| m.id == *selected && m.installed())
                && let Some(model) =
                    catalog::recommend(&self.models, kind, self.memory).filter(|m| m.installed())
            {
                *selected = model.id.clone();
            }
        }
    }
    fn save(&mut self) {
        if let Err(e) = self.settings.save() {
            self.error = Some(format!("Could not save settings: {e}"));
        }
    }
    fn reload(&mut self) {
        self.models = catalog::all_models(&self.settings.model_roots);
        self.memory = Memory::read();
        self.select_defaults();
    }
    fn submit(&mut self, kind: JobKind, body: Value) {
        self.next_id += 1;
        self.error = None;
        self.status = match kind {
            JobKind::Setup => "Installing local runtime…",
            JobKind::Check => "Checking MLX runtime…",
            JobKind::Transcribe => "Transcribing locally…",
            JobKind::Rewrite => "Rewriting locally…",
            JobKind::Download => "Starting download…",
            JobKind::Catalog => "Refreshing model library…",
            JobKind::Unload => "Releasing model memory…",
        }
        .into();
        self.job = Some(Job {
            id: self.next_id,
            kind,
            started: Instant::now(),
        });
        self.service.submit(self.next_id, body);
    }
    fn selected(&self, kind: Kind) -> Option<&Model> {
        let id = if kind == Kind::Speech {
            &self.settings.speech_model
        } else {
            &self.settings.rewrite_model
        };
        self.models.iter().find(|m| &m.id == id && m.installed())
    }
    fn keep_models(&self) -> bool {
        let total: f64 = [Kind::Speech, Kind::Rewrite]
            .iter()
            .filter_map(|k| self.selected(*k))
            .map(Model::estimated_gb)
            .sum();
        total < self.memory.budget()
    }
    fn toggle_recording(&mut self, from_shortcut: bool) {
        if self.job.is_some() {
            return;
        }
        if let Some(recording) = self.recording.take() {
            match recording.stop() {
                Ok(file) => {
                    self.audio_file = Some(file);
                    self.transcribe();
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.status = "Recording stopped".into();
                }
            }
            return;
        }
        if !self.ready {
            self.tab = Tab::Setup;
            self.error = Some("Install and check the local runtime first.".into());
            return;
        }
        if self.selected(Kind::Speech).is_none() {
            self.tab = Tab::Models;
            self.error = Some("Download a speech model or add an existing model folder.".into());
            return;
        }
        self.target_pid = if from_shortcut {
            system::frontmost_pid().filter(|p| *p != std::process::id() as i32)
        } else {
            None
        };
        match Recording::start() {
            Ok(recording) => {
                self.recording = Some(recording);
                self.error = None;
                self.status = "Listening…".into();
            }
            Err(e) => {
                self.error = Some(format!("{e:#}"));
                self.status = "Microphone unavailable".into();
            }
        }
    }
    fn transcribe(&mut self) {
        let Some(model) = self.selected(Kind::Speech) else {
            return;
        };
        let Some(file) = &self.audio_file else {
            return;
        };
        self.submit(JobKind::Transcribe, json!({"op": "transcribe", "model": model.path,
            "audio": file.path(), "language": self.settings.language, "keep_models": self.keep_models()}));
    }
    fn rewrite(&mut self) {
        let Some(model) = self.selected(Kind::Rewrite) else {
            self.error = Some("Download S1-mini by Superwhisper in Models to enable rewriting. Your transcript is preserved.".into());
            self.status = "Transcript ready".into();
            return;
        };
        self.submit(JobKind::Rewrite, json!({"op": "rewrite", "model": model.path, "text": self.text,
            "styling": self.settings.styling, "structure": self.settings.structure, "context": self.settings.context,
            "keep_models": self.keep_models()}));
    }
    fn complete_text(&mut self) {
        if self.text.is_empty() {
            self.status = "No speech to insert".into();
            self.target_pid = None;
            return;
        }
        self.status = format!("Ready · {:.1}s processing", self.elapsed);
        if self.settings.auto_paste
            && let Some(pid) = self.target_pid.take()
        {
            match system::copy_text(&self.text).and_then(|_| system::paste_to(pid)) {
                Ok(()) => self.status = "Inserted into your app".into(),
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
    fn events(&mut self) {
        while let Ok(event) = self.service.rx.try_recv() {
            let id = match &event {
                Event::Progress(i, _) | Event::Done(i, _) | Event::Error(i, _) => *i,
            };
            if self.job.as_ref().is_none_or(|j| j.id != id) {
                continue;
            }
            match event {
                Event::Progress(_, message) => self.status = message,
                Event::Error(_, message) => {
                    self.job = None;
                    self.audio_file = None;
                    self.target_pid = None;
                    self.error = Some(message);
                    self.status = "Stopped · your text is preserved".into();
                }
                Event::Done(_, value) => {
                    let job = self.job.take().unwrap();
                    match job.kind {
                        JobKind::Setup => self.submit(JobKind::Check, json!({"op": "ping"})),
                        JobKind::Check => {
                            self.ready = value["ready"].as_bool() == Some(true);
                            if self.ready {
                                self.status = "Local MLX runtime ready".into();
                            } else {
                                self.error = Some(
                                    "MLX cannot access Metal. Use an Apple Silicon Mac.".into(),
                                );
                            }
                        }
                        JobKind::Transcribe => {
                            self.audio_file = None;
                            self.text = value["text"].as_str().unwrap_or("").to_string();
                            self.raw = self.text.clone();
                            self.elapsed = job.started.elapsed().as_secs_f64();
                            let detected =
                                value["language"].as_str().unwrap_or("en").to_lowercase();
                            let english = ["auto", "en"].contains(&self.settings.language.as_str())
                                && ["en", "english"].contains(&detected.as_str());
                            if self.settings.auto_rewrite && english && !self.text.is_empty() {
                                self.rewrite();
                            } else {
                                self.complete_text();
                            }
                        }
                        JobKind::Rewrite => {
                            self.text = value["text"].as_str().unwrap_or("").to_string();
                            self.elapsed += job.started.elapsed().as_secs_f64();
                            self.complete_text();
                        }
                        JobKind::Download => {
                            self.reload();
                            self.save();
                            self.status = "Download complete. Select the model in Dictate.".into();
                        }
                        JobKind::Catalog => {
                            if let Some(models) = value.get("models") {
                                match std::fs::create_dir_all(fastaf_flow::settings::data_dir())
                                    .and_then(|_| {
                                        std::fs::write(
                                            fastaf_flow::settings::data_dir().join("catalog.json"),
                                            serde_json::to_vec_pretty(models)?,
                                        )
                                    }) {
                                    Ok(()) => {
                                        self.reload();
                                        self.status = "Model library refreshed".into();
                                    }
                                    Err(e) => {
                                        self.error =
                                            Some(format!("Could not save model library: {e}"))
                                    }
                                }
                            }
                        }
                        JobKind::Unload => self.status = "Model memory released".into(),
                    }
                }
            }
        }
    }
    fn cancel(&mut self) {
        self.recording = None;
        self.job = None;
        self.audio_file = None;
        self.target_pid = None;
        self.service.cancel();
        self.status = "Cancelled · your text is preserved".into();
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(275.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(22))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.add_space(8.0);
                    ui.label(RichText::new("FastAF Flow").size(27.0).strong());
                    ui.label(RichText::new("Your voice. On your Mac.").color(MUTED));
                    ui.add_space(22.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.tab, Tab::Dictate, "Dictate");
                        ui.selectable_value(&mut self.tab, Tab::Models, "Models");
                        ui.selectable_value(&mut self.tab, Tab::Setup, "Setup");
                    });
                    ui.add_space(20.0);
                    let enabled = self.job.is_none() && self.recording.is_none();
                    ui.add_enabled_ui(enabled, |ui| {
                        ui.label(RichText::new("Speech model").strong());
                        model_picker(
                            ui,
                            "speech",
                            &self.models,
                            Kind::Speech,
                            &mut self.settings.speech_model,
                        );
                        ui.add_space(8.0);
                        ui.label(RichText::new("S1-mini by Superwhisper").strong());
                        model_picker(
                            ui,
                            "rewrite",
                            &self.models,
                            Kind::Rewrite,
                            &mut self.settings.rewrite_model,
                        );
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.settings.auto_rewrite, "Clean up after dictation");
                        ui.label(
                            RichText::new("S1-mini rewrites English.")
                                .size(12.0)
                                .color(MUTED),
                        );
                        ui.add_space(6.0);
                        choice(
                            ui,
                            "Style",
                            &mut self.settings.styling,
                            &[
                                ("casual", "Casual"),
                                ("semi-casual", "Relaxed"),
                                ("semi-formal", "Natural"),
                                ("formal", "Formal"),
                            ],
                        );
                        choice(
                            ui,
                            "Structure",
                            &mut self.settings.structure,
                            &[("prose", "Paragraphs"), ("lists", "Lists")],
                        );
                        choice(
                            ui,
                            "Context",
                            &mut self.settings.context,
                            &[("general", "General"), ("email", "Email")],
                        );
                    });
                    ui.add_space(18.0);
                    ui.separator();
                    ui.label(
                        RichText::new(format!("{:.0} GB unified memory", self.memory.total_gb))
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{:.1} GB currently available",
                            self.memory.available_gb
                        ))
                        .size(13.0)
                        .color(MUTED),
                    );
                    let combined: f64 = [Kind::Speech, Kind::Rewrite]
                        .iter()
                        .filter_map(|k| self.selected(*k))
                        .map(Model::estimated_gb)
                        .sum();
                    ui.label(
                        RichText::new(format!("Selected pair: ~{combined:.1} GB"))
                            .size(13.0)
                            .color(MUTED),
                    );
                    if combined > self.memory.budget() {
                        ui.label(
                            RichText::new("Memory is tight. Models will run one at a time.")
                                .size(12.0)
                                .color(RED),
                        );
                    }
                    ui.add_space(6.0);
                });
            });
    }
    fn dictate(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Speak freely.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.job.is_none() && self.recording.is_none(),
                        egui::Button::new("Import WAV"),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("WAV audio", &["wav"])
                        .pick_file()
                {
                    self.import_wav(path);
                }
            });
        });
        ui.label(RichText::new("A thought, a message, a first draft. Start talking.").color(MUTED));
        ui.add_space(16.0);
        let recording = self.recording.is_some();
        let button = egui::Button::new(
            RichText::new(if recording {
                "Finish recording"
            } else {
                "Start recording"
            })
            .size(18.0)
            .color(Color32::WHITE),
        )
        .fill(if recording { RED } else { BLUE })
        .min_size(Vec2::new(ui.available_width(), 64.0));
        if ui.add_enabled(self.job.is_none(), button).clicked() {
            self.toggle_recording(false);
        }
        if let Some(recording) = &self.recording {
            let elapsed = recording.elapsed().as_secs();
            let db = 20.0 * recording.level().max(0.00001).log10();
            let level = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            ui.add(egui::ProgressBar::new(level).fill(RED).text(format!(
                "{}:{:02} · {}",
                elapsed / 60,
                elapsed % 60,
                recording.device
            )));
        } else {
            ui.label(
                RichText::new("Or use ⌘⇧Space from any app. Up to 5 minutes per recording.")
                    .size(12.0)
                    .color(MUTED),
            );
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Your text").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.text.is_empty(), egui::Button::new("Copy"))
                    .clicked()
                {
                    match system::copy_text(&self.text) {
                        Ok(()) => self.status = "Copied to clipboard".into(),
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                if ui
                    .add_enabled(
                        !self.text.is_empty() && self.job.is_none(),
                        egui::Button::new("Rewrite"),
                    )
                    .clicked()
                {
                    self.raw = self.text.clone();
                    self.target_pid = None;
                    self.elapsed = 0.0;
                    self.rewrite();
                }
                if ui
                    .add_enabled(
                        !self.raw.is_empty() && self.job.is_none(),
                        egui::Button::new("Restore raw"),
                    )
                    .clicked()
                {
                    self.text = self.raw.clone();
                }
            });
        });
        let height = (ui.available_height() - 58.0).max(150.0);
        egui::Frame::new().fill(Color32::WHITE).corner_radius(10).inner_margin(18)
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(219, 225, 235)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).show(ui, |ui| {
                    ui.add_enabled(self.job.is_none(), egui::TextEdit::multiline(&mut self.text)
                        .id_source("transcript").font(egui::FontId::proportional(20.0)).frame(false)
                        .desired_width(f32::INFINITY).desired_rows(10)
                        .hint_text("Your words will appear here.\n\nYou can also paste text and choose Rewrite."));
                });
            });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} words", self.text.split_whitespace().count()))
                    .size(12.0)
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Clear"))
                    .clicked()
                {
                    self.text.clear();
                    self.raw.clear();
                    self.target_pid = None;
                }
                ui.label(
                    RichText::new("Text stays in this session")
                        .size(12.0)
                        .color(MUTED),
                );
            });
        });
    }
    fn import_wav(&mut self, path: PathBuf) {
        if !self.ready || self.selected(Kind::Speech).is_none() {
            self.error =
                Some("Set up the runtime and select a downloaded speech model first.".into());
            return;
        }
        // Do not hold arbitrary user files open or ever delete their source file.
        match (|| -> anyhow::Result<tempfile::NamedTempFile> {
            let metadata = std::fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= 150_000_000,
                "WAV file is too large. Import up to five minutes."
            );
            let file = tempfile::Builder::new()
                .prefix("fastaf-flow-")
                .suffix(".wav")
                .tempfile()?;
            std::fs::copy(path, file.path())?;
            Ok(file)
        })() {
            Ok(file) => {
                self.audio_file = Some(file);
                self.target_pid = None;
                self.transcribe();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn library(&mut self, ui: &mut egui::Ui) {
        ui.heading("Choose your local models");
        ui.label(RichText::new("Downloads are shared with your Hugging Face cache.").color(MUTED));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.library_kind, Kind::Speech, "Speech recognition");
            ui.selectable_value(
                &mut self.library_kind,
                Kind::Rewrite,
                "S1-mini by Superwhisper",
            );
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(220.0)
                    .hint_text("Search models…"),
            );
            ui.checkbox(&mut self.downloaded_only, "Downloaded");
        });
        let idle = self.job.is_none() && self.recording.is_none();
        ui.add_enabled_ui(idle, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Rescan local").clicked() {
                    self.reload();
                    self.status = "Local models rescanned".into();
                }
                if ui.button("Refresh online").clicked() {
                    self.submit(JobKind::Catalog, json!({"op": "catalog"}));
                }
                if ui.button("Add folder").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    if !self.settings.model_roots.contains(&folder) {
                        self.settings.model_roots.push(folder);
                    }
                    self.reload();
                    self.save();
                }
            });
        });
        if self.library_kind == Kind::Rewrite {
            ui.label(RichText::new("All published MLX variants: 4-bit, 8-bit, BF16. Custom local quants are discovered too.").size(13.0).color(MUTED));
        }
        if let Some(recommended) = catalog::recommend(&self.models, self.library_kind, self.memory)
        {
            ui.label(
                RichText::new(format!("Suggested for this Mac: {}", recommended.title()))
                    .color(BLUE),
            );
        } else {
            ui.colored_label(RED, "Free some memory before loading a model.");
        }
        ui.label(RichText::new("Memory values are estimates, including runtime overhead. Unfamiliar models may need extra dependencies.").size(12.0).color(MUTED));
        ui.separator();
        let query = self.query.to_lowercase();
        let visible: Vec<Model> = self
            .models
            .iter()
            .filter(|m| {
                m.kind == self.library_kind
                    && (!self.downloaded_only || m.installed())
                    && m.id.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if visible.is_empty() {
                    ui.label(
                        "No matching models. Rescan, add a model folder, or refresh the library.",
                    );
                }
                for model in visible {
                    ui.push_id(&model.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width((ui.available_width() - 130.0).max(180.0));
                                ui.label(RichText::new(model.title()).strong())
                                    .on_hover_text(&model.id);
                                ui.label(RichText::new(model.detail()).size(12.0).color(MUTED));
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if model.installed() {
                                        if ui
                                            .add_enabled(idle, egui::Button::new("Use model"))
                                            .clicked()
                                        {
                                            if model.kind == Kind::Speech {
                                                self.settings.speech_model = model.id.clone();
                                            } else {
                                                self.settings.rewrite_model = model.id.clone();
                                            }
                                            self.save();
                                            self.tab = Tab::Dictate;
                                        }
                                    } else if ui
                                        .add_enabled(
                                            idle && self.ready,
                                            egui::Button::new("Download"),
                                        )
                                        .clicked()
                                    {
                                        self.submit(
                                            JobKind::Download,
                                            json!({"op": "download", "repo": model.id}),
                                        );
                                    }
                                },
                            );
                        });
                        ui.separator();
                    });
                }
            });
    }
    fn setup(&mut self, ui: &mut egui::Ui) {
        ui.heading("Make yourself at home");
        ui.label(RichText::new("One-time setup. Then your voice stays on this Mac.").color(MUTED));
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(18.0);
            ui.label(RichText::new("Local runtime").size(19.0).strong());
            ui.label(if self.ready { "MLX Audio and MLX-LM are ready." } else { "Install Python and the MLX libraries into FastAF Flow's own environment." });
            ui.label(RichText::new("First setup needs internet and disk space for the runtime. Models are separate downloads.").size(13.0).color(MUTED));
            ui.add_enabled_ui(self.job.is_none(), |ui| {
                ui.horizontal(|ui| {
                    if ui.button(if runtime::installed() { "Repair runtime" } else { "Install runtime" }).clicked() {
                        self.submit(JobKind::Setup, json!({"op": "setup"}));
                    }
                    if ui.button("Check runtime").clicked() { self.submit(JobKind::Check, json!({"op": "ping"})); }
                    if ui.button("Choose models").clicked() { self.tab = Tab::Models; }
                });
            });
            ui.add_space(12.0);
            ui.separator();
            ui.label(RichText::new("Dictation").size(19.0).strong());
            ui.add_enabled_ui(self.job.is_none() && self.recording.is_none(), |ui| {
                ui.checkbox(&mut self.settings.hold_to_talk, "Hold ⌘⇧Space to talk; release to finish");
                ui.checkbox(&mut self.settings.auto_paste, "Insert finished dictation into the active app");
                ui.label(RichText::new("Automatic insertion applies to shortcut recordings. It replaces the clipboard and requires Accessibility access.").size(13.0).color(MUTED));
                choice(ui, "Language", &mut self.settings.language, &[("auto", "Detect automatically"), ("en", "English"), ("es", "Spanish"), ("fr", "French"),
                    ("de", "German"), ("it", "Italian"), ("pt", "Portuguese"), ("ja", "Japanese"), ("zh", "Chinese"), ("ko", "Korean"), ("hi", "Hindi")]);
                ui.label(RichText::new("Language hints depend on the speech model. Choose a specific non-English language to skip S1-mini cleanup.").size(13.0).color(MUTED));
            });
            ui.horizontal(|ui| {
                if ui.button("Microphone settings").clicked() { system::open_privacy(true); }
                if ui.button("Accessibility settings").clicked() { system::open_privacy(false); }
            });
            ui.label(RichText::new(if system::accessibility_allowed() { "Accessibility: allowed" } else { "Accessibility: not yet allowed (recording and copying still work)" }).size(13.0).color(MUTED));
            #[cfg(target_os = "macos")]
            if !self.desktop.shortcut_ready { ui.colored_label(RED, "Global shortcut unavailable. Another app may be using ⌘⇧Space."); }
            ui.add_space(12.0);
            ui.separator();
            ui.label(RichText::new("Privacy & memory").size(19.0).strong());
            ui.label("No accounts, telemetry, cloud inference, or saved transcript history. Temporary recordings are deleted after processing or cancellation.");
            ui.label(RichText::new("Models remain loaded for faster repeated dictation when memory allows.").size(13.0).color(MUTED));
            if ui.add_enabled(self.job.is_none() && self.recording.is_none(), egui::Button::new("Unload models")).clicked() {
                self.submit(JobKind::Unload, json!({"op": "unload"}));
            }
            ui.add_space(12.0);
            ui.label("Open source under the MIT license. S1-mini by Superwhisper has its own model license. Model weights are downloaded from their publishers.");
            ui.hyperlink_to("Source, setup guide & releases", "https://github.com/ksg98/fastaf-flow");
            ui.hyperlink_to("S1-mini by Superwhisper · model & license", "https://huggingface.co/superwhisper/s1-mini");
        });
    }
}
impl eframe::App for FlowApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.events();
        #[cfg(feature = "screenshot")]
        if let Ok(path) = std::env::var("FASTAF_SCREENSHOT_TO") {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                    image::save_buffer(
                        &path,
                        &pixels,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    )
                    .expect("save screenshot");
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if self.job.is_none() && !self.capture_requested {
                self.capture_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        #[cfg(target_os = "macos")]
        {
            while let Ok(event) = self.desktop.receiver.try_recv() {
                use crate::desktop::Event as NativeEvent;
                use global_hotkey::HotKeyState;
                match event {
                    NativeEvent::Shortcut(HotKeyState::Pressed) if !self.hotkey_down => {
                        self.hotkey_down = true;
                        if !self.settings.hold_to_talk || self.recording.is_none() {
                            self.toggle_recording(true);
                        }
                    }
                    NativeEvent::Shortcut(HotKeyState::Released) => {
                        self.hotkey_down = false;
                        if self.settings.hold_to_talk && self.recording.is_some() {
                            self.toggle_recording(true);
                        }
                    }
                    NativeEvent::Record => self.toggle_recording(true),
                    NativeEvent::Quit => {
                        self.cancel();
                        self.quitting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    _ => {}
                }
            }
            self.desktop.recording(self.recording.is_some());
            if ctx.input(|i| i.viewport().close_requested())
                && self.desktop.tray_ready
                && !self.quitting
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if self.recording.as_ref().is_some_and(|r| {
            r.elapsed().as_secs() >= fastaf_flow::audio::MAX_SECONDS || r.error().is_some()
        }) {
            self.toggle_recording(false);
        }
        // Timeouts make a stuck worker recoverable; downloads can take longer.
        if self.job.as_ref().is_some_and(|j| {
            j.started.elapsed()
                > Duration::from_secs(if matches!(j.kind, JobKind::Download | JobKind::Setup) {
                    3600
                } else {
                    600
                })
        }) {
            self.cancel();
            self.error = Some(
                "Operation timed out. Your text is preserved. Try a smaller model or retry.".into(),
            );
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape))
            && (self.job.is_some() || self.recording.is_some())
        {
            self.cancel();
        }
        if self.memory_checked.elapsed() > Duration::from_secs(30) {
            self.memory = Memory::read();
            self.memory_checked = Instant::now();
        }
        // Native events wake the UI; do not render continuously while idle.
        ctx.request_repaint_after(if self.recording.is_some() {
            Duration::from_millis(33)
        } else if self.job.is_some() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(30)
        });
        let before = serde_json::to_string(&self.settings).unwrap_or_default();
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(12))
            .show(ctx, |ui| {
                if let Some(error) = self.error.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(RED, error);
                        if ui.small_button("Dismiss").clicked() {
                            self.error = None;
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if self.job.is_some() {
                        ui.spinner();
                    }
                    ui.add(egui::Label::new(RichText::new(&self.status).size(13.0)).truncate());
                    if (self.job.is_some() || self.recording.is_some())
                        && ui.small_button("Cancel · Esc").clicked()
                    {
                        self.cancel();
                    }
                });
            });
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(PAPER).inner_margin(28))
            .show(ctx, |ui| match self.tab {
                Tab::Dictate => self.dictate(ui),
                Tab::Models => self.library(ui),
                Tab::Setup => self.setup(ui),
            });
        if serde_json::to_string(&self.settings).unwrap_or_default() != before {
            self.save();
        }
    }
}
fn model_picker(
    ui: &mut egui::Ui,
    salt: &str,
    models: &[Model],
    kind: Kind,
    selected: &mut String,
) {
    let installed: Vec<&Model> = models
        .iter()
        .filter(|m| m.kind == kind && m.installed())
        .collect();
    let title = installed
        .iter()
        .find(|m| m.id == *selected)
        .map(|m| m.title())
        .unwrap_or("Choose downloaded model");
    egui::ComboBox::from_id_salt(salt)
        .selected_text(title)
        .width(227.0)
        .show_ui(ui, |ui| {
            if installed.is_empty() {
                ui.label("No downloaded models. Open Models to download.");
            }
            for model in installed {
                ui.selectable_value(
                    selected,
                    model.id.clone(),
                    format!("{}  ({})", model.title(), model.quant),
                )
                .on_hover_text(&model.id);
            }
        });
    if let Some(m) = models.iter().find(|m| m.id == *selected && m.installed()) {
        ui.label(
            RichText::new(format!("{} · ~{:.1} GB memory", m.quant, m.estimated_gb()))
                .size(12.0)
                .color(MUTED),
        );
    }
}
fn choice(ui: &mut egui::Ui, label: &str, selected: &mut String, values: &[(&str, &str)]) {
    ui.horizontal(|ui| {
        ui.label(label);
        let text = values
            .iter()
            .find(|(id, _)| *id == selected)
            .map(|(_, title)| *title)
            .unwrap_or(selected.as_str());
        egui::ComboBox::from_id_salt(label)
            .selected_text(text)
            .width(130.0)
            .show_ui(ui, |ui| {
                for (value, title) in values {
                    ui.selectable_value(selected, value.to_string(), *title);
                }
            });
    });
}
