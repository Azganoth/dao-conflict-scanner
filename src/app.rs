use std::path::PathBuf;
use std::sync::mpsc;
use std::{io, mem, thread};

use crate::config::AppConfig;
use crate::scanner::{DuplicateGroups, find_duplicates};
use crate::utils::{get_bioware_dir, open_location};
use eframe::egui;
use pathdiff::diff_paths;

fn setup_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut(use_theme);
}

fn use_theme(style: &mut egui::Style) {
    style.visuals.selection = egui::style::Selection {
        bg_fill: egui::Color32::PURPLE,
        stroke: egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
    };
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.hovered.expansion = 0.0;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
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
            status: "Idle".to_string(),
            scan_thread: None,
            receiver: None,
            pending_commands: Vec::new(),
            marked_for_delition: None,
        }
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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(egui::Margin::same(12)))
            .show(ctx, |ui| {
                if let Some(bioware_dir) = get_bioware_dir() {
                    if bioware_dir.exists() {
                        self.main(ui, bioware_dir);
                        return;
                    }
                };

                self.not_found_error(ui);
            });
    }
}

impl App {
    fn not_found_error(&self, ui: &mut egui::Ui) {
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

    fn main(&mut self, ui: &mut egui::Ui, bioware_dir: PathBuf) {
        ui.horizontal(|ui| {
            ui.style_mut().spacing.button_padding = egui::vec2(32.0, 8.0);

            if ui
                .add(egui::Button::new(egui::RichText::new("🔍").size(24.0)))
                .clicked()
            {
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

            ui.add_space(10.0);
            ui.label(egui::RichText::new(&self.status).size(16.0));
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // Results
        if let Some(receiver) = &self.receiver {
            if let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(duplicates) => {
                        self.duplicates = duplicates;
                        self.status = format!("Found {} duplicate groups", self.duplicates.len());
                    }
                    Err(e) => {
                        self.status = format!("Error: {}", e);
                    }
                }
                self.scan_thread.take();
            }
        }

        let available_height = ui.available_height();
        let ignored_height = 200.0;
        let results_height = available_height - ignored_height;

        egui::ScrollArea::both()
            .id_salt("results")
            .auto_shrink(false)
            .max_height(results_height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 8.0);
                ui.set_width(ui.available_width());

                let mut sorted_duplicates: Vec<_> = self
                    .duplicates
                    .iter()
                    .filter(|(key, paths)| !self.is_group_ignored(key, paths))
                    .collect();
                sorted_duplicates.sort_by(|a, b| a.0.cmp(b.0));

                for (key, paths) in sorted_duplicates {
                    if paths.len() > 1 {
                        egui::CollapsingHeader::new(
                            egui::RichText::new(format!("{} ({} duplicates)", key, paths.len()))
                                .size(14.0),
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
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

                                        if ui.button("Ignore").clicked() {
                                            self.pending_commands.push(Command::IgnoreGroup(
                                                key.clone(),
                                                paths.clone(),
                                            ));
                                        }
                                    });

                                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                                    ui.spacing_mut().button_padding = egui::vec2(2.0, 1.0);

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

                                            if ui
                                                .add_enabled(
                                                    path.extension()
                                                        .is_some_and(|ext| ext != "erf"),
                                                    egui::Button::new(
                                                        egui::RichText::new("❌").size(16.0),
                                                    ),
                                                )
                                                .on_hover_text("Delete file")
                                                .clicked()
                                            {
                                            }

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

        ui.add_space(10.0);
        ui.separator();
        ui.heading("Ignored");
        ui.add_space(4.0);

        egui::ScrollArea::both()
            .id_salt("ignored")
            .auto_shrink(false)
            .max_height(ignored_height)
            .show(ui, |ui| {
                let mut ignored: Vec<_> = self.config.ignored.iter().collect();
                ignored.sort_by(|a, b| a.0.cmp(b.0));

                for (key, paths) in ignored {
                    egui::CollapsingHeader::new(
                        egui::RichText::new(format!("{} ({} paths)", key, paths.len()))
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
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);

                                    if ui.button("Forget").clicked() {
                                        self.pending_commands
                                            .push(Command::UnignoreGroup(key.clone()));
                                    }
                                });
                                ui.add_space(2.0);

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

        let commands = mem::take(&mut self.pending_commands);
        let should_save = !commands.is_empty();

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
        if should_save {
            let _ = self.config.save();
        }
    }
}
