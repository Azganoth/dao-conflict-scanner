use std::path::PathBuf;
use std::sync::mpsc;
use std::{io, mem, thread};

use crate::config::AppConfig;
use crate::scanner::{DuplicateGroups, find_duplicates};
use crate::utils::{get_bioware_dir, open_location};
use anyhow::Context;
use eframe::egui;
use pathdiff::diff_paths;

fn setup_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style()).clone();

    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.hovered.expansion = 0.0;

    ctx.set_style(style);
}

pub struct App {
    config: AppConfig,
    duplicates: DuplicateGroups,
    status: String,

    scan_thread: Option<thread::JoinHandle<()>>,
    receiver: Option<mpsc::Receiver<io::Result<DuplicateGroups>>>,
    pending_commands: Vec<Command>,
    marked_for_delition: Option<PathBuf>,
}

#[derive(Debug)]
enum Command {
    IgnoreGroup(String, Vec<PathBuf>),
    UnignoreGroup(String),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_theme(&cc.egui_ctx);

        Self {
            config: AppConfig::load(),
            duplicates: DuplicateGroups::new(),
            status: "Waiting...".to_string(),
            scan_thread: None,
            receiver: None,
            pending_commands: Vec::new(),
            marked_for_delition: None,
        }
    }

    fn start_scan(&mut self, bioware_dir: &PathBuf) {
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);

        let game_dir = bioware_dir.clone();
        self.scan_thread = Some(thread::spawn(move || {
            let result = find_duplicates(&game_dir);
            tx.send(result).unwrap();
        }));

        self.status = "Scanning...".to_string();
        self.duplicates.clear();
    }

    fn process_scan_results(&mut self) {
        if let Some(receiver) = &self.receiver {
            if let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(duplicates) => {
                        self.duplicates = duplicates;

                        // Cleanup ignored groups in-place
                        self.config.ignored.retain(|key, ignored_paths| {
                            match self.duplicates.get(key) {
                                Some(current_paths) => {
                                    let mut sorted_ignored = ignored_paths.clone();
                                    let mut sorted_current = current_paths.clone();
                                    sorted_ignored.sort();
                                    sorted_current.sort();
                                    sorted_ignored == sorted_current
                                }
                                None => false,
                            }
                        });

                        self.status = format!("Found {} duplicate groups", self.duplicates.len());

                        if let Err(e) = self.config.save() {
                            self.status = format!("Config error: {}", e);
                        }
                    }
                    Err(e) => {
                        self.status = format!("Scan error: {}", e);
                    }
                }
                self.scan_thread.take();
            }
        }
    }

    fn handle_commands(&mut self) -> anyhow::Result<()> {
        let commands = mem::take(&mut self.pending_commands);
        if commands.is_empty() {
            return Ok(());
        }

        for command in commands {
            match command {
                Command::IgnoreGroup(key, paths) => {
                    let mut sorted_paths = paths;
                    sorted_paths.sort();
                    self.config.ignored.insert(key, sorted_paths);
                }
                Command::UnignoreGroup(key) => {
                    self.config.ignored.remove(&key);
                }
            }
        }

        self.config.save().context("Failed to save config")?;
        Ok(())
    }

    fn is_group_ignored(&self, key: &str, paths: &[PathBuf]) -> bool {
        self.config.ignored.get(key).map_or(false, |ignored_paths| {
            let mut sorted_current = paths.to_vec();
            let mut sorted_ignored = ignored_paths.clone();
            sorted_current.sort();
            sorted_ignored.sort();
            sorted_current == sorted_ignored
        })
    }
}

