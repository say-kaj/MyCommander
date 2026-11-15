//! macOS-style colours, in a light and a dark variant that follow the system
//! appearance. The file-type groups still come from Midnight Commander's
//! filehighlight.ini; here they colour the file icons instead of the names.

use crate::fsutil::Highlight;
use egui::Color32;

#[derive(Clone, Copy)]
pub struct Pal {
    pub dark: bool,
    pub window: Color32,
    pub toolbar: Color32,
    pub toolbar_line: Color32,
    pub card: Color32,
    pub row_alt: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub separator: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub sel_inactive: Color32,
    pub marked_bg: Color32,
    pub marked_text: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_border: Color32,
    pub field: Color32,
    pub menu_bg: Color32,
    pub danger: Color32,
    pub folder: Color32,
    pub folder_dark: Color32,
    pub shadow: Color32,
    pub code_bg: Color32,
    pub code_text: Color32,
}

pub const LIGHT: Pal = Pal {
    dark: false,
    window: Color32::from_rgb(236, 236, 236),
    toolbar: Color32::from_rgb(232, 232, 232),
    toolbar_line: Color32::from_rgb(208, 208, 208),
    card: Color32::from_rgb(255, 255, 255),
    row_alt: Color32::from_rgb(245, 245, 247),
    text: Color32::from_rgb(29, 29, 31),
    text_dim: Color32::from_rgb(110, 110, 115),
    text_faint: Color32::from_rgb(160, 160, 165),
    separator: Color32::from_rgb(222, 222, 224),
    accent: Color32::from_rgb(10, 100, 216),
    accent_text: Color32::WHITE,
    sel_inactive: Color32::from_rgb(220, 220, 224),
    marked_bg: Color32::from_rgb(255, 238, 196),
    marked_text: Color32::from_rgb(178, 84, 0),
    control: Color32::from_rgb(255, 255, 255),
    control_hover: Color32::from_rgb(242, 242, 244),
    control_border: Color32::from_rgb(200, 200, 204),
    field: Color32::from_rgb(255, 255, 255),
    menu_bg: Color32::from_rgb(246, 246, 246),
    danger: Color32::from_rgb(215, 0, 21),
    folder: Color32::from_rgb(84, 168, 247),
    folder_dark: Color32::from_rgb(52, 138, 228),
    shadow: Color32::from_black_alpha(60),
    code_bg: Color32::from_rgb(255, 255, 255),
    code_text: Color32::from_rgb(29, 29, 31),
};

pub const DARK: Pal = Pal {
    dark: true,
    window: Color32::from_rgb(30, 30, 30),
    toolbar: Color32::from_rgb(44, 44, 46),
    toolbar_line: Color32::from_rgb(20, 20, 20),
    card: Color32::from_rgb(36, 36, 38),
    row_alt: Color32::from_rgb(43, 43, 46),
    text: Color32::from_rgb(235, 235, 240),
    text_dim: Color32::from_rgb(160, 160, 166),
    text_faint: Color32::from_rgb(110, 110, 116),
    separator: Color32::from_rgb(58, 58, 62),
    accent: Color32::from_rgb(10, 110, 230),
    accent_text: Color32::WHITE,
    sel_inactive: Color32::from_rgb(70, 70, 74),
    marked_bg: Color32::from_rgb(92, 64, 10),
    marked_text: Color32::from_rgb(255, 196, 80),
    control: Color32::from_rgb(70, 70, 74),
    control_hover: Color32::from_rgb(86, 86, 90),
    control_border: Color32::from_rgb(86, 86, 90),
    field: Color32::from_rgb(30, 30, 32),
    menu_bg: Color32::from_rgb(50, 50, 52),
    danger: Color32::from_rgb(255, 69, 58),
    folder: Color32::from_rgb(90, 170, 245),
    folder_dark: Color32::from_rgb(60, 140, 220),
    shadow: Color32::from_black_alpha(120),
    code_bg: Color32::from_rgb(30, 30, 32),
    code_text: Color32::from_rgb(220, 220, 225),
};

/// Colour of a file icon's badge for its mc highlight group.
pub fn type_color(h: Highlight) -> Color32 {
    match h {
        Highlight::Normal | Highlight::Symlink => Color32::from_rgb(142, 142, 147),
        Highlight::Directory => Color32::from_rgb(84, 168, 247),
        Highlight::Executable => Color32::from_rgb(52, 199, 89),
        Highlight::StaleLink => Color32::from_rgb(255, 59, 48),
        Highlight::Device | Highlight::Special => Color32::from_rgb(175, 82, 222),
        Highlight::Core => Color32::from_rgb(255, 59, 48),
        Highlight::Temp => Color32::from_rgb(174, 174, 178),
        Highlight::Archive => Color32::from_rgb(175, 82, 222),
        Highlight::Doc => Color32::from_rgb(0, 122, 255),
        Highlight::Source => Color32::from_rgb(255, 149, 0),
        Highlight::Media => Color32::from_rgb(255, 45, 85),
        Highlight::Graph => Color32::from_rgb(48, 176, 199),
        Highlight::Database => Color32::from_rgb(162, 132, 94),
    }
}

/// A Finder-like "Kind" text for the list.
pub fn kind_label(h: Highlight, ext: &str, is_app: bool) -> String {
    if is_app {
        return "Application".into();
    }
    let e = ext.to_ascii_uppercase();
    match h {
        Highlight::Directory => "Folder".into(),
        Highlight::Executable => "Program".into(),
        Highlight::Symlink => "Alias".into(),
        Highlight::StaleLink => "Broken alias".into(),
        Highlight::Device => "Device".into(),
        Highlight::Special => "Special file".into(),
        Highlight::Core => "Crash dump".into(),
        Highlight::Archive => format!("{} archive", e),
        Highlight::Graph => format!("{} image", e),
        Highlight::Media => format!("{} media", e),
        Highlight::Source => format!("{} source", e),
        Highlight::Doc => format!("{} document", e),
        Highlight::Database => format!("{} database", e),
        Highlight::Temp => "Temporary file".into(),
        Highlight::Normal => {
            if e.is_empty() {
                "Document".into()
            } else {
                format!("{} file", e)
            }
        }
    }
}
