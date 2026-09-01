#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app;
mod doc;
mod i18n;
mod io;
mod nav;
mod parser;
mod tabs;
mod view;
mod workspace;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let mut open_paths: Vec<PathBuf> = Vec::new();
    let mut selftest = false;
    for arg in std::env::args().skip(1) {
        if arg == "--selftest" {
            selftest = true;
        } else if arg.starts_with('-') {
            eprintln!("{}", crate::i18n::unknown_arg(&arg));
        } else {
            open_paths.push(PathBuf::from(arg));
        }
    }
    if selftest {
        let fail = parser::selftest();
        std::process::exit(if fail == 0 { 0 } else { 1 });
    }

    let Some(incoming) = io::single::claim(&open_paths) else {
        return Ok(());
    };

    let (size, pos) = io::session::restore_startup_window();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_min_inner_size(io::session::MIN_SIZE)
        .with_title(app::viewport_title(None))
        .with_drag_and_drop(true)
        .with_icon(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                .expect("app icon"),
        );
    if let Some(p) = pos {
        viewport = viewport.with_position(p);
    }
    let native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "rustmarkdown",
        native,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open_paths, incoming)))),
    )
}
