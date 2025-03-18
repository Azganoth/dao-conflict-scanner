use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    {mem, thread},
};

use anyhow::{Context, Error as AnyhowError, Result as AnyhowResult, anyhow};
use directories::UserDirs;
use eframe::egui;
use pathdiff::diff_paths;

use crate::{
    config::AppConfig,
    scanner::{Conflicts, ScanError, scan_for_conflicts},
    utils::{delete, open_in_explorer},
};

const BUTTON_RADIUS: f32 = 3.0;

fn setup_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut(|style| {
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.hovered.expansion = 0.0;
    });
}

pub struct App {
    config: AppConfig,
    conflicts: Conflicts,
    status: String,
    error: Option<AnyhowError>,
    pending_commands: Vec<Command>,
    expanded_conflicts: HashSet<String>,
    scan_thread: Option<thread::JoinHandle<()>>,
    receiver: Option<mpsc::Receiver<Result<Conflicts, ScanError>>>,
    has_scanned: bool,
}

#[derive(Debug)]
enum Command {
    IgnoreConflict(String, Vec<PathBuf>),
    UnignoreConflict(String),
    DeleteConflictFile(String, PathBuf),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_theme(&cc.egui_ctx);

        Self {
            config: AppConfig::load(),
            conflicts: Conflicts::new(),
            status: "Waiting for a scan...".into(),
            error: None,
            scan_thread: None,
            receiver: None,
            pending_commands: Vec::new(),
            expanded_conflicts: HashSet::new(),
            has_scanned: false,
        }
    }

    fn start_scan(&mut self, bioware_dir: &Path) {
        self.has_scanned = true;
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);

        let game_dir = bioware_dir.to_path_buf();
        self.scan_thread = Some(thread::spawn(move || {
            let result = scan_for_conflicts(&game_dir);
            let _ = tx.send(result);
        }));

        self.status = "Scanning...".into();
        self.conflicts.clear();
    }

    fn process_scan_results(&mut self) {
        if let Some(receiver) = &self.receiver {
            if let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(conflicts) => {
                        self.conflicts = conflicts;

                        // Remove old conflicts when new ones are found
                        self.config.ignored.retain(|key, ignored_paths| {
                            self.conflicts
                                .get(key)
                                .map_or(false, |paths| paths == ignored_paths)
                        });
                        self.expanded_conflicts
                            .retain(|k| self.conflicts.contains_key(k));

                        self.status = format!("Found {} conflicts!", self.conflicts.len());

                        let _ = self.config.save();
                    }
                    Err(e) => {
                        self.status = "Scan failed!".into();
                        self.error = Some(e.into());
                    }
                }

                self.receiver = None;
                self.scan_thread = None;
            }
        }
    }

    fn handle_commands(&mut self) -> AnyhowResult<()> {
        let commands = mem::take(&mut self.pending_commands);
        for command in commands {
            match command {
                Command::IgnoreConflict(key, paths) => {
                    self.config.ignored.insert(key, paths);
                }
                Command::UnignoreConflict(key) => {
                    self.config.ignored.remove(&key);
                }
                Command::DeleteConflictFile(key, path) => {
                    delete(&path).context(format!("Failed to delete {}", path.display()))?;

                    if let Some(paths) = self.conflicts.get_mut(&key) {
                        paths.retain(|p| p != &path);
                        if paths.is_empty() {
                            self.conflicts.remove(&key);
                        }
                    }
                }
            }
        }

        self.config.save().context("Failed to save config")?;
        Ok(())
    }

    fn expand_all(&mut self) {
        self.expanded_conflicts = self.conflicts.keys().cloned().collect();
    }

    fn collapse_all(&mut self) {
        self.expanded_conflicts.clear();
    }
}

