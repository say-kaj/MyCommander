//! Midnight Commander's pull-down menu structure, item for item, in mc's order
//! (strings at 0x1000a12a0.., built in `_do_nc` 0x10001f4d0 and
//! `_create_panel_menu` 0x100021250). Used by the in-window menu bar (F9) and by
//! the macOS menu bar. Items that need mc's virtual file systems or terminal
//! features are shown but disabled.

use crate::app::Action;

pub struct MItem {
    pub label: &'static str,
    pub key: &'static str,
    /// None = separator.
    pub action: Option<Action>,
    pub enabled: bool,
}

fn it(label: &'static str, key: &'static str, action: Action) -> MItem {
    MItem { label, key, action: Some(action), enabled: true }
}
fn off(label: &'static str) -> MItem {
    MItem { label, key: "", action: Some(Action::Unavailable), enabled: false }
}
fn sep() -> MItem {
    MItem { label: "", key: "", action: None, enabled: false }
}

pub const TITLES: [&str; 5] = ["Left", "File", "Command", "Options", "Right"];

pub fn items(idx: usize) -> Vec<MItem> {
    match idx {
        0 | 4 => {
            let p = if idx == 0 { 0 } else { 1 };
            vec![
                it("File listing", "", Action::ShowListing(p)),
                it("Quick view", "⌃X Q", Action::QuickView(p)),
                it("Info", "⌃X I", Action::Info(p)),
                it("Tree", "", Action::Tree(p)),
                sep(),
                it("Listing format…", "⌥T", Action::ListingDialog(p)),
                it("Sort order…", "", Action::SortDialog(p)),
                it("Filter…", "", Action::FilterDialog(p)),
                off("Encoding…"),
                sep(),
                it("FTP link…", "", Action::Remote(p, crate::vfs::Scheme::Ftp)),
                off("Shell link…"),
                it("SFTP link…", "", Action::Remote(p, crate::vfs::Scheme::Sftp)),
                it("Panelize", "", Action::Panelize(p)),
                sep(),
                it("Rescan", "⌃R", Action::Rescan(p)),
            ]
        }
        1 => vec![
            it("View", "F3", Action::View),
            it("View file…", "", Action::ViewFile),
            it("Filtered view", "⌥!", Action::FilteredView),
            it("Edit", "F4", Action::Edit),
            it("Copy", "F5", Action::Copy),
            it("Chmod", "⌃X C", Action::Chmod),
            it("Link", "⌃X L", Action::Link),
            it("Symlink", "⌃X S", Action::Symlink),
            it("Relative symlink", "⌃X V", Action::RelSymlink),
            it("Edit symlink", "⌃X ⌃S", Action::EditSymlink),
            it("Chown", "⌃X O", Action::Chown),
            it("Advanced chown", "", Action::AdvChown),
            it("Rename/Move", "F6", Action::Move),
            it("Mkdir", "F7", Action::Mkdir),
            it("Delete", "F8", Action::Delete),
            it("Quick cd", "⌥C", Action::QuickCd),
            sep(),
            it("Select group", "+", Action::SelectGroup),
            it("Unselect group", "−", Action::UnselectGroup),
            it("Invert selection", "*", Action::InvertSelection),
            sep(),
            it("Exit", "F10", Action::Quit),
        ],
        2 => vec![
            it("User menu", "F2", Action::UserMenu),
            it("Directory tree", "", Action::DirTree),
            it("Find file", "⌥?", Action::Find),
            it("Swap panels", "⌃U", Action::Swap),
            it("Switch panels on/off", "⌃O", Action::ShowOutput),
            it("Compare directories", "⌃X D", Action::CompareDirs),
            it("Compare files", "⌃X ⌃D", Action::CompareFiles),
            it("External panelize", "⌃X !", Action::ExternalPanelize),
            it("Show directory sizes", "⌃Space", Action::AllDirSizes),
            sep(),
            it("Command history", "⌥H", Action::CommandHistory),
            it("Viewed/edited files history", "⌥⇧E", Action::FileHistory),
            it("Directory hotlist", "⌃\\", Action::Hotlist),
            off("Active VFS list"),
            it("Background jobs", "⌃X J", Action::BackgroundJobs),
            off("Screen list"),
            sep(),
            off("Edit extension file"),
            it("Edit menu file", "", Action::EditMenuFile),
            off("Edit highlighting group file"),
        ],
        _ => vec![
            it("Configuration…", "", Action::Config),
            it("Layout…", "", Action::Layout),
            it("Panel options…", "", Action::PanelOptions),
            it("Confirmation…", "", Action::Confirmation),
            it("Appearance…", "", Action::Appearance),
            it("Quick view…", "", Action::QuickViewOptions),
            off("Display bits…"),
            off("Learn keys…"),
            off("Virtual FS…"),
            sep(),
            it("Save setup", "", Action::SaveSetup),
        ],
    }
}
