#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ble;
mod gif_proc;
mod giphy;
mod protocol;
mod storage;
mod video_proc;

use app::IPixelApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 700.0])
            .with_min_inner_size([820.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        concat!("iPixel 32x32 Controller v", env!("CARGO_PKG_VERSION")),
        options,
        Box::new(|cc| Ok(Box::new(IPixelApp::new(cc)))),
    )
}