impl App {
    fn show_error_dialog(&mut self, ctx: &egui::Context) {
        if let Some(err) = &self.error {
            let mut open = true;
            let mut should_clear_error = false;

            egui::Area::new(egui::Id::new("modal_overlay"))
                .order(egui::Order::Background)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        ui.ctx().screen_rect(),
                        egui::CornerRadius::ZERO,
                        egui::Color32::from_black_alpha(150),
                    );
                });

            egui::Window::new("Error")
                .open(&mut open)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Frame::new().inner_margin(6.0).show(ui, |ui| {
                        let errors: Vec<String> = err.chain().map(|e| e.to_string()).collect();

                        let message = errors.join("\n\n").replace(r"\\?\", ""); // Clean Windows extended path prefix

                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(&message).size(14.0));
                            });

                        ui.add_space(7.0);
                        ui.separator();
                        ui.add_space(8.0);

                        ui.with_layout(
                            egui::Layout::top_down_justified(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().button_padding = egui::vec2(6.0, 6.0);

                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("Copy").size(14.0))
                                            .corner_radius(BUTTON_RADIUS),
                                    )
                                    .clicked()
                                {
                                    ui.ctx().copy_text(message);
                                }

                                ui.add_space(6.0);

                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("Close").size(14.0))
                                            .corner_radius(BUTTON_RADIUS),
                                    )
                                    .clicked()
                                {
                                    should_clear_error = true;
                                }
                            },
                        );
                    });
                });

            if !open || should_clear_error {
                self.error = None;
            }
        }
    }

    fn main_ui(&mut self, ui: &mut egui::Ui, bioware_dir: &Path) {
        egui::TopBottomPanel::top("controls").show_inside(ui, |ui| {
            self.scan_controls(ui, bioware_dir);
            ui.add_space(8.0);
        });

        egui::TopBottomPanel::bottom("ignored").show_inside(ui, |ui| {
            ui.add_space(8.0);
            self.ignored_panel(ui, bioware_dir);
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main_results")
                .show(ui, |ui| {
                    self.results_panel(ui, bioware_dir);
                });
        });
    }

    fn scan_controls(&mut self, ui: &mut egui::Ui, bioware_dir: &Path) {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(24.0, 6.0);

            // Scan button
            if ui
                .add_enabled(
                    self.scan_thread.is_none(),
                    egui::Button::new(egui::RichText::new("🔍").size(24.0))
                        .corner_radius(BUTTON_RADIUS),
                )
                .on_hover_text("Start new scan")
                .clicked()
            {
                self.start_scan(bioware_dir);
            }

            ui.add_space(4.0);
            ui.label(egui::RichText::new(&self.status).size(14.0));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().button_padding = egui::vec2(4.0, 2.0);

                ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("⏷").size(12.0))
                                .corner_radius(BUTTON_RADIUS),
                        )
                        .on_hover_text("Expand all conflicts")
                        .clicked()
                    {
                        self.expand_all();
                    }
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new("⏶").size(12.0))
                                .corner_radius(BUTTON_RADIUS),
                        )
                        .on_hover_text("Collapse all conflicts")
                        .clicked()
                    {
                        self.collapse_all();
                    }
                });
            });
        });
    }

    fn results_panel(&mut self, ui: &mut egui::Ui, bioware_dir: &Path) {
        if self.scan_thread.is_some() || !self.has_scanned {
            return;
        }

        let mut filtered_conflicts: Vec<_> = self
            .conflicts
            .iter()
            .filter_map(|(key, paths)| {
                if self.config.ignored.get(key).map_or(false, |p| p == paths) {
                    None
                } else {
                    Some((key.clone(), paths.clone()))
                }
            })
            .collect();

        if filtered_conflicts.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new("All conflicts resolved!").size(24.0))
                        .selectable(false),
                );
            });
            return;
        }

        filtered_conflicts.sort_by(|a, b| a.0.cmp(&b.0));

        egui::ScrollArea::both()
            .id_salt("results_panel")
            .auto_shrink(false)
            .show(ui, |ui| {
                for (key, paths) in filtered_conflicts {
                    self.render_result_conflict(ui, &key, &paths, bioware_dir);
                }
            });
    }

    fn render_result_conflict(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        paths: &[PathBuf],
        bioware_dir: &Path,
    ) {
        let is_open = self.expanded_conflicts.contains(key);

        let response = egui::CollapsingHeader::new(
            egui::RichText::new(format!("{} ({})", key, paths.len())).size(14.0),
        )
        .open(Some(is_open))
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 2,
                    right: 16,
                    top: 6,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    // Ignore button
                    ui.horizontal(|ui| {
                        ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);

                        if ui
                            .add(egui::Button::new("Ignore").corner_radius(BUTTON_RADIUS))
                            .clicked()
                        {
                            self.pending_commands
                                .push(Command::IgnoreConflict(key.to_string(), paths.to_vec()));
                        }
                    });
                    ui.add_space(4.0);

                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 8.0);
                    ui.spacing_mut().button_padding = egui::vec2(2.0, 1.0);

                    for path in paths {
                        self.render_result_conflict_path(
                            ui,
                            path,
                            bioware_dir,
                            key,
                            paths.last().is_some_and(|p| p == path),
                        );
                    }
                });
        });

        if response.header_response.clicked() {
            if is_open {
                self.expanded_conflicts.remove(key);
            } else {
                self.expanded_conflicts.insert(key.to_string());
            }
        }
    }

    fn render_result_conflict_path(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        bioware_dir: &Path,
        key: &str,
        is_last: bool,
    ) {
        ui.horizontal(|ui| {
            // Open in Explorer button
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("📂").size(16.0))
                        .corner_radius(BUTTON_RADIUS),
                )
                .on_hover_text("Open location in Explorer")
                .clicked()
            {
                let _ = open_in_explorer(path);
            }

            // Delete button (only for non-ERF files)
            let is_erf = path
                .extension()
                .map_or(false, |ext| ext.eq_ignore_ascii_case("erf"));
            if ui
                .add_enabled(
                    !is_erf,
                    egui::Button::new(egui::RichText::new("❌").size(16.0))
                        .corner_radius(BUTTON_RADIUS),
                )
                .on_hover_text("Delete file")
                .clicked()
            {
                self.pending_commands.push(Command::DeleteConflictFile(
                    key.to_string(),
                    path.to_path_buf(),
                ));
            }

            let display_path = diff_paths(path, bioware_dir)
                .unwrap_or_else(|| path.to_path_buf())
                .display()
                .to_string();

            let text = if is_last {
                format!("{} ⭐", display_path)
            } else {
                display_path
            };

            ui.add(egui::Label::new(egui::RichText::new(text).size(13.0)).selectable(false));
        });
    }

    fn ignored_panel(&mut self, ui: &mut egui::Ui, bioware_dir: &Path) {
        let mut ignored_conflicts: Vec<_> = self
            .config
            .ignored
            .iter()
            .map(|(key, paths)| (key.clone(), paths.clone()))
            .collect();
        ignored_conflicts.sort_by(|a, b| a.0.cmp(&b.0));

        egui::CollapsingHeader::new(
            egui::RichText::new(format!(
                "Resolved conflicts ({})",
                self.config.ignored.len()
            ))
            .size(18.0),
        )
        .show_unindented(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt("ignored_panel")
                .min_scrolled_height(200.0)
                .auto_shrink(false)
                .show(ui, |ui| {
                    if ignored_conflicts.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("No ignored conflicts")
                                        .color(egui::Color32::DARK_GRAY)
                                        .size(16.0),
                                )
                                .selectable(false),
                            );
                        });
                        return;
                    }

                    egui::Frame::new()
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 8,
                            top: 4,
                            bottom: 8,
                        })
                        .show(ui, |ui| {
                            for (key, paths) in ignored_conflicts {
                                self.render_ignored_conflict(ui, &key, &paths, bioware_dir);
                            }
                        });
                });
        });
    }

    fn render_ignored_conflict(
        &mut self,
        ui: &mut egui::Ui,
        key: &str,
        paths: &[PathBuf],
        bioware_dir: &Path,
    ) {
        egui::CollapsingHeader::new(
            egui::RichText::new(format!("{} ({})", key, paths.len())).size(14.0),
        )
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 2,
                    right: 16,
                    top: 6,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    // Restore button
                    ui.horizontal(|ui| {
                        ui.spacing_mut().button_padding = egui::vec2(6.0, 4.0);

                        if ui
                            .add(egui::Button::new("Forget").corner_radius(BUTTON_RADIUS))
                            .clicked()
                        {
                            self.pending_commands
                                .push(Command::UnignoreConflict(key.to_string()));
                        }
                    });

                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 4.0);

                    for path in paths {
                        self.render_ignored_conflict_path(
                            ui,
                            path,
                            bioware_dir,
                            paths.last().is_some_and(|p| p == path),
                        );
                    }
                });
        });
    }

    fn render_ignored_conflict_path(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        bioware_dir: &Path,
        is_last: bool,
    ) {
        ui.horizontal(|ui| {
            let display_path = diff_paths(path, bioware_dir)
                .unwrap_or_else(|| path.to_path_buf())
                .display()
                .to_string();

            let text = if is_last {
                format!("{} ⭐", display_path)
            } else {
                display_path
            };

            ui.add(egui::Label::new(egui::RichText::new(text).size(12.0)).selectable(false));
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_scan_results();

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(12.0))
            .show(ctx, |ui| match get_bioware_dir() {
                Some(bioware_dir) if bioware_dir.exists() => {
                    self.main_ui(ui, &bioware_dir);
                }
                _ => {
                    self.error = anyhow!(
                        "'Documents/BioWare/Dragon Age' folder is missing, make sure it exists."
                    )
                    .into();
                }
            });

        if let Err(e) = self.handle_commands() {
            self.error = Some(e);
        }

        self.show_error_dialog(ctx);
    }
}

fn get_bioware_dir() -> Option<PathBuf> {
    UserDirs::new()?
        .document_dir()?
        .join("BioWare/Dragon Age")
        .canonicalize()
        .ok()
}
