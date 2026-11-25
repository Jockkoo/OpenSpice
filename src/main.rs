#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod analysis;
mod dock;
mod plot;
mod schematic;
mod simulation;
mod source;
mod ui;
mod waveform;

use eframe::egui;

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/openspice.png"))
        .expect("embedded OpenSpice icon must be a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("openspice")
            .with_icon(icon)
            .with_inner_size([1140., 700.])
            .with_min_inner_size([720., 430.]),
        ..Default::default()
    };
    eframe::run_native(
        "OpenSpice",
        options,
        Box::new(|cc| Ok(Box::new(ui::Workbench::new(&cc.egui_ctx, cc.storage)))),
    )
}
