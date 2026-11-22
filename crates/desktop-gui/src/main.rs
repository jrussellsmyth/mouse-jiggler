mod app;
mod serial_client;

use app::JigglerApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 580.0])
            .with_min_inner_size([400.0, 480.0])
            .with_title("RP2040 Mouse Jiggler Config"),
        ..Default::default()
    };

    eframe::run_native(
        "RP2040 Mouse Jiggler Config",
        native_options,
        Box::new(|_cc| Ok(Box::new(JigglerApp::default()))),
    )
}
