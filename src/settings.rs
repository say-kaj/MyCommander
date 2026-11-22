//! Saved setup ("Options > Save setup"): panel folders, sort, listing, hotlist,
//! and the Settings window.

use crate::panel::{Listing, SortOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
pub struct PanelSetup {
    pub dir: PathBuf,
    pub sort: SortOptions,
    pub listing: Listing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

/// Direction of a notched mouse wheel (PNL-15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WheelDir {
    /// Follow the macOS "natural scrolling" setting.
    System,
    /// Turning the wheel towards you moves down.
    #[default]
    Down,
    /// Turning the wheel towards you moves up.
    Up,
}

impl WheelDir {
    pub const ALL: [WheelDir; 3] = [WheelDir::System, WheelDir::Down, WheelDir::Up];
    pub fn label(self) -> &'static str {
        match self {
            WheelDir::System => "Like the system",
            WheelDir::Down => "Turn towards me = down",
            WheelDir::Up => "Turn towards me = up",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub left: Option<PanelSetup>,
    pub right: Option<PanelSetup>,
    pub show_hidden: bool,
    pub hotlist: Vec<PathBuf>,
    pub auto_save: bool,
    pub font_size: f32,
    pub command_history: Vec<String>,
    pub file_history: Vec<PathBuf>,
    // Layout
    pub horizontal_split: bool,
    pub show_menubar: bool,
    pub show_cmdline: bool,
    pub show_keybar: bool,
    pub show_toolbar: bool,
    // Panel options
    pub show_mini_status: bool,
    pub lynx_motion: bool,
    pub mix_all_files: bool,
    /// Calculate folder sizes in the background (PNL-14).
    pub dir_sizes: bool,
    /// Direction of a notched mouse wheel (PNL-15). Trackpads are not affected.
    pub wheel_dir: WheelDir,
    /// List Time Machine volumes in Places (PNL-16).
    pub show_timemachine: bool,
    /// Look for a new release at startup (SPEC §12).
    pub check_updates: bool,
    // Confirmation
    pub confirm_delete: bool,
    pub confirm_overwrite: bool,
    pub confirm_execute: bool,
    pub confirm_exit: bool,
    // Appearance
    pub theme: ThemeChoice,
    /// Text encoding for Viewer, Editor and Quick view.
    pub text_encoding: crate::textenc::TextEncoding,
    // Quick view
    pub qv_images: bool,
    pub qv_markdown: bool,
    pub qv_highlight: bool,
    pub qv_line_numbers: bool,
    pub qv_max_kb: u32,
    pub qv_theme_light: String,
    pub qv_theme_dark: String,
    /// One `extension=Syntax name` per line, e.g. `conf=INI`.
    pub qv_syntax_map: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            left: None,
            right: None,
            show_hidden: false,
            hotlist: Vec::new(),
            auto_save: true,
            font_size: 13.0,
            command_history: Vec::new(),
            file_history: Vec::new(),
            horizontal_split: false,
            show_menubar: true,
            show_cmdline: true,
            show_keybar: true,
            show_toolbar: true,
            show_mini_status: true,
            lynx_motion: false,
            mix_all_files: false,
            dir_sizes: true,
            wheel_dir: WheelDir::Down,
            show_timemachine: false,
            check_updates: true,
            confirm_delete: true,
            confirm_overwrite: true,
            confirm_execute: false,
            confirm_exit: false,
            theme: ThemeChoice::System,
            text_encoding: Default::default(),
            qv_images: true,
            qv_markdown: true,
            qv_highlight: true,
            qv_line_numbers: false,
            qv_max_kb: 512,
            qv_theme_light: "InspiredGitHub".into(),
            qv_theme_dark: "base16-ocean.dark".into(),
            qv_syntax_map: String::new(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    crate::fsutil::home_dir().join("Library/Application Support/myCommander")
}

fn path() -> PathBuf {
    config_dir().join("settings.json")
}

/// The user menu file (Command > Edit menu file): lines `Label = shell command`,
/// where %f is the current file, %d the current folder, %F / %D the other panel's.
pub fn menu_file() -> PathBuf {
    config_dir().join("menu.txt")
}

pub const DEFAULT_MENU: &str = "# myCommander user menu (F2)\n\
# One entry per line:  Label = shell command\n\
# %f current file, %d current folder, %F file in other panel, %D other folder\n\
List folder sizes = du -sh * | sort -h\n\
Show file type = file %f\n\
Compress to tar.gz = tar czf %f.tar.gz %f\n\
Count lines = wc -l %f\n\
Git status = git status\n";

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let p = path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let s = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, s).map_err(|e| e.to_string())
    }

    pub fn remember_file(&mut self, p: &std::path::Path) {
        self.file_history.retain(|x| x != p);
        self.file_history.push(p.to_path_buf());
        if self.file_history.len() > 50 {
            self.file_history.remove(0);
        }
    }
}

/// User menu entries from the menu file.
pub fn user_menu_entries() -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(menu_file()) else { return Vec::new() };
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(a, b)| (a.trim().to_string(), b.trim().to_string())))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .collect()
}
