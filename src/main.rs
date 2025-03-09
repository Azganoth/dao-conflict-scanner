#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod config;
mod erf;
mod scanner;
mod utils;

fn main() {
    let _ = eframe::run_native(
        "DA:O Conflict Scanner",
        eframe::NativeOptions {
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    );
}