impl App {
    fn main_ui(&mut self, ui: &mut egui::Ui, bioware_dir: PathBuf) {
        egui::TopBottomPanel::top("controls").show_inside(ui, |ui| {
            self.scan_controls(ui, &bioware_dir);
            ui.add_space(8.0);
        });

        egui::TopBottomPanel::bottom("ignored")
            .min_height(200.0)
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                self.ignored_panel(ui, &bioware_dir);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main_results")
                .show(ui, |ui| {
                    self.results_panel(ui, &bioware_dir);
                });
        });
    }

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

    fn scan_controls(&mut self, ui: &mut egui::Ui, bioware_dir: &PathBuf) {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(24.0, 6.0);

            // Scan button
            if ui
                .add(egui::Button::new(egui::RichText::new("🔍").size(24.0)))
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

    fn ignored_panel(&mut self, ui: &mut egui::Ui, bioware_dir: &PathBuf) {
        ui.heading(format!("Ignored Groups ({})", self.config.ignored.len()));
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .id_salt("ignored_panel")
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut ignored: Vec<_> = self.config.ignored.iter().collect();
                ignored.sort_by(|a, b| a.0.cmp(b.0));

                for (key, paths) in ignored {
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!("{} ({})", key, paths.len()))
                            .size(14.0)
                            .color(egui::Color32::GRAY),
                    )
                    .show(ui, |ui| {
                        egui::Frame::NONE
                            .inner_margin(egui::Margin {
                                left: 2,
                                right: 16,
                                top: 6,
                                bottom: 8,
                            })
                            .show(ui, |ui| {
                                // Restore button
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

                                    if ui.button("Forget").clicked() {
                                        self.pending_commands
                                            .push(Command::UnignoreGroup(key.to_string()));
                                    }
                                });

                                // File list
                                ui.spacing_mut().item_spacing = egui::vec2(10.0, 4.0);

                                for path in paths {
                                    ui.horizontal(|ui| {
                                        let mut display_text = format!(
                                            "{}",
                                            diff_paths(path, bioware_dir.clone())
                                                .unwrap_or_else(|| path.clone())
                                                .display()
                                        );
                                        if paths.last().is_some_and(|last_path| last_path == path) {
                                            display_text.push_str(" ⭐");
                                        }

                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(display_text).size(12.0),
                                            )
                                            .selectable(false),
                                        );
                                    });
                                }
                            });
                    });
                }
            });
    }

    fn results_panel(&mut self, ui: &mut egui::Ui, bioware_dir: &PathBuf) {
        egui::ScrollArea::vertical()
            .id_salt("results_panel")
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut sorted_duplicates: Vec<_> = self
                    .duplicates
                    .iter()
                    .filter(|(key, paths)| !self.is_group_ignored(key, paths))
                    .collect();
                sorted_duplicates.sort_by(|a, b| a.0.cmp(b.0));

                for (key, paths) in sorted_duplicates {
                    if paths.len() > 1 {
                        egui::CollapsingHeader::new(
                            egui::RichText::new(format!("{} ({})", key, paths.len())).size(14.0),
                        )
                        .show(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin {
                                    left: 2,
                                    right: 16,
                                    top: 6,
                                    bottom: 8,
                                })
                                .show(ui, |ui| {
                                    // Ignore button
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

                                        if ui.button("Ignore").clicked() {
                                            self.pending_commands.push(Command::IgnoreGroup(
                                                key.to_string(),
                                                paths.clone(),
                                            ));
                                        }
                                    });
                                    ui.add_space(4.0);

                                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                                    ui.spacing_mut().button_padding = egui::vec2(2.0, 1.0);

                                    // File list
                                    for path in paths {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(egui::Button::new(
                                                    egui::RichText::new("📂").size(16.0),
                                                ))
                                                .on_hover_text("Open location in Explorer")
                                                .clicked()
                                            {
                                                let _ = open_location(path);
                                            }

                                            // Delete button (only for non-ERF files)
                                            let is_erf =
                                                path.extension().map_or(false, |ext| ext == "erf");
                                            if ui
                                                .add_enabled(
                                                    !is_erf,
                                                    egui::Button::new(
                                                        egui::RichText::new("❌").size(16.0),
                                                    ),
                                                )
                                                .on_hover_text("Delete file")
                                                .clicked()
                                            {
                                            }

                                            // Path display
                                            let mut display_text = format!(
                                                "{}",
                                                diff_paths(path, bioware_dir.clone())
                                                    .unwrap_or_else(|| path.clone())
                                                    .display()
                                            );
                                            if paths
                                                .last()
                                                .is_some_and(|last_path| last_path == path)
                                            {
                                                display_text.push_str(" ⭐");
                                            }

                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(display_text).size(13.0),
                                                )
                                                .selectable(false),
                                            );
                                        });
                                    }
                                });
                        });
                    }
                }
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
                        self.main_ui(ui, bioware_dir);
                        return;
                    }
                };
                self.not_found_ui(ui);
            });

        if let Err(e) = self.handle_commands() {
            self.status = format!("Config error: {}", e);
        }
    }
}
