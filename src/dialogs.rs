//! Dialog boxes (mc style: grey boxes, red for errors and deletes) and the
//! progress window of a running operation with its overwrite question.

use crate::app::{Action, App, Dialog, JobKind};
use crate::fsutil;
use crate::ops::{self, Answer, OpKind};
use crate::panel::{Listing, SortMode};
use crate::util::*;
use egui::{Align2, Color32, FontId, Key, Sense, Stroke, Vec2};
use std::path::{Path, PathBuf};

type Then = Box<dyn FnOnce(&mut App, &egui::Context)>;

fn field(ui: &mut egui::Ui, text: &mut String, width: f32) -> egui::Response {
    ui.add(egui::TextEdit::singleline(text).desired_width(width).font(FontId::monospace(14.0)))
}

impl App {
    /// A macOS-style alert / sheet: rounded, soft shadow, bold title; errors and
    /// deletes get a red warning badge instead of mc's red box.
    pub fn dialog_window<R>(&self, ctx: &egui::Context, title: &str, error: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
        let pal = self.pal;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("dim")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(if pal.dark { 90 } else { 40 }));
            ui.allocate_rect(screen, Sense::click());
        });
        let r = egui::Window::new(title)
            .id(egui::Id::new(("dialog", title)))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 90.0))
            .order(egui::Order::Foreground)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(pal.menu_bg)
                    .stroke(if pal.lcars { Stroke::new(2.0_f32, pal.control_border) } else { Stroke::new(0.5_f32, pal.control_border) })
                    .corner_radius(if pal.lcars { 18.0 } else { 12.0 })
                    .inner_margin(egui::Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(400.0);
                if pal.lcars {
                    self.lcars_dialog_title(ui, title, error);
                    return add(ui);
                }
                ui.horizontal(|ui| {
                    if error {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 11.0, pal.danger);
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, "!", FontId::proportional(15.0), Color32::WHITE);
                    }
                    ui.label(egui::RichText::new(title).size(15.0).strong().color(pal.text));
                });
                ui.add_space(8.0);
                add(ui)
            });
        r.and_then(|r| r.inner)
    }

    pub fn draw_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let enter = pressed(ctx, Key::Enter);
        let esc = pressed(ctx, Key::Escape) || pressed(ctx, Key::F10);
        let mut keep: Option<Dialog> = None;
        let mut then: Option<Then> = None;

        match dialog {
            Dialog::Message { title, text, error } => {
                let ok = self
                    .dialog_window(ctx, &title, error, |ui| {
                        ui.label(&text);
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, &self.pal, "OK").clicked()).inner
                    })
                    .unwrap_or(false);
                if !(ok || enter || esc) {
                    keep = Some(Dialog::Message { title, text, error });
                }
            }
            Dialog::Mkdir { mut name } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Create a new Directory", false, |ui| {
                        ui.label("Enter directory name:");
                        field(ui, &mut name, f32::INFINITY).request_focus();
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if (ok || enter) && !name.trim().is_empty() {
                    let n = name.trim().to_string();
                    then = Some(Box::new(move |app, _| {
                        let dir = app.act().dir.clone();
                        let path = if crate::vfs::is_remote(&dir) {
                            crate::vfs::join(&dir, &n)
                        } else if n.starts_with('/') {
                            PathBuf::from(&n)
                        } else {
                            dir.join(&n)
                        };
                        let made = if crate::vfs::is_remote(&path) {
                            // one level at a time, like create_dir_all
                            let base = crate::vfs::join(&dir, "");
                            let mut cur = crate::vfs::normalize(&base);
                            let mut r = Ok(());
                            for part in n.split('/').filter(|s| !s.is_empty()) {
                                cur = crate::vfs::join(&cur, part);
                                if !crate::vfs::exists(&cur) {
                                    r = crate::vfs::mkdir(&cur);
                                    if r.is_err() {
                                        break;
                                    }
                                }
                            }
                            r
                        } else {
                            std::fs::create_dir_all(&path)
                        };
                        match made {
                            Ok(()) => {
                                let first = Path::new(&n).components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or(n.clone());
                                app.act().reload_keep(Some(first));
                                let o = 1 - app.active;
                                app.panels[o].reload();
                            }
                            Err(e) => app.err(format!("Cannot create directory \"{}\"\n{}", path.display(), e)),
                        }
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Mkdir { name });
                }
            }
            Dialog::CopyMove { kind, sources, mut mask, mut dest } => {
                // mc prompt: "%o %f \"%s\" with source mask:" / "%o %d %f with source mask:"
                let what = if sources.len() == 1 {
                    let e = &sources[0];
                    format!("{} {} \"{}\" with source mask:", kind.label(), if e.is_dir_like() { "directory" } else { "file" }, e.name)
                } else {
                    format!("{} {} files/directories with source mask:", kind.label(), sources.len())
                };
                let title = kind.label().to_string();
                let first = !ctx.memory(|m| m.focused().is_some());
                let (ok, cancel) = self
                    .dialog_window(ctx, &title, false, |ui| {
                        ui.label(&what);
                        field(ui, &mut mask, 560.0);
                        ui.add_space(4.0);
                        ui.label("to:");
                        let r = field(ui, &mut dest, 560.0);
                        if first {
                            r.request_focus();
                        }
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if (ok || enter) && !dest.trim().is_empty() {
                    let base = self.act().dir.clone();
                    let d = dest.trim();
                    let d = if d.starts_with("sftp://") || d.starts_with("ftp://") {
                        crate::vfs::normalize(Path::new(d))
                    } else if let Some(rest) = d.strip_prefix("~/") {
                        fsutil::home_dir().join(rest)
                    } else if crate::vfs::is_remote(&base) && !d.starts_with('/') {
                        crate::vfs::join(&base, d.trim_end_matches('/'))
                    } else {
                        base.join(d)
                    };
                    let m = mask.trim().to_string();
                    let srcs: Vec<PathBuf> =
                        sources.iter().filter(|e| m.is_empty() || m == "*" || fsutil::wildcard_match(&m, &e.name, false)).map(|e| e.path.clone()).collect();
                    if srcs.is_empty() {
                        then = Some(Box::new(|app, _| app.err("No file matches the source mask.")));
                    } else {
                        then = Some(Box::new(move |app, _| app.start_op(kind, srcs, Some(d))));
                    }
                } else if !(cancel || esc) {
                    keep = Some(Dialog::CopyMove { kind, sources, mask, dest });
                }
            }
            Dialog::Delete { sources, text } => {
                let (yes, no) = self
                    .dialog_window(ctx, "Delete", true, |ui| {
                        ui.label(&text);
                        ui.label(egui::RichText::new("This can't be undone.").color(self.pal.text_dim));
                        ui.add_space(12.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let y = ui
                                .add(
                                    egui::Button::new(egui::RichText::new("Delete").color(Color32::WHITE))
                                        .fill(self.pal.danger)
                                        .corner_radius(6.0_f32)
                                        .min_size(Vec2::new(78.0, 24.0)),
                                )
                                .clicked();
                            let n = plain_button(ui, "Cancel").clicked();
                            (y, n)
                        })
                        .inner
                    })
                    .unwrap_or((false, false));
                if yes || enter {
                    then = Some(Box::new(move |app, _| app.start_op(OpKind::Delete, sources, None)));
                } else if !(no || esc) {
                    keep = Some(Dialog::Delete { sources, text });
                }
            }
            Dialog::SelectGroup { select, mut pattern, mut files_only, mut case } => {
                let title = if select { "Select" } else { "Unselect" };
                let (ok, cancel) = self
                    .dialog_window(ctx, title, false, |ui| {
                        field(ui, &mut pattern, f32::INFINITY).request_focus();
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut files_only, "Files only");
                            ui.checkbox(&mut case, "Case sensitive");
                        });
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| app.act().select_group(&pattern, select, files_only, case)));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::SelectGroup { select, pattern, files_only, case });
                }
            }
            Dialog::Filter { panel, mut pattern } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Filter", false, |ui| {
                        ui.label("Show only files matching (folders are always shown):");
                        field(ui, &mut pattern, f32::INFINITY).request_focus();
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    let p = pattern.trim().to_string();
                    then = Some(Box::new(move |app, _| {
                        app.panels[panel].filter = if p.is_empty() || p == "*" { None } else { Some(p) };
                        app.panels[panel].reload();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Filter { panel, pattern });
                }
            }
            Dialog::Sort { panel, mut opts } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Sort order", false, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                for m in SortMode::ALL {
                                    ui.radio_value(&mut opts.mode, m, m.label());
                                }
                            });
                            ui.add_space(30.0);
                            ui.vertical(|ui| {
                                ui.checkbox(&mut opts.exec_first, "Executable first");
                                ui.checkbox(&mut opts.case_sensitive, "Case sensitive");
                                ui.checkbox(&mut opts.reverse, "Reverse");
                                ui.checkbox(&mut opts.mix_all, "Mix files and folders");
                            });
                        });
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        app.panels[panel].sort = opts;
                        app.panels[panel].resort();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Sort { panel, opts });
                }
            }
            Dialog::Listing { panel, mut listing } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Listing format", false, |ui| {
                        for l in [Listing::Full, Listing::Brief, Listing::Long] {
                            ui.radio_value(&mut listing, l, l.label());
                        }
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| app.panels[panel].listing = listing));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Listing { panel, listing });
                }
            }
            Dialog::Chmod { paths, mut mode } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Chmod command", false, |ui| {
                        let name = if paths.len() == 1 {
                            paths[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                        } else {
                            format!("{} files", paths.len())
                        };
                        ui.label(format!("Permission for {}", name));
                        ui.add_space(4.0);
                        let bits: [(u32, &str); 12] = [
                            (0o4000, "set user ID on execution"),
                            (0o2000, "set group ID on execution"),
                            (0o1000, "sticky bit"),
                            (0o400, "read by owner"),
                            (0o200, "write by owner"),
                            (0o100, "execute/search by owner"),
                            (0o040, "read by group"),
                            (0o020, "write by group"),
                            (0o010, "execute/search by group"),
                            (0o004, "read by others"),
                            (0o002, "write by others"),
                            (0o001, "execute/search by others"),
                        ];
                        for (b, label) in bits {
                            let mut on = mode & b != 0;
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    mode |= b;
                                } else {
                                    mode &= !b;
                                }
                            }
                        }
                        ui.label(egui::RichText::new(format!("Octal: {:04o}", mode)).monospace());
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        for p in &paths {
                            if let Err(e) = fsutil::set_mode(p, mode) {
                                app.err(format!("Cannot chmod \"{}\"\n{}", p.display(), e));
                                break;
                            }
                        }
                        app.act().unmark_all();
                        app.act().reload();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Chmod { paths, mode });
                }
            }
            Dialog::Find { mut start, mut pattern, mut content, mut case, mut skip_hidden } => {
                let first = !ctx.memory(|m| m.focused().is_some());
                let (ok, cancel) = self
                    .dialog_window(ctx, "Find File", false, |ui| {
                        ui.label("Start at:");
                        field(ui, &mut start, 560.0);
                        ui.label("File name:");
                        let r = field(ui, &mut pattern, 560.0);
                        if first {
                            r.request_focus();
                        }
                        ui.label("Content:");
                        field(ui, &mut content, 560.0);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut case, "Case sensitive");
                            ui.checkbox(&mut skip_hidden, "Skip hidden");
                        });
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    let fp = ops::FindParams { start: PathBuf::from(start.trim()), pattern, content, case_sensitive: case, skip_hidden };
                    keep = Some(Dialog::FindResults { shared: ops::start_find(fp), cursor: 0 });
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Find { start, pattern, content, case, skip_hidden });
                }
            }
            Dialog::FindResults { shared, mut cursor } => {
                let (found, finished, current) = {
                    let g = shared.lock().unwrap();
                    (g.found.clone(), g.finished, g.current.clone())
                };
                if !finished {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                let moved = ctx.input(|i| i.key_pressed(Key::ArrowDown) || i.key_pressed(Key::ArrowUp));
                if take(ctx, Key::ArrowDown, NONE) && cursor + 1 < found.len() {
                    cursor += 1;
                }
                if take(ctx, Key::ArrowUp, NONE) {
                    cursor = cursor.saturating_sub(1);
                }
                let mut chosen: Option<PathBuf> = None;
                let mut close = false;
                let mut panelize = false;
                self.dialog_window(ctx, "Find File", false, |ui| {
                    ui.set_min_width(640.0);
                    egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, false]).show(ui, |ui| {
                        for (i, p) in found.iter().enumerate() {
                            let r = ui.selectable_label(i == cursor, egui::RichText::new(fsutil::tilde(p)).monospace());
                            if i == cursor && moved {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                cursor = i;
                            }
                            if r.double_clicked() {
                                chosen = Some(p.clone());
                            }
                        }
                    });
                    ui.separator();
                    if finished {
                        ui.label(format!("Finished. {} found.", found.len()));
                    } else {
                        ui.label(format!("Searching {} ...", truncate_left(&current, 60)));
                    }
                    ui.horizontal(|ui| {
                        if crate::util::lbutton(ui, "Chdir").clicked() {
                            chosen = found.get(cursor).cloned();
                        }
                        if !finished && crate::util::lbutton(ui, "Stop").clicked() {
                            shared.lock().unwrap().cancel = true;
                        }
                        if crate::util::lbutton(ui, "Panelize").on_hover_text("Show these files in the panel").clicked() {
                            panelize = true;
                        }
                        if crate::util::lbutton(ui, "Quit").clicked() {
                            close = true;
                        }
                    });
                });
                if enter {
                    chosen = found.get(cursor).cloned();
                }
                if panelize {
                    shared.lock().unwrap().cancel = true;
                    let list = found.clone();
                    then = Some(Box::new(move |app, ctx| {
                        let dir = app.act().dir.clone();
                        app.last_panelize = Some((dir, list));
                        let p = app.active;
                        app.do_action(ctx, Action::Panelize(p));
                    }));
                } else if let Some(p) = chosen {
                    shared.lock().unwrap().cancel = true;
                    then = Some(Box::new(move |app, _| {
                        if let Some(parent) = p.parent() {
                            app.chdir_active(parent);
                            if let Some(n) = p.file_name() {
                                let n = n.to_string_lossy().to_string();
                                app.act().select_name(&n);
                            }
                        }
                    }));
                } else if close || esc {
                    shared.lock().unwrap().cancel = true;
                } else {
                    keep = Some(Dialog::FindResults { shared, cursor });
                }
            }
            Dialog::Hotlist { mut cursor, mut mounts, hosts } => {
                #[derive(Clone)]
                enum Item {
                    Mount(usize),
                    Server(String),
                    Fav(usize),
                }
                let favs = self.settings.hotlist.clone();
                let mut items: Vec<Item> = (0..mounts.len()).map(Item::Mount).collect();
                items.extend(hosts.iter().cloned().map(Item::Server));
                items.extend((0..favs.len()).map(Item::Fav));
                if take(ctx, Key::ArrowDown, NONE) && cursor + 1 < items.len() {
                    cursor += 1;
                }
                if take(ctx, Key::ArrowUp, NONE) {
                    cursor = cursor.saturating_sub(1);
                }
                cursor = cursor.min(items.len().saturating_sub(1));
                let mut go: Option<Item> = None;
                let (mut add, mut remove, mut eject, mut close) = (false, false, false, take(ctx, Key::E, CMD));
                let dim = self.pal.text_dim;
                self.dialog_window(ctx, "Places", false, |ui| {
                    ui.set_min_width(560.0);
                    let section = |ui: &mut egui::Ui, t: &str| {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(t).strong().color(dim));
                    };
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        let mut last = 0;
                        for (i, it) in items.iter().enumerate() {
                            let kind = match it {
                                Item::Mount(_) => 1,
                                Item::Server(_) => 2,
                                Item::Fav(_) => 3,
                            };
                            if kind != last {
                                section(ui, ["", "Mounted", "Servers (~/.ssh/config)", "Favorites"][kind]);
                                last = kind;
                            }
                            let text = match it {
                                Item::Mount(m) => {
                                    let m = &mounts[*m];
                                    let icon = if m.network { "🌐" } else if m.is_root { "💻" } else { "💾" };
                                    let free = if m.total > 0 { format!("{} free", fsutil::size_human(m.free)) } else { String::new() };
                                    format!("{icon} {:<22} {:<22} {}", crate::util::truncate_middle(&m.name, 22), crate::util::truncate_middle(&m.kind(), 22), free)
                                }
                                Item::Server(h) => format!("🔑 {h}"),
                                Item::Fav(f) => fsutil::tilde(&favs[*f]),
                            };
                            let r = ui.selectable_label(i == cursor, egui::RichText::new(text).monospace());
                            if r.clicked() {
                                cursor = i;
                            }
                            if r.double_clicked() {
                                go = Some(it.clone());
                            }
                        }
                        if items.is_empty() {
                            ui.label("Nothing here yet. Use \"Add current\" to remember a folder.");
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if crate::util::lbutton(ui, "Go").clicked() {
                            go = items.get(cursor).cloned();
                        }
                        if crate::util::lbutton(ui, "Eject").on_hover_text("Unmount the selected volume (⌘E)").clicked() {
                            eject = true;
                        }
                        if crate::util::lbutton(ui, "Add current").clicked() {
                            add = true;
                        }
                        if crate::util::lbutton(ui, "Remove").clicked() {
                            remove = true;
                        }
                        if crate::util::lbutton(ui, "Cancel").clicked() {
                            close = true;
                        }
                    });
                });
                if enter {
                    go = items.get(cursor).cloned();
                }
                let cur = items.get(cursor).cloned();
                let tm = self.settings.show_timemachine;
                if add {
                    let d = self.act().dir.clone();
                    if !self.settings.hotlist.contains(&d) {
                        self.settings.hotlist.push(d);
                        let _ = self.settings.save();
                    }
                    keep = Some(Dialog::Hotlist { cursor, mounts, hosts });
                } else if remove {
                    if let Some(Item::Fav(f)) = cur {
                        if f < self.settings.hotlist.len() {
                            self.settings.hotlist.remove(f);
                            let _ = self.settings.save();
                        }
                        cursor = cursor.saturating_sub(1);
                    }
                    keep = Some(Dialog::Hotlist { cursor, mounts, hosts });
                } else if eject {
                    if let Some(Item::Mount(m)) = cur {
                        crate::mounts::eject(mounts[m].path.clone(), ctx.clone());
                        mounts = crate::mounts::list(tm);
                        keep = Some(Dialog::Hotlist { cursor, mounts, hosts });
                    } else {
                        keep = Some(Dialog::Hotlist { cursor, mounts, hosts });
                    }
                } else if let Some(it) = go {
                    match it {
                        Item::Mount(m) => {
                            let p = mounts[m].path.clone();
                            then = Some(Box::new(move |app, _| app.chdir_active(&p)));
                        }
                        Item::Fav(f) => {
                            let p = favs[f].clone();
                            then = Some(Box::new(move |app, _| app.chdir_active(&p)));
                        }
                        Item::Server(h) => {
                            then = Some(Box::new(move |app, ctx| {
                                let panel = app.active;
                                app.connect_remote(ctx, panel, crate::vfs::Scheme::Sftp, h)
                            }));
                        }
                    }
                } else if !(close || esc) {
                    keep = Some(Dialog::Hotlist { cursor, mounts, hosts });
                }
            }
            Dialog::Help { mut page } => {
                const PAGES: [&str; 3] = ["Welcome", "Keys", "Remote control"];
                if take(ctx, Key::ArrowRight, NONE) && page + 1 < PAGES.len() {
                    page += 1;
                }
                if take(ctx, Key::ArrowLeft, NONE) {
                    page = page.saturating_sub(1);
                }
                let level = self.settings.mcp_level;
                let (mut copy_cmd, mut copy_json, mut open_settings) = (false, false, false);
                let ok = self
                    .dialog_window(ctx, "Help", false, |ui| {
                        ui.set_width(680.0);
                        ui.horizontal(|ui| {
                            for (i, t) in PAGES.iter().enumerate() {
                                let label = egui::RichText::new(*t).strong().color(if self.pal.lcars && page == i { self.pal.on_fill } else { ui.visuals().text_color() });
                                if ui.selectable_label(page == i, label).clicked() {
                                    page = i;
                                }
                            }
                        });
                        ui.separator();
                        if page == 2 {
                            let running = crate::control::server_running();
                            let status = if level == crate::settings::McpLevel::Off {
                                "Remote control is OFF. Choose a level in Settings ▸ General ▸ Remote control.".to_string()
                            } else if running {
                                format!("Remote control is ON – level \"{}\". Socket: {}", level.name(), crate::control::socket_path().display())
                            } else {
                                format!("Remote control level \"{}\" – starting …", level.name())
                            };
                            ui.label(egui::RichText::new(status).strong().color(if level == crate::settings::McpLevel::Off { self.pal.danger } else { self.pal.accent }));
                            ui.horizontal(|ui| {
                                if crate::util::lbutton(ui, "Copy command for Claude Code").clicked() {
                                    copy_cmd = true;
                                }
                                if crate::util::lbutton(ui, "Copy JSON configuration").clicked() {
                                    copy_json = true;
                                }
                                if crate::util::lbutton(ui, "Open Settings").clicked() {
                                    open_settings = true;
                                }
                            });
                            ui.add_space(4.0);
                        }
                        // A fixed-size box: wide content (code, tables) is clipped and never widens the window.
                        let (area_rect, _) = ui.allocate_exact_size(egui::vec2(680.0, 430.0), Sense::hover());
                        let mut area_ui = ui.new_child(egui::UiBuilder::new().max_rect(area_rect));
                        area_ui.set_clip_rect(area_rect.intersect(ui.clip_rect()));
                        egui::ScrollArea::vertical().max_height(430.0).auto_shrink([false, false]).show(&mut area_ui, |ui| match page {
                            0 => {
                                egui_commonmark::CommonMarkViewer::new().show(ui, &mut self.help_cache.borrow_mut(), crate::control::help_welcome());
                            }
                            1 => {
                                egui::Grid::new("help").num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
                                    for (k, v) in HELP {
                                        if k.is_empty() {
                                            ui.label(egui::RichText::new(*v).strong().color(self.pal.accent));
                                            ui.label("");
                                        } else {
                                            ui.label(egui::RichText::new(*k).monospace());
                                            ui.label(*v);
                                        }
                                        ui.end_row();
                                    }
                                });
                            }
                            _ => {
                                egui_commonmark::CommonMarkViewer::new().show(ui, &mut self.help_cache.borrow_mut(), &crate::control::mcp_help_text());
                            }
                        });
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, &self.pal, "Done").clicked()).inner
                    })
                    .unwrap_or(false);
                if copy_cmd {
                    ctx.copy_text(crate::control::claude_code_command());
                    self.flash(ctx, "Command copied – paste it into a terminal");
                }
                if copy_json {
                    ctx.copy_text(crate::control::json_config());
                    self.flash(ctx, "JSON configuration copied");
                }
                if open_settings {
                    then = Some(Box::new(|app, _| app.dialog = Some(Dialog::Options { page: crate::app::OptPage::General, s: Box::new(app.settings.clone()) })));
                } else if !(ok || esc || enter) {
                    keep = Some(Dialog::Help { page });
                }
            }
            Dialog::RemoteAsk { id, title, lines } => {
                let mut allow = enter;
                let mut deny = esc;
                self.dialog_window(ctx, &title, true, |ui| {
                    ui.set_min_width(520.0);
                    ui.label(egui::RichText::new("An AI agent connected through remote control asks for your permission:").weak());
                    ui.add_space(6.0);
                    for l in &lines {
                        ui.label(egui::RichText::new(l).monospace());
                    }
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(ui, &self.pal, "Allow").clicked() {
                            allow = true;
                        }
                        if crate::util::lbutton(ui, "Deny").clicked() {
                            deny = true;
                        }
                    });
                });
                if allow || deny {
                    let ok = allow && !deny;
                    then = Some(Box::new(move |app, ctx| app.remote_decide(ctx, id, ok)));
                } else {
                    keep = Some(Dialog::RemoteAsk { id, title, lines });
                }
            }
            Dialog::About => {
                let ok = self
                    .dialog_window(ctx, "About", false, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("myCommander").size(22.0).strong().color(self.pal.text));
                            ui.label("A native two-panel file manager for macOS");
                            ui.label("in the spirit of the classic orthodox file managers.");
                            ui.add_space(6.0);
                            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                            ui.add_space(8.0);
                            ui.label("Made by Kay Mühlena");
                            ui.hyperlink_to("github.com/say-kaj/MyCommander", "https://github.com/say-kaj/MyCommander");
                            ui.label(egui::RichText::new("Free software – bug reports and ideas are welcome on GitHub.").weak().size(12.0));
                            ui.add_space(10.0);
                            primary_button(ui, &self.pal, "OK").clicked()
                        })
                        .inner
                    })
                    .unwrap_or(false);
                if !(ok || enter || esc) {
                    keep = Some(Dialog::About);
                }
            }
            other => {
                let (k, t) = self.draw_more_dialog(ctx, other, enter, esc);
                keep = k;
                then = t;
            }
        }
        if keep.is_some() {
            self.dialog = keep;
        }
        if let Some(f) = then {
            f(self, ctx);
            ctx.request_repaint(); // panels were drawn earlier in this frame
        }
        // Keys a dialog used must not reach the panels.
        drop_keys(ctx, &[Key::Enter, Key::Escape, Key::F10]);
    }

    pub fn draw_job(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.as_ref() else { return };
        let shared = job.shared.clone();
        let finished = shared.lock().unwrap().finished;
        if finished {
            // The panels were drawn before this point in the frame: draw again with the reloaded lists.
            ctx.request_repaint();
            let job = self.job.take().unwrap();
            let (errors, size, out, code) = {
                let g = shared.lock().unwrap();
                (g.errors.clone(), g.dir_size, g.output.clone(), g.exit_code)
            };
            match job.kind {
                JobKind::FileOp(_) => {
                    for p in self.panels.iter_mut() {
                        p.unmark_all();
                    }
                    self.reload_both();
                    if !errors.is_empty() {
                        let mut t = errors.iter().take(8).cloned().collect::<Vec<_>>().join("\n\n");
                        if errors.len() > 8 {
                            t.push_str(&format!("\n\n... and {} more", errors.len() - 8));
                        }
                        self.err(t);
                    }
                }
                JobKind::DirSize { panel, name } => {
                    if let Some(e) = self.panels[panel].entries.iter_mut().find(|e| e.name == name) {
                        e.dir_size = size;
                    }
                    self.panels[panel].move_cursor(1);
                    if self.panels[panel].sort.mode == SortMode::Size {
                        self.panels[panel].resort();
                    }
                }
                JobKind::DirSizes { panel } => {
                    let sizes = shared.lock().unwrap().sizes.clone();
                    for (path, size) in sizes {
                        if let Some(e) = self.panels[panel].entries.iter_mut().find(|e| e.path == path) {
                            e.dir_size = Some(size);
                        }
                    }
                    if self.panels[panel].sort.mode == SortMode::Size {
                        self.panels[panel].resort();
                    }
                }
                JobKind::Panelize { panel } => {
                    let dir = self.panels[panel].dir.clone();
                    let list: Vec<PathBuf> = out
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty())
                        .map(|l| {
                            let l = l.strip_prefix("./").unwrap_or(l);
                            if l.starts_with('/') { PathBuf::from(l) } else { dir.join(l) }
                        })
                        .filter(|p| std::fs::symlink_metadata(p).is_ok())
                        .collect();
                    if list.is_empty() {
                        self.err("The command listed no existing files.");
                    } else {
                        self.last_panelize = Some((dir, list));
                        self.do_action(ctx, Action::Panelize(panel));
                    }
                }
                JobKind::Shell => {
                    self.last_output.push_str(&out);
                    if let Some(c) = code {
                        if c != 0 {
                            self.last_output.push_str(&format!("\n[exit code {}]\n", c));
                        }
                    }
                    self.reload_both();
                    self.screen = crate::app::Screen::Output;
                }
            }
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(60));
        let is_fileop = matches!(job.kind, JobKind::FileOp(_));
        let label = match &job.kind {
            JobKind::FileOp(k) => k.label().to_string(),
            JobKind::DirSize { .. } | JobKind::DirSizes { .. } => "Directory size".into(),
            JobKind::Panelize { .. } => "External panelize".into(),
            JobKind::Shell => "Running command".into(),
        };
        let g = shared.lock().unwrap();
        let question = g.question.clone();
        let (cur, fd, ft, bd, bt, fbd, fbt, title) =
            (g.current.clone(), g.files_done, g.files_total, g.bytes_done, g.bytes_total, g.file_bytes_done, g.file_bytes_total, g.title.clone());
        let elapsed = g.started.map(|s| s.elapsed().as_secs_f32()).unwrap_or(0.0);
        drop(g);

        if let Some(q) = question {
            let mut answer: Option<Answer> = None;
            self.dialog_window(ctx, "File exists", true, |ui| {
                ui.label(&q.text);
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&q.src_info).monospace());
                ui.label(egui::RichText::new(&q.dst_info).monospace());
                ui.add_space(8.0);
                ui.label("Overwrite this file?");
                ui.horizontal(|ui| {
                    if crate::util::lbutton(ui, "Yes").clicked() {
                        answer = Some(Answer::Yes);
                    }
                    if crate::util::lbutton(ui, "No").clicked() {
                        answer = Some(Answer::No);
                    }
                });
                ui.label("Overwrite all files?");
                ui.horizontal(|ui| {
                    for (t, a) in [("All", Answer::All), ("None", Answer::None), ("Skip", Answer::Skip), ("Abort", Answer::Abort)] {
                        if crate::util::lbutton(ui, t).clicked() {
                            answer = Some(a);
                        }
                    }
                });
            });
            if take(ctx, Key::Escape, NONE) {
                answer = Some(Answer::Abort);
            }
            if let Some(a) = answer {
                let mut g = shared.lock().unwrap();
                if a == Answer::Abort {
                    g.cancel = true;
                }
                g.answer = Some(a);
            }
            return;
        }
        if !is_fileop && elapsed < 0.4 {
            return;
        }
        let mut abort = false;
        let title_text = if is_fileop { label.clone() } else { title };
        self.dialog_window(ctx, &title_text, false, |ui| {
            ui.set_min_width(480.0);
            if is_fileop {
                ui.label(egui::RichText::new(truncate_left(&cur, 64)).monospace());
                ui.add(egui::ProgressBar::new(if fbt > 0 { fbd as f32 / fbt as f32 } else { 0.0 }).desired_height(10.0));
                ui.add_space(6.0);
                ui.label(format!("Files processed: {} / {}", fd, ft));
                let frac = if bt > 0 {
                    bd as f32 / bt as f32
                } else if ft > 0 {
                    fd as f32 / ft as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(frac).show_percentage());
                let speed = if elapsed > 0.5 { bd as f32 / elapsed } else { 0.0 };
                ui.label(format!(
                    "{} of {}   ·   {}/s   ·   {:.0} s",
                    fsutil::size_human(bd),
                    fsutil::size_human(bt),
                    fsutil::size_human(speed as u64),
                    elapsed
                ));
            } else {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(format!("{} ...", label));
                });
            }
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                if crate::util::lbutton(ui, "  Abort  ").clicked() {
                    abort = true;
                }
            });
        });
        if abort || take(ctx, Key::Escape, NONE) {
            shared.lock().unwrap().cancel = true;
            if !is_fileop {
                // A shell command or size count can't be stopped midway; stop waiting for it.
                self.job = None;
            }
        }
    }
}
