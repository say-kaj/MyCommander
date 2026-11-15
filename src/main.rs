//! myCommander — a native macOS two-panel file manager modelled on how
//! Midnight Commander works (see MECHANICS.md).

mod app;
mod native_menu;
mod dialogs;
mod dialogs2;
mod menus;
mod fsutil;
mod ops;
mod panel;
mod panel_view;
mod preview;
mod screens;
mod settings;
mod textenc;
mod theme;
mod ui_main;
mod util;
mod vfs;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut screenshot: Option<PathBuf> = None;
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => {
                let Some(p) = args.next() else {
                    eprintln!("--screenshot needs a file name");
                    std::process::exit(2);
                };
                let p = PathBuf::from(p);
                let p = if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) };
                screenshot = Some(p);
            }
            "-h" | "--help" => {
                println!("myCommander [left] [right] [--screenshot file.png]   (folders, or sftp://host/path, ftp://host/path)");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("myCommander {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => {
                let p = PathBuf::from(other);
                if vfs::is_remote(&p) {
                    dirs.push(vfs::normalize(&p));
                } else if p.is_dir() {
                    dirs.push(std::fs::canonicalize(&p).unwrap_or(p));
                }
            }
        }
    }

    let taking_picture = screenshot.is_some();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("myCommander")
        .with_app_id("myCommander")
        .with_icon(app_icon())
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([760.0, 440.0])
        // Unified title bar: the toolbar sits next to the traffic-light buttons.
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false);
    if taking_picture {
        // Don't take focus from what the person is doing.
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    // Keep drawing while not focused (needed for the picture).
    options.run_and_return = true;
    #[cfg(target_os = "macos")]
    if taking_picture {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_activate_ignoring_other_apps(false);
        }));
    }

    eframe::run_native("myCommander", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, dirs)))))
}

/// The app icon for the Dock and window. Without it eframe shows its own default "E" icon,
/// even inside an app bundle that has an .icns.
fn app_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../assets/AppIcon-1024.png");
    match image::load_from_memory(PNG) {
        Ok(img) => {
            let img = img.resize(512, 512, image::imageops::FilterType::Lanczos3).to_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}
