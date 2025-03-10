use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::{mem, thread};

use anyhow::{Context, Result as AnyhowResult};
use directories::UserDirs;
use eframe::egui;
use pathdiff::diff_paths;

use crate::config::AppConfig;
use crate::scanner::{Conflicts, ScanError, scan_for_conflicts};
use crate::utils::{delete, open_in_explorer};

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

    scan_thread: Option<thread::JoinHandle<()>>,
    receiver: Option<mpsc::Receiver<Result<Conflicts, ScanError>>>,
    pending_commands: Vec<Command>,
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
            status: "".to_string(),
            scan_thread: None,
            receiver: None,
            pending_commands: Vec::new(),
        }
    }

    fn start_scan(&mut self, bioware_dir: &Path) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);

        let game_dir = bioware_dir.to_path_buf();
        self.scan_thread = Some(thread::spawn(move || {
            let result = scan_for_conflicts(&game_dir);
            tx.send(result).unwrap_or_else(|e| {
                eprintln!("Failed to send scan result: {}", e);
            });
        }));

        self.status = "Scanning...".to_string();
        self.conflicts.clear();
    }

    fn process_scan_results(&mut self) {
        if let Some(receiver) = &self.receiver {
            if let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(duplicates) => {
                        self.conflicts = duplicates;

                        // Remove old conflicts when new ones are found
                        self.config.ignored.retain(|key, ignored_paths| {
                            self.conflicts
                                .get(key)
                                .map_or(false, |paths| paths == ignored_paths)
                        });

                        self.status = format!("Found {} conflicts", self.conflicts.len());

                        // Silently ignore
                        self.config.save().unwrap_or_else(|e| {
                            eprintln!("Config error: {}", e);
                        });
                    }
                    Err(e) => {
                        self.status = format!("Scan failed: {}", e);
                    }
                }

                // Reset scan state
                self.receiver.take();
                self.scan_thread.take();
            }
        }
    }

    fn handle_commands(&mut self) -> AnyhowResult<()> {
        let commands = mem::take(&mut self.pending_commands);
        if commands.is_empty() {
            return Ok(());
        }

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

                    // Update conflicts
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
}

impl App {
    fn not_found_ui(&self, ui: &mut egui::Ui) {
        ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.add(egui::Label::new(
                    egui::RichText::new(
                        "'Documents/BioWare/Dragon Age' folder is missing, make sure it exists.",
                    )
                    .size(24.0)
                    .color(egui::Color32::RED),
                ).wrap());
            },
        );
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

            // Status label
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(&self.status)
                    .size(14.0)
                    .color(egui::Color32::LIGHT_GRAY),
            );
        });
    }

    fn results_panel(&mut self, ui: &mut egui::Ui, bioware_dir: &Path) {
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
                    egui::Label::new(egui::RichText::new("No conflicts found!").size(24.0))
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

                    // File list
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

            // Path display
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
            egui::RichText::new(format!("Ignored conflicts ({})", self.config.ignored.len()))
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
            // Path display
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
            .show(ctx, |ui| {
                if let Some(bioware_dir) = get_bioware_dir() {
                    if bioware_dir.exists() {
                        self.main_ui(ui, &bioware_dir);
                        return;
                    }
                };
                self.not_found_ui(ui);
            });

        if let Err(e) = self.handle_commands() {
            self.status = format!("Command error: {}", e);
        }
    }
}

fn get_bioware_dir() -> Option<PathBuf> {
    UserDirs::new()?
        .document_dir()?
        .join("BioWare/Dragon Age")
        .canonicalize()
        .ok()
}
