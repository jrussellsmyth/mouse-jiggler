mod app;
mod serial_client;

use app::JigglerApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 520.0])
            .with_min_inner_size([380.0, 420.0])
            .with_title("RP2040 Mouse Jiggler Config"),
        ..Default::default()
    };

    eframe::run_native(
        "RP2040 Mouse Jiggler Config",
        native_options,
        Box::new(|_cc| Box::new(JigglerApp::default())),
    )
}
