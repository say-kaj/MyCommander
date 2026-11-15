//! The myCommander window state, actions and keyboard handling.
//! Drawing lives in `ui_main.rs`, dialogs in `dialogs.rs`, viewer/editor in `screens.rs`.

use crate::fsutil::{self, Entry, Kind};
use crate::ops::{self, OpKind, Shared};
use crate::panel::{Listing, Panel, PanelView, SortOptions};
use crate::settings::{PanelSetup, Settings};
use crate::util::*;
use egui::{FontId, Key};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Unavailable,
    Tree(usize),
    Panelize(usize),
    ViewFile,
    FilteredView,
    Link,
    RelSymlink,
    EditSymlink,
    Chown,
    AdvChown,
    QuickCd,
    DirTree,
    CompareFiles,
    ExternalPanelize,
    AllDirSizes,
    CommandHistory,
    FileHistory,
    BackgroundJobs,
    EditMenuFile,
    Layout,
    PanelOptions,
    Confirmation,
    Appearance,
    QuickViewOptions,
    UserCommand(usize),
    Help,
    UserMenu,
    View,
    Edit,
    EditNew,
    Copy,
    Move,
    Mkdir,
    Delete,
    Menu,
    Quit,
    Reread,
    Swap,
    ToggleHidden,
    DirSize,
    Find,
    Hotlist,
    HotlistAdd,
    CycleListing,
    ListingDialog(usize),
    SortDialog(usize),
    FilterDialog(usize),
    Rescan(usize),
    ShowListing(usize),
    QuickView(usize),
    Info(usize),
    SelectGroup,
    UnselectGroup,
    InvertSelection,
    Chmod,
    Symlink,
    CompareDirs,
    ShowOutput,
    OtherPanelHere,
    Config,
    SaveSetup,
    #[allow(dead_code)]
    About,
    OpenFinder,
    OpenTerminal,
    CopyPath,
    QuickLook,
    Zip,
}

pub struct MenuState {
    pub top: usize,
    pub sel: usize,
    pub user: bool,
}

pub enum Dialog {
    Message { title: String, text: String, error: bool },
    Mkdir { name: String },
    CopyMove { kind: OpKind, sources: Vec<Entry>, mask: String, dest: String },
    Delete { sources: Vec<PathBuf>, text: String },
    SelectGroup { select: bool, pattern: String, files_only: bool, case: bool },
    Filter { panel: usize, pattern: String },
    Sort { panel: usize, opts: SortOptions },
    Listing { panel: usize, listing: Listing },
    Chmod { paths: Vec<PathBuf>, mode: u32 },
    Link { kind: LinkKind, target: String, link: String },
    EditSymlink { link: PathBuf, target: String },
    Chown { paths: Vec<PathBuf>, owner: String, group: String, mode: Option<u32> },
    Input { kind: InputKind, value: String },
    History { files: bool, cursor: usize },
    Options { page: OptPage, s: Box<Settings> },
    ConfirmQuit,
    ConfirmExec { name: String },
    Find { start: String, pattern: String, content: String, case: bool, skip_hidden: bool },
    FindResults { shared: Shared, cursor: usize },
    Hotlist { cursor: usize },
    Help,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Hard,
    Symbolic,
    Relative,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    ViewFile,
    FilteredView,
    QuickCd,
    ExternalPanelize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptPage {
    Configuration,
    Layout,
    PanelOptions,
    Confirmation,
    Appearance,
    QuickView,
}

pub enum JobKind {
    FileOp(OpKind),
    DirSize { panel: usize, name: String },
    DirSizes { panel: usize },
    Panelize { panel: usize },
    Shell,
}

pub struct Job {
    pub kind: JobKind,
    pub shared: Shared,
}

pub enum Screen {
    Main,
    Viewer(Box<crate::screens::Viewer>),
    Editor(Box<crate::screens::Editor>),
    Output,
}

pub struct App {
    pub panels: [Panel; 2],
    pub active: usize,
    pub cmdline: String,
    pub cmd_hist_pos: Option<usize>,
    pub dialog: Option<Dialog>,
    pub screen: Screen,
    pub menu: Option<MenuState>,
    pub job: Option<Job>,
    pub settings: Settings,
    pub last_output: String,
    pub screenshot: Option<PathBuf>,
    pub frame: u64,
    pub screenshot_requested: bool,
    pub xmap: bool,
    pub status: Option<(String, f64)>,
    pub char_w: f32,
    pub quit_asked: bool,
    pub pal: crate::theme::Pal,
    pub native_menu: Option<crate::native_menu::NativeMenu>,
    pub search_text: String,
    pub preview: crate::preview::Preview,
    /// The last Find / External panelize result, for Left/Right > Panelize.
    pub last_panelize: Option<(PathBuf, Vec<PathBuf>)>,
    /// x position of each menu title in the menu bar (for placing the drop-down).
    pub menu_x: [f32; 5],
}

impl App {
    pub fn new(cc: &eframe::CreationContext, screenshot: Option<PathBuf>, start_dirs: Vec<PathBuf>) -> Self {
        install_fonts(&cc.egui_ctx);
        let settings = if screenshot.is_some() { Settings::default() } else { Settings::load() };
        let home = fsutil::home_dir();
        let pick = |setup: &Option<PanelSetup>, arg: Option<&PathBuf>, fallback: PathBuf| -> (PathBuf, Option<PanelSetup>) {
            if let Some(a) = arg {
                return (a.clone(), setup.clone());
            }
            match setup {
                Some(s) if s.dir.is_dir() => (s.dir.clone(), Some(s.clone())),
                _ => (fallback, setup.clone()),
            }
        };
        let cwd = std::env::current_dir().unwrap_or_else(|_| home.clone());
        let left_default = if screenshot.is_some() || cwd == Path::new("/") { home.clone() } else { cwd };
        let (ld, ls) = pick(&settings.left, start_dirs.first(), left_default);
        let right_default = if screenshot.is_some() { PathBuf::from("/Applications") } else { home.clone() };
        let (rd, rs) = pick(&settings.right, start_dirs.get(1), right_default);
        let show_hidden = settings.show_hidden;
        let make = |d: PathBuf, s: Option<PanelSetup>| {
            let mut p = Panel::new(d);
            p.show_hidden = show_hidden;
            if let Some(s) = s {
                p.sort = s.sort;
                p.listing = s.listing;
            }
            p.reload_keep(None);
            p
        };
        let left = make(ld, ls);
        let right = make(rd, rs);
        let pal = if cc.egui_ctx.system_theme() == Some(egui::Theme::Dark) { crate::theme::DARK } else { crate::theme::LIGHT };
        let mut style = (*cc.egui_ctx.style()).clone();
        apply_style(&mut style, settings.font_size, &pal);
        cc.egui_ctx.set_style(style);
        cc.egui_ctx.data_mut(|d| d.insert_temp(egui::Id::new("pal_dark"), pal.dark));
        let native_menu = if screenshot.is_none() { crate::native_menu::NativeMenu::install() } else { None };
        App {
            panels: [left, right],
            active: 0,
            cmdline: String::new(),
            cmd_hist_pos: None,
            dialog: None,
            screen: Screen::Main,
            menu: None,
            job: None,
            settings,
            last_output: String::new(),
            screenshot,
            frame: 0,
            screenshot_requested: false,
            xmap: false,
            status: None,
            char_w: 8.0,
            preview: Default::default(),
            quit_asked: false,
            pal,
            native_menu,
            search_text: String::new(),
            last_panelize: None,
            menu_x: [12.0, 70.0, 130.0, 220.0, 300.0],
        }
    }

    /// Follow the system's light / dark appearance.
    pub fn refresh_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.settings.theme {
            crate::settings::ThemeChoice::System => ctx.system_theme() == Some(egui::Theme::Dark),
            crate::settings::ThemeChoice::Light => false,
            crate::settings::ThemeChoice::Dark => true,
        };
        if dark != self.pal.dark {
            self.pal = if dark { crate::theme::DARK } else { crate::theme::LIGHT };
            self.restyle(ctx);
        }
    }

    pub fn restyle(&mut self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        apply_style(&mut style, self.settings.font_size, &self.pal);
        ctx.set_style(style);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("pal_dark"), self.pal.dark));
    }

    fn poll_native_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.native_menu.as_ref() else { return };
        let actions = menu.poll();
        for a in actions {
            let main = matches!(self.screen, Screen::Main) && self.dialog.is_none() && self.job.is_none();
            if main || a == Action::Quit {
                self.do_action(ctx, a);
            }
        }
    }

    pub fn act(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    pub fn mono(&self) -> FontId {
        FontId::monospace(self.settings.font_size)
    }

    pub fn err(&mut self, text: impl Into<String>) {
        self.dialog = Some(Dialog::Message { title: "Error".into(), text: text.into(), error: true });
    }

    pub fn flash(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.status = Some((text.into(), ctx.input(|i| i.time) + 3.0));
    }

    pub fn reload_both(&mut self) {
        for p in self.panels.iter_mut() {
            p.reload();
        }
    }

    pub fn save_setup(&mut self) -> Result<(), String> {
        let mk = |p: &Panel| PanelSetup { dir: p.dir.clone(), sort: p.sort.clone(), listing: p.listing };
        self.settings.left = Some(mk(&self.panels[0]));
        self.settings.right = Some(mk(&self.panels[1]));
        self.settings.show_hidden = self.panels[0].show_hidden;
        self.settings.save()
    }

    pub fn chdir_active(&mut self, dir: &Path) {
        if let Err(e) = self.act().chdir(dir, true) {
            self.err(e);
        }
    }

    /// Enter on the cursor: folders open, programs run, other files open in their Mac app.
    pub fn enter(&mut self, ctx: &egui::Context) {
        let Some(e) = self.act().current().cloned() else { return };
        if e.is_updir() {
            if let Err(err) = self.act().go_parent() {
                self.err(err);
            }
            return;
        }
        if e.is_dir_like() && !e.name.ends_with(".app") {
            let p = e.path.clone();
            self.chdir_active(&p);
            return;
        }
        if e.is_executable() && e.kind == Kind::File {
            if self.settings.confirm_execute {
                self.dialog = Some(Dialog::ConfirmExec { name: e.name.clone() });
            } else {
                let cmd = format!("./{}", shell_quote(&e.name));
                self.run_command(ctx, cmd);
            }
            return;
        }
        match std::process::Command::new("/usr/bin/open").arg(&e.path).spawn() {
            Ok(_) => self.flash(ctx, format!("Opening {}", e.name)),
            Err(err) => self.err(format!("Cannot open \"{}\"\n{}", e.name, err)),
        }
    }

    pub fn run_command(&mut self, ctx: &egui::Context, cmd: String) {
        let cmd = cmd.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        if self.settings.command_history.last() != Some(&cmd) {
            self.settings.command_history.push(cmd.clone());
            if self.settings.command_history.len() > 100 {
                self.settings.command_history.remove(0);
            }
        }
        self.cmd_hist_pos = None;
        // `cd` is handled by the panel itself.
        if cmd == "cd" || cmd.starts_with("cd ") {
            let arg = cmd[2..].trim();
            let target = if arg.is_empty() || arg == "~" {
                fsutil::home_dir()
            } else if let Some(rest) = arg.strip_prefix("~/") {
                fsutil::home_dir().join(rest)
            } else {
                self.act().dir.join(unquote(arg))
            };
            self.chdir_active(&target);
            self.cmdline.clear();
            return;
        }
        if self.job.is_some() {
            self.flash(ctx, "Please wait for the running job to finish");
            return;
        }
        let dir = self.act().dir.clone();
        self.job = Some(Job { kind: JobKind::Shell, shared: ops::start_shell(cmd.clone(), dir.clone()) });
        self.last_output = format!("{}$ {}\n", fsutil::tilde(&dir), cmd);
        self.cmdline.clear();
    }

    pub fn open_editor(&mut self, path: &Path) {
        match std::fs::read(path) {
            Ok(bytes) => {
                if bytes.len() > 16 << 20 {
                    self.err("This file is too large for the editor (over 16 MB).");
                    return;
                }
                let text = String::from_utf8_lossy(&bytes).to_string();
                self.settings.remember_file(path);
                self.screen = Screen::Editor(Box::new(crate::screens::Editor::new(path.to_path_buf(), text, false)));
            }
            Err(e) => self.err(format!("Cannot open \"{}\" for editing\n{}", path.display(), e)),
        }
    }

    pub fn start_op(&mut self, kind: OpKind, sources: Vec<PathBuf>, dest: Option<PathBuf>) {
        if self.job.is_some() {
            self.err("Another operation is still running.");
            return;
        }
        let ask = self.settings.confirm_overwrite;
        self.job = Some(Job { kind: JobKind::FileOp(kind), shared: ops::start_file_op_with(kind, sources, dest, ask) });
    }

    /// Ctrl-X d: mark files that are missing, different or newer than in the other panel.
    pub fn compare_dirs(&mut self, ctx: &egui::Context) {
        use std::collections::HashMap;
        let (a, b) = (self.active, 1 - self.active);
        let index = |p: &Panel| -> HashMap<String, (u64, i64, bool)> {
            p.entries.iter().map(|e| (e.name.clone(), (e.size, e.mtime, e.is_dir_like()))).collect()
        };
        let names_a = index(&self.panels[a]);
        let names_b = index(&self.panels[b]);
        let mark = |p: &mut Panel, other: &HashMap<String, (u64, i64, bool)>| {
            for e in p.entries.iter_mut() {
                if e.is_dir_like() {
                    continue;
                }
                e.marked = match other.get(&e.name) {
                    None => true,
                    Some(&(s, t, d)) => !d && (s != e.size || t < e.mtime),
                };
            }
        };
        mark(&mut self.panels[a], &names_b);
        mark(&mut self.panels[b], &names_a);
        self.flash(ctx, "Files that differ are marked in both panels");
    }

    pub fn do_action(&mut self, ctx: &egui::Context, a: Action) {
        self.menu = None;
        match a {
            Action::Help => self.dialog = Some(Dialog::Help),
            Action::UserMenu => self.menu = Some(MenuState { top: 0, sel: 0, user: true }),
            Action::Menu => self.menu = Some(MenuState { top: if self.active == 0 { 0 } else { 4 }, sel: 0, user: false }),
            Action::View => {
                let Some(e) = self.act().current().cloned() else { return };
                if e.is_updir() {
                    let _ = self.act().go_parent();
                } else if e.is_dir_like() {
                    self.chdir_active(&e.path);
                } else {
                    self.open_viewer(&e.path);
                }
            }
            Action::Edit => {
                if let Some(e) = self.act().current().cloned() {
                    if !e.is_dir_like() {
                        self.open_editor(&e.path);
                    }
                }
            }
            Action::EditNew => {
                let dir = self.act().dir.clone();
                let mut n = 1;
                let mut p = dir.join("untitled.txt");
                while p.exists() {
                    n += 1;
                    p = dir.join(format!("untitled{}.txt", n));
                }
                self.screen = Screen::Editor(Box::new(crate::screens::Editor::new(p, String::new(), true)));
            }
            Action::Copy | Action::Move => {
                let sources = self.act().selection();
                if sources.is_empty() {
                    self.err("Cannot operate on \"..\"!");
                    return;
                }
                let kind = if a == Action::Copy { OpKind::Copy } else { OpKind::Move };
                let other = &self.panels[1 - self.active];
                let mut dest = format!("{}/", other.dir.display());
                if other.dir == self.panels[self.active].dir && sources.len() == 1 {
                    dest = sources[0].name.clone();
                }
                self.dialog = Some(Dialog::CopyMove { kind, sources, mask: "*".into(), dest });
            }
            Action::Mkdir => self.dialog = Some(Dialog::Mkdir { name: String::new() }),
            Action::Delete => {
                let sources = self.act().selection();
                if sources.is_empty() {
                    self.err("Cannot operate on \"..\"!");
                    return;
                }
                let text = if sources.len() == 1 {
                    let e = &sources[0];
                    if e.is_dir_like() && e.kind != Kind::Symlink {
                        format!("Delete directory\n\"{}\"\nand everything inside it?", e.name)
                    } else {
                        format!("Delete file\n\"{}\"?", e.name)
                    }
                } else {
                    format!("Delete {} files/directories?", sources.len())
                };
                let paths: Vec<PathBuf> = sources.iter().map(|e| e.path.clone()).collect();
                if self.settings.confirm_delete {
                    self.dialog = Some(Dialog::Delete { sources: paths, text });
                } else {
                    self.start_op(OpKind::Delete, paths, None);
                }
            }
            Action::Quit if self.settings.confirm_exit && !self.quit_asked => {
                self.dialog = Some(Dialog::ConfirmQuit);
            }
            Action::Quit => {
                if self.screenshot.is_none() && self.settings.auto_save {
                    let _ = self.save_setup();
                }
                self.quit_asked = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Action::Reread => self.reload_both(),
            Action::Rescan(p) => self.panels[p].reload(),
            Action::Swap => {
                self.panels.swap(0, 1);
                self.active = 1 - self.active;
            }
            Action::ToggleHidden => {
                let v = !self.panels[0].show_hidden;
                for p in self.panels.iter_mut() {
                    p.show_hidden = v;
                    p.reload();
                }
                self.settings.show_hidden = v;
                self.flash(ctx, if v { "Hidden files are shown" } else { "Hidden files are hidden" });
            }
            Action::DirSize => {
                let Some(e) = self.act().current().cloned() else { return };
                if e.is_dir_like() && !e.is_updir() && self.job.is_none() {
                    self.job = Some(Job {
                        kind: JobKind::DirSize { panel: self.active, name: e.name.clone() },
                        shared: ops::start_dir_size(e.path.clone()),
                    });
                }
            }
            Action::Find => {
                let start = format!("{}", self.act().dir.display());
                self.dialog = Some(Dialog::Find { start, pattern: "*".into(), content: String::new(), case: false, skip_hidden: true });
            }
            Action::Hotlist => self.dialog = Some(Dialog::Hotlist { cursor: 0 }),
            Action::HotlistAdd => {
                let d = self.act().dir.clone();
                if !self.settings.hotlist.contains(&d) {
                    self.settings.hotlist.push(d.clone());
                    let _ = self.settings.save();
                }
                self.flash(ctx, format!("Added {} to the hotlist", fsutil::tilde(&d)));
            }
            Action::CycleListing => {
                let p = self.act();
                p.listing = p.listing.next();
            }
            Action::ListingDialog(p) => self.dialog = Some(Dialog::Listing { panel: p, listing: self.panels[p].listing }),
            Action::SortDialog(p) => self.dialog = Some(Dialog::Sort { panel: p, opts: self.panels[p].sort.clone() }),
            Action::FilterDialog(p) => {
                let pattern = self.panels[p].filter.clone().unwrap_or_else(|| "*".into());
                self.dialog = Some(Dialog::Filter { panel: p, pattern });
            }
            Action::ShowListing(p) => {
                self.panels[p].view = PanelView::Listing;
                self.active = p;
            }
            Action::QuickView(p) | Action::Info(p) => {
                let v = if matches!(a, Action::Info(_)) { PanelView::Info } else { PanelView::QuickView };
                let panel = &mut self.panels[p];
                panel.view = if panel.view == v { PanelView::Listing } else { v };
                if self.panels[p].view != PanelView::Listing {
                    self.active = 1 - p;
                }
            }
            Action::SelectGroup => self.dialog = Some(Dialog::SelectGroup { select: true, pattern: "*".into(), files_only: true, case: false }),
            Action::UnselectGroup => self.dialog = Some(Dialog::SelectGroup { select: false, pattern: "*".into(), files_only: true, case: false }),
            Action::InvertSelection => self.act().invert_selection(),
            Action::Chmod => {
                let sel = self.act().selection();
                if let Some(first) = sel.first() {
                    let mode = first.mode & 0o7777;
                    self.dialog = Some(Dialog::Chmod { paths: sel.iter().map(|e| e.path.clone()).collect(), mode });
                }
            }
            Action::Symlink | Action::Link | Action::RelSymlink => {
                if let Some(e) = self.act().current().filter(|e| !e.is_updir()).cloned() {
                    let kind = match a {
                        Action::Link => LinkKind::Hard,
                        Action::RelSymlink => LinkKind::Relative,
                        _ => LinkKind::Symbolic,
                    };
                    let link = self.panels[1 - self.active].dir.join(&e.name);
                    let target = if kind == LinkKind::Relative {
                        relative_path(&e.path, link.parent().unwrap_or(Path::new("/"))).display().to_string()
                    } else {
                        e.path.display().to_string()
                    };
                    self.dialog = Some(Dialog::Link { kind, target, link: link.display().to_string() });
                }
            }
            Action::EditSymlink => match self.act().current().filter(|e| matches!(e.kind, Kind::Symlink | Kind::StaleLink)).cloned() {
                Some(e) => {
                    let target = fsutil::read_link(&e.path).unwrap_or_default();
                    self.dialog = Some(Dialog::EditSymlink { link: e.path.clone(), target });
                }
                None => self.err("The file under the cursor is not a symlink."),
            },
            Action::Chown | Action::AdvChown => {
                let sel = self.act().selection();
                if let Some(first) = sel.first() {
                    let mode = if a == Action::AdvChown { Some(first.mode & 0o7777) } else { None };
                    self.dialog = Some(Dialog::Chown {
                        paths: sel.iter().map(|e| e.path.clone()).collect(),
                        owner: fsutil::user_name(first.uid),
                        group: fsutil::group_name(first.gid),
                        mode,
                    });
                }
            }
            Action::ViewFile => self.dialog = Some(Dialog::Input { kind: InputKind::ViewFile, value: String::new() }),
            Action::FilteredView => self.dialog = Some(Dialog::Input { kind: InputKind::FilteredView, value: String::new() }),
            Action::QuickCd => self.dialog = Some(Dialog::Input { kind: InputKind::QuickCd, value: String::new() }),
            Action::ExternalPanelize => self.dialog = Some(Dialog::Input { kind: InputKind::ExternalPanelize, value: "find . -type f -mtime -7".into() }),
            Action::Tree(p) => {
                let panel = &mut self.panels[p];
                panel.view = if panel.view == PanelView::Tree { PanelView::Listing } else { PanelView::Tree };
                if self.panels[p].view == PanelView::Tree {
                    self.active = 1 - p;
                }
            }
            Action::DirTree => {
                let o = 1 - self.active;
                self.panels[o].view = PanelView::Tree;
            }
            Action::Panelize(p) => match self.last_panelize.clone() {
                Some((dir, list)) => {
                    let panel = &mut self.panels[p];
                    let _ = panel.chdir(&dir, true);
                    panel.panelized = Some(list);
                    panel.reload_keep(None);
                    panel.view = PanelView::Listing;
                    self.active = p;
                }
                None => self.err("Nothing to panelize yet.\nUse Find file or External panelize first."),
            },
            Action::CompareFiles => {
                let a1 = self.panels[0].current().filter(|e| !e.is_dir_like()).map(|e| e.path.clone());
                let a2 = self.panels[1].current().filter(|e| !e.is_dir_like()).map(|e| e.path.clone());
                match (a1, a2) {
                    (Some(x), Some(y)) => {
                        let cmd = format!("/usr/bin/diff -u {} {}", shell_quote(&x.display().to_string()), shell_quote(&y.display().to_string()));
                        self.run_command(ctx, cmd);
                    }
                    _ => self.err("Put the cursor on a file in both panels to compare them."),
                }
            }
            Action::AllDirSizes => {
                let p = &self.panels[self.active];
                let marked: Vec<PathBuf> = p.entries.iter().filter(|e| e.marked && e.is_dir_like()).map(|e| e.path.clone()).collect();
                let list = if marked.is_empty() {
                    p.entries.iter().filter(|e| !e.is_updir() && e.kind == Kind::Dir).map(|e| e.path.clone()).collect()
                } else {
                    marked
                };
                if !list.is_empty() && self.job.is_none() {
                    self.job = Some(Job { kind: JobKind::DirSizes { panel: self.active }, shared: ops::start_dir_sizes(list) });
                }
            }
            Action::CommandHistory => self.dialog = Some(Dialog::History { files: false, cursor: self.settings.command_history.len().saturating_sub(1) }),
            Action::FileHistory => self.dialog = Some(Dialog::History { files: true, cursor: self.settings.file_history.len().saturating_sub(1) }),
            Action::BackgroundJobs => {
                let t = match &self.job {
                    Some(j) => format!("Running: {}", j.shared.lock().unwrap().title),
                    None => "No background jobs are running.".to_string(),
                };
                self.dialog = Some(Dialog::Message { title: "Background jobs".into(), text: t, error: false });
            }
            Action::EditMenuFile => {
                let f = crate::settings::menu_file();
                if !f.exists() {
                    let _ = std::fs::create_dir_all(crate::settings::config_dir());
                    let _ = std::fs::write(&f, crate::settings::DEFAULT_MENU);
                }
                self.open_editor(&f);
            }
            Action::UserCommand(i) => {
                if let Some((_, cmd)) = crate::settings::user_menu_entries().get(i).cloned() {
                    let name_of = |p: Option<PathBuf>| p.map(|p| shell_quote(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())).unwrap_or_default();
                    let cur = self.panels[self.active].current().filter(|e| !e.is_updir()).map(|e| e.path.clone());
                    let oth = self.panels[1 - self.active].current().filter(|e| !e.is_updir()).map(|e| e.path.clone());
                    let cmd = cmd
                        .replace("%F", &oth.map(|p| shell_quote(&p.display().to_string())).unwrap_or_default())
                        .replace("%D", &shell_quote(&self.panels[1 - self.active].dir.display().to_string()))
                        .replace("%f", &name_of(cur))
                        .replace("%d", &shell_quote(&self.panels[self.active].dir.display().to_string()));
                    self.run_command(ctx, cmd);
                }
            }
            Action::Layout => self.dialog = Some(Dialog::Options { page: OptPage::Layout, s: Box::new(self.settings.clone()) }),
            Action::PanelOptions => self.dialog = Some(Dialog::Options { page: OptPage::PanelOptions, s: Box::new(self.settings.clone()) }),
            Action::Confirmation => self.dialog = Some(Dialog::Options { page: OptPage::Confirmation, s: Box::new(self.settings.clone()) }),
            Action::QuickViewOptions => self.dialog = Some(Dialog::Options { page: OptPage::QuickView, s: Box::new(self.settings.clone()) }),
            Action::Appearance => self.dialog = Some(Dialog::Options { page: OptPage::Appearance, s: Box::new(self.settings.clone()) }),
            Action::Unavailable => {
                self.dialog = Some(Dialog::Message {
                    title: "Not available".into(),
                    text: "This Midnight Commander feature (virtual file systems and terminal settings) is not part of myCommander yet.".into(),
                    error: false,
                })
            }

            Action::CompareDirs => self.compare_dirs(ctx),
            Action::ShowOutput => self.screen = Screen::Output,
            Action::OtherPanelHere => {
                let d = self.act().dir.clone();
                let o = 1 - self.active;
                if let Err(e) = self.panels[o].chdir(&d, true) {
                    self.err(e);
                }
            }
            Action::Config => {
                let mut st = self.settings.clone();
                st.show_hidden = self.panels[0].show_hidden;
                self.dialog = Some(Dialog::Options { page: OptPage::Configuration, s: Box::new(st) })
            }
            Action::SaveSetup => match self.save_setup() {
                Ok(()) => self.flash(ctx, "Setup saved"),
                Err(e) => self.err(format!("Cannot save setup\n{}", e)),
            },
            Action::About => self.dialog = Some(Dialog::About),
            Action::OpenFinder => {
                let target = self.act().current().filter(|e| !e.is_updir()).map(|e| e.path.clone());
                let mut c = std::process::Command::new("/usr/bin/open");
                match target {
                    Some(t) => c.arg("-R").arg(t),
                    None => c.arg(&self.panels[self.active].dir),
                };
                let _ = c.spawn();
            }
            Action::OpenTerminal => {
                let _ = std::process::Command::new("/usr/bin/open").arg("-a").arg("Terminal").arg(&self.panels[self.active].dir).spawn();
            }
            Action::QuickLook => {
                if let Some(e) = self.act().current().filter(|e| !e.is_updir()).cloned() {
                    let _ = std::process::Command::new("/usr/bin/qlmanage")
                        .arg("-p")
                        .arg(&e.path)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
            Action::CopyPath => {
                let dir = self.panels[self.active].dir.clone();
                let p = self.act().current().filter(|e| !e.is_updir()).map(|e| e.path.clone()).unwrap_or(dir);
                ctx.copy_text(p.display().to_string());
                self.flash(ctx, "Path copied to the clipboard");
            }
            Action::Zip => {
                let sel = self.act().selection();
                if sel.is_empty() {
                    return;
                }
                let name = if sel.len() == 1 { format!("{}.zip", sel[0].name) } else { "Archive.zip".into() };
                let mut cmd = format!("/usr/bin/zip -r -y {}", shell_quote(&name));
                for e in &sel {
                    cmd.push(' ');
                    cmd.push_str(&shell_quote(&e.name));
                }
                self.run_command(ctx, cmd);
            }
        }
    }

    pub fn insert_cmd(&mut self, s: &str) {
        if !self.cmdline.is_empty() && !self.cmdline.ends_with(' ') {
            self.cmdline.push(' ');
        }
        self.cmdline.push_str(s);
        self.cmdline.push(' ');
    }

    pub fn history_step(&mut self, d: isize) {
        let len = self.settings.command_history.len();
        if len == 0 {
            return;
        }
        let pos = match self.cmd_hist_pos {
            None if d < 0 => len - 1,
            None => return,
            Some(p) => {
                let n = p as isize + d;
                if n < 0 {
                    0
                } else if n as usize >= len {
                    self.cmd_hist_pos = None;
                    self.cmdline.clear();
                    return;
                } else {
                    n as usize
                }
            }
        };
        self.cmd_hist_pos = Some(pos);
        self.cmdline = self.settings.command_history[pos].clone();
    }

    fn handle_xmap(&mut self, ctx: &egui::Context) {
        let mut done = true;
        let o = 1 - self.active;
        if take(ctx, Key::C, NONE) {
            self.do_action(ctx, Action::Chmod);
        } else if take(ctx, Key::S, CTRL) {
            self.do_action(ctx, Action::EditSymlink);
        } else if take(ctx, Key::D, CTRL) {
            self.do_action(ctx, Action::CompareFiles);
        } else if take(ctx, Key::S, NONE) {
            self.do_action(ctx, Action::Symlink);
        } else if take(ctx, Key::L, NONE) {
            self.do_action(ctx, Action::Link);
        } else if take(ctx, Key::V, NONE) {
            self.do_action(ctx, Action::RelSymlink);
        } else if take(ctx, Key::O, NONE) {
            self.do_action(ctx, Action::Chown);
        } else if take(ctx, Key::J, NONE) {
            self.do_action(ctx, Action::BackgroundJobs);
        } else if take_text(ctx, "!") {
            self.do_action(ctx, Action::ExternalPanelize);
        } else if take(ctx, Key::D, NONE) {
            self.do_action(ctx, Action::CompareDirs);
        } else if take(ctx, Key::I, NONE) {
            self.do_action(ctx, Action::Info(o));
        } else if take(ctx, Key::Q, NONE) {
            self.do_action(ctx, Action::QuickView(o));
        } else if take(ctx, Key::H, NONE) {
            self.do_action(ctx, Action::HotlistAdd);
        } else if take(ctx, Key::P, NONE) {
            let d = self.act().dir.display().to_string();
            self.insert_cmd(&shell_quote(&d));
        } else if take(ctx, Key::Escape, NONE) {
        } else {
            done = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
        }
        if done {
            self.xmap = false;
            drop_text(ctx);
        }
    }

    fn handle_quick_search(&mut self, ctx: &egui::Context) -> bool {
        let mut texts = Vec::new();
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                if let egui::Event::Text(t) = e {
                    texts.push(t.clone());
                    false
                } else {
                    true
                }
            })
        });
        for t in texts {
            let p = self.act();
            let mut q = p.quick_search.clone().unwrap_or_default();
            q.push_str(&t);
            p.quick_search = Some(q.clone());
            if !p.quick_search_jump(false) {
                q.pop();
                p.quick_search = Some(q);
            }
        }
        if take(ctx, Key::Backspace, NONE) {
            if let Some(q) = self.act().quick_search.as_mut() {
                q.pop();
            }
        }
        if take(ctx, Key::S, CTRL) || take(ctx, Key::S, ALT) {
            self.act().quick_search_jump(true);
        }
        if take(ctx, Key::Escape, NONE) {
            self.act().quick_search = None;
        }
        if take(ctx, Key::Enter, NONE) {
            self.act().quick_search = None;
            self.enter(ctx);
            return true;
        }
        let nav = [Key::ArrowUp, Key::ArrowDown, Key::PageUp, Key::PageDown, Key::Tab, Key::Home, Key::End];
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { key, pressed: true, .. } if nav.contains(key)))) {
            self.act().quick_search = None;
        }
        false
    }

    /// Keys of the file manager (mc.default.keymap [filemanager] and [panel]).
    pub fn handle_main_keys(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.menu.is_some() {
            return;
        }
        if self.xmap {
            self.handle_xmap(ctx);
            return;
        }
        if self.act().quick_search.is_some() && self.handle_quick_search(ctx) {
            return;
        }
        if take(ctx, Key::F4, SHIFT) || take(ctx, Key::F14, NONE) {
            self.do_action(ctx, Action::EditNew);
            return;
        }
        let fkeys = [
            (Key::F1, Action::Help),
            (Key::F2, Action::UserMenu),
            (Key::F3, Action::View),
            (Key::F4, Action::Edit),
            (Key::F5, Action::Copy),
            (Key::F6, Action::Move),
            (Key::F7, Action::Mkdir),
            (Key::F8, Action::Delete),
            (Key::F9, Action::Menu),
            (Key::F10, Action::Quit),
        ];
        for (k, a) in fkeys {
            if take(ctx, k, NONE) {
                self.do_action(ctx, a);
                return;
            }
        }
        let shortcuts: [(Key, M, Action); 22] = [
            (Key::R, CTRL, Action::Reread),
            (Key::R, CMD, Action::Reread),
            (Key::U, CTRL, Action::Swap),
            (Key::Period, ALT, Action::ToggleHidden),
            (Key::Period, CMD_SHIFT, Action::ToggleHidden),
            (Key::Space, CTRL, Action::DirSize),
            (Key::Questionmark, ALT, Action::Find),
            (Key::Slash, ALT_SHIFT, Action::Find),
            (Key::F, CMD, Action::Find),
            (Key::Backslash, CTRL, Action::Hotlist),
            (Key::D, CMD, Action::Hotlist),
            (Key::T, ALT, Action::CycleListing),
            (Key::O, CTRL, Action::ShowOutput),
            (Key::O, ALT, Action::OtherPanelHere),
            (Key::Backspace, CMD, Action::Delete),
            (Key::C, ALT, Action::QuickCd),
            (Key::Plus, ALT, Action::SelectGroup),
            (Key::Equals, ALT, Action::SelectGroup),
            (Key::Minus, ALT, Action::UnselectGroup),
            (Key::Comma, CMD, Action::Config),
            (Key::Q, CMD, Action::Quit),
            (Key::N, CMD_SHIFT, Action::Mkdir),
        ];
        for (k, m, a) in shortcuts {
            if take(ctx, k, m) {
                self.do_action(ctx, a);
                return;
            }
        }
        if take(ctx, Key::X, CTRL) {
            self.xmap = true;
            return;
        }
        if take(ctx, Key::H, ALT) {
            self.do_action(ctx, Action::CommandHistory);
            return;
        }
        if take(ctx, Key::E, ALT_SHIFT) {
            self.do_action(ctx, Action::FileHistory);
            return;
        }
        if take(ctx, Key::Num1, ALT_SHIFT) || take(ctx, Key::Exclamationmark, ALT) {
            self.do_action(ctx, Action::FilteredView);
            return;
        }
        // Lynx-like motion (Options > Panel options): Left = parent folder, Right = open folder.
        if self.settings.lynx_motion && self.cmdline.is_empty() && self.act().listing != Listing::Brief {
            if take(ctx, Key::ArrowLeft, NONE) {
                if let Err(e) = self.act().go_parent() {
                    self.err(e);
                }
                return;
            }
            if take(ctx, Key::ArrowRight, NONE) {
                if self.act().current().map(|e| e.is_dir_like()).unwrap_or(false) {
                    self.enter(ctx);
                }
                return;
            }
        }
        if take(ctx, Key::S, CTRL) || take(ctx, Key::S, ALT) {
            self.act().quick_search = Some(String::new());
            return;
        }
        if take(ctx, Key::Tab, NONE) || take(ctx, Key::I, CTRL) {
            let other = 1 - self.active;
            if self.panels[other].view == PanelView::Listing {
                self.active = other;
            }
            return;
        }
        let rows = self.act().page_rows.max(1) as isize;
        if take(ctx, Key::ArrowUp, SHIFT) {
            self.act().toggle_mark_current();
            self.act().move_cursor(-1);
        }
        if take(ctx, Key::ArrowDown, SHIFT) || take(ctx, Key::Insert, NONE) || take(ctx, Key::T, CTRL) {
            self.act().toggle_mark_current();
            self.act().move_cursor(1);
        }
        if take(ctx, Key::ArrowUp, NONE) || take(ctx, Key::P, CTRL) {
            self.act().move_cursor(-1);
        }
        if take(ctx, Key::ArrowDown, NONE) || take(ctx, Key::N, CTRL) {
            self.act().move_cursor(1);
        }
        if take(ctx, Key::PageUp, CTRL) || take(ctx, Key::ArrowUp, CMD) {
            if let Err(e) = self.act().go_parent() {
                self.err(e);
            }
        }
        if take(ctx, Key::PageDown, CTRL) || take(ctx, Key::ArrowDown, CMD) {
            if self.act().current().map(|e| e.is_dir_like()).unwrap_or(false) {
                self.enter(ctx);
            }
        }
        if take(ctx, Key::PageUp, NONE) || take(ctx, Key::V, ALT) {
            self.act().move_cursor(-(rows - 1).max(1));
        }
        if take(ctx, Key::PageDown, NONE) || take(ctx, Key::V, CTRL) {
            self.act().move_cursor((rows - 1).max(1));
        }
        if self.cmdline.is_empty() {
            if self.act().listing == Listing::Brief {
                if take(ctx, Key::ArrowLeft, NONE) {
                    self.act().move_cursor(-rows);
                }
                if take(ctx, Key::ArrowRight, NONE) {
                    self.act().move_cursor(rows);
                }
            }
            if take(ctx, Key::Home, NONE) {
                self.act().home();
            }
            if take(ctx, Key::End, NONE) {
                self.act().end();
            }
        }
        if take(ctx, Key::Y, ALT) || take(ctx, Key::OpenBracket, CMD) {
            if let Err(e) = self.act().history_back() {
                self.err(e);
            }
        }
        if take(ctx, Key::U, ALT) || take(ctx, Key::CloseBracket, CMD) {
            if let Err(e) = self.act().history_forward() {
                self.err(e);
            }
        }
        if take(ctx, Key::I, ALT) {
            let d = self.act().dir.clone();
            let o = 1 - self.active;
            if let Err(e) = self.panels[o].chdir(&d, true) {
                self.err(e);
            }
        }
        if take(ctx, Key::Enter, ALT) || take(ctx, Key::Enter, CTRL) {
            if let Some(e) = self.act().current().filter(|e| !e.is_updir()).cloned() {
                self.insert_cmd(&shell_quote(&e.name));
            }
        }
        if take(ctx, Key::A, ALT) {
            let d = self.act().dir.display().to_string();
            self.insert_cmd(&shell_quote(&d));
        }
        if take(ctx, Key::P, ALT) {
            self.history_step(-1);
        }
        if take(ctx, Key::N, ALT) {
            self.history_step(1);
        }
        if take(ctx, Key::Enter, NONE) {
            if self.cmdline.trim().is_empty() {
                self.enter(ctx);
            } else {
                let c = self.cmdline.clone();
                self.run_command(ctx, c);
            }
        }
        if take(ctx, Key::Escape, NONE) {
            if !self.cmdline.is_empty() {
                self.cmdline.clear();
            } else {
                self.act().unmark_all();
            }
        }
        // On an empty command line + - * select like mc; Space marks (Mac keyboards lack Insert).
        if self.cmdline.is_empty() {
            if take_text(ctx, "+") {
                self.do_action(ctx, Action::SelectGroup);
                return;
            }
            if take_text(ctx, "-") || take_text(ctx, "\\") {
                self.do_action(ctx, Action::UnselectGroup);
                return;
            }
            if take_text(ctx, "*") {
                self.act().invert_selection();
            }
            if take_text(ctx, " ") {
                self.act().toggle_mark_current();
                self.act().move_cursor(1);
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.char_w = ctx.fonts(|f| f.glyph_width(&FontId::monospace(self.settings.font_size), 'M'));
        self.refresh_theme(ctx);
        self.handle_screenshot(ctx);
        self.poll_native_menu(ctx);

        // Closing the window saves the setup too.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_asked && self.screenshot.is_none() && self.settings.auto_save {
            let _ = self.save_setup();
        }

        if self.draw_viewer(ctx) || self.draw_editor(ctx) || self.draw_output(ctx) {
            return;
        }

        let job_busy = self.job.is_some();
        if !job_busy {
            self.handle_main_keys(ctx);
        }

        let mut menubar_bottom = 0.0;
        if self.settings.show_toolbar {
            menubar_bottom += self.draw_toolbar(ctx);
        }
        if self.settings.show_menubar || !self.settings.show_toolbar {
            menubar_bottom = self.draw_menubar(ctx, menubar_bottom);
        }
        let labels: Vec<(&str, Option<u8>)> = vec![
            ("Help", Some(1)),
            ("Menu", Some(2)),
            ("View", Some(3)),
            ("Edit", Some(4)),
            ("Copy", Some(5)),
            ("RenMov", Some(6)),
            ("Mkdir", Some(7)),
            ("Delete", Some(8)),
            ("PullDn", Some(9)),
            ("Quit", Some(10)),
        ];
        let hit = if self.settings.show_keybar { crate::ui_main::draw_bar(ctx, "buttonbar", self.settings.font_size, &self.pal, &labels) } else { None };
        if self.settings.show_cmdline {
            self.draw_cmdline(ctx);
        }
        self.draw_panels(ctx);
        self.draw_toast(ctx);

        if let Some(n) = hit {
            let a = match n {
                1 => Action::Help,
                2 => Action::UserMenu,
                3 => Action::View,
                4 => Action::Edit,
                5 => Action::Copy,
                6 => Action::Move,
                7 => Action::Mkdir,
                8 => Action::Delete,
                9 => Action::Menu,
                _ => Action::Quit,
            };
            if self.dialog.is_none() && !job_busy {
                self.do_action(ctx, a);
            }
        }

        if self.job.is_some() {
            self.draw_job(ctx);
        } else {
            self.draw_menu_popup(ctx, menubar_bottom);
            self.draw_dialog(ctx);
        }
    }
}

/// `target` written relative to the folder `from` (for Relative symlink).
pub fn relative_path(target: &Path, from: &Path) -> PathBuf {
    let t: Vec<_> = target.components().collect();
    let f: Vec<_> = from.components().collect();
    let common = t.iter().zip(f.iter()).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..f.len() {
        out.push("..");
    }
    for c in &t[common..] {
        out.push(c.as_os_str());
    }
    out
}
