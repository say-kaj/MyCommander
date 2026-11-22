//! The dialogs behind mc's remaining menu items: Link / Symlink / Relative
//! symlink, Edit symlink, Chown and Advanced chown, View file, Filtered view,
//! Quick cd, External panelize, Command / file history, and the Options pages
//! (one Settings window).

use crate::app::{Action, App, Dialog, InputKind, Job, JobKind, LinkKind, OptPage};
use crate::fsutil;
use crate::ops;
use crate::settings::{Settings, ThemeChoice};
use crate::util::*;
use egui::{FontId, Key};
use std::path::{Path, PathBuf};

pub type Then = Box<dyn FnOnce(&mut App, &egui::Context)>;

fn field(ui: &mut egui::Ui, text: &mut String, width: f32) -> egui::Response {
    ui.add(egui::TextEdit::singleline(text).desired_width(width).font(FontId::monospace(14.0)))
}

fn uid_of(name: &str) -> Option<u32> {
    if let Ok(n) = name.parse() {
        return Some(n);
    }
    let c = std::ffi::CString::new(name).ok()?;
    unsafe {
        let pw = libc::getpwnam(c.as_ptr());
        if pw.is_null() {
            None
        } else {
            Some((*pw).pw_uid)
        }
    }
}

fn gid_of(name: &str) -> Option<u32> {
    if let Ok(n) = name.parse() {
        return Some(n);
    }
    let c = std::ffi::CString::new(name).ok()?;
    unsafe {
        let gr = libc::getgrnam(c.as_ptr());
        if gr.is_null() {
            None
        } else {
            Some((*gr).gr_gid)
        }
    }
}

fn chown(path: &std::path::Path, uid: u32, gid: u32) -> Result<(), String> {
    let c = std::ffi::CString::new(path.as_os_str().to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    let r = unsafe { libc::lchown(c.as_ptr(), uid, gid) };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

impl App {
    /// Handles the dialogs added for the full mc menu. Returns the dialog to keep
    /// open (if any) and an action to run after it closes.
    pub fn draw_more_dialog(&mut self, ctx: &egui::Context, dialog: Dialog, enter: bool, esc: bool) -> (Option<Dialog>, Option<Then>) {
        let mut keep: Option<Dialog> = None;
        let mut then: Option<Then> = None;
        match dialog {
            Dialog::Link { kind, mut target, mut link } => {
                let title = match kind {
                    LinkKind::Hard => "Link",
                    LinkKind::Symbolic => "Symbolic link",
                    LinkKind::Relative => "Relative symlink",
                };
                let (ok, cancel) = self
                    .dialog_window(ctx, title, false, |ui| {
                        ui.label(match kind {
                            LinkKind::Hard => "Existing file:",
                            _ => "Existing filename (filename symlink will point to):",
                        });
                        field(ui, &mut target, 560.0);
                        ui.label(if kind == LinkKind::Hard { "Link name:" } else { "Symbolic link filename:" });
                        field(ui, &mut link, 560.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        let r = match kind {
                            LinkKind::Hard => std::fs::hard_link(&target, &link),
                            _ => std::os::unix::fs::symlink(&target, &link),
                        };
                        if let Err(e) = r {
                            app.err(format!("Cannot create link \"{}\"\n{}", link, e));
                        }
                        app.reload_both();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Link { kind, target, link });
                }
            }
            Dialog::EditSymlink { link, mut target } => {
                let name = link.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let (ok, cancel) = self
                    .dialog_window(ctx, "Edit symlink", false, |ui| {
                        ui.label(format!("Symlink '{}' points to:", name));
                        field(ui, &mut target, 560.0).request_focus();
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        let tmp = link.with_extension("mycmd-tmp-link");
                        let r = std::os::unix::fs::symlink(&target, &tmp).and_then(|_| std::fs::rename(&tmp, &link));
                        if let Err(e) = r {
                            app.err(format!("edit symlink: {}", e));
                        }
                        app.reload_both();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::EditSymlink { link, target });
                }
            }
            Dialog::Chown { paths, mut owner, mut group, mut mode } => {
                let title = if mode.is_some() { "Advanced chown" } else { "Chown command" };
                let (ok, cancel) = self
                    .dialog_window(ctx, title, false, |ui| {
                        let what = if paths.len() == 1 {
                            paths[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                        } else {
                            format!("{} files", paths.len())
                        };
                        ui.label(format!("Owner and group for {}", what));
                        egui::Grid::new("chown").num_columns(2).show(ui, |ui| {
                            ui.label("Owner:");
                            field(ui, &mut owner, 240.0);
                            ui.end_row();
                            ui.label("Group:");
                            field(ui, &mut group, 240.0);
                            ui.end_row();
                        });
                        if let Some(m) = mode.as_mut() {
                            ui.add_space(6.0);
                            ui.label("Permissions:");
                            egui::Grid::new("adv_perm").num_columns(4).show(ui, |ui| {
                                ui.label("");
                                ui.label("Read");
                                ui.label("Write");
                                ui.label("Execute");
                                ui.end_row();
                                for (who, shift) in [("Owner", 6u32), ("Group", 3), ("Others", 0)] {
                                    ui.label(who);
                                    for bit in [4u32, 2, 1] {
                                        let b = bit << shift;
                                        let mut on = *m & b != 0;
                                        if ui.checkbox(&mut on, "").changed() {
                                            if on {
                                                *m |= b;
                                            } else {
                                                *m &= !b;
                                            }
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                            ui.label(egui::RichText::new(format!("Octal: {:04o}", *m)).monospace());
                        }
                        ui.label(egui::RichText::new("Changing the owner usually needs administrator rights.").small());
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        let (Some(uid), Some(gid)) = (uid_of(owner.trim()), gid_of(group.trim())) else {
                            app.err("Unknown owner or group.");
                            return;
                        };
                        for p in &paths {
                            if let Err(e) = chown(p, uid, gid) {
                                app.err(format!("Cannot chown \"{}\"\n{}", p.display(), e));
                                break;
                            }
                            if let Some(m) = mode {
                                if let Err(e) = fsutil::set_mode(p, m) {
                                    app.err(format!("Cannot chmod \"{}\"\n{}", p.display(), e));
                                    break;
                                }
                            }
                        }
                        app.act().unmark_all();
                        app.reload_both();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Chown { paths, owner, group, mode });
                }
            }
            Dialog::Input { kind, mut value } => {
                let (title, label) = match kind {
                    InputKind::ViewFile => ("View file", "Filename:"),
                    InputKind::FilteredView => ("Filtered view", "Filter command and arguments (the file name is added):"),
                    InputKind::QuickCd => ("Quick cd", "cd"),
                    InputKind::ExternalPanelize => ("External panelize", "Command (its output lines become the panel's files):"),
                };
                let (ok, cancel) = self
                    .dialog_window(ctx, title, false, |ui| {
                        ui.label(label);
                        field(ui, &mut value, 520.0).request_focus();
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if (ok || enter) && !value.trim().is_empty() {
                    let v = value.trim().to_string();
                    then = Some(Box::new(move |app, ctx| app.finish_input(ctx, kind, v)));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Input { kind, value });
                }
            }
            Dialog::Remote { panel, scheme, mut value, hosts } => {
                let sftp = scheme == crate::vfs::Scheme::Sftp;
                let title = if sftp { "SFTP link" } else { "FTP link" };
                let mut picked: Option<String> = None;
                let dim = self.pal.text_dim;
                let (ok, cancel) = self
                    .dialog_window(ctx, title, false, |ui| {
                        ui.set_min_width(520.0);
                        ui.label(if sftp {
                            "Host – an alias from ~/.ssh/config or user@host – optionally followed by /path:"
                        } else {
                            "[user@]host[/path] – the password is read from ~/.netrc:"
                        });
                        field(ui, &mut value, 520.0).request_focus();
                        if !hosts.is_empty() {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("Hosts from ~/.ssh/config (click to pick):").color(dim));
                            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                                for h in &hosts {
                                    if ui.selectable_label(false, egui::RichText::new(h).monospace()).clicked() {
                                        picked = Some(h.clone());
                                    }
                                }
                            });
                        }
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if let Some(h) = picked {
                    value = h;
                }
                if (ok || enter) && !value.trim().is_empty() {
                    let v = value.trim().to_string();
                    then = Some(Box::new(move |app, ctx| app.connect_remote(ctx, panel, scheme, v)));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Remote { panel, scheme, value, hosts });
                }
            }
            Dialog::Encoding { mut choice } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Encoding", false, |ui| {
                        ui.set_min_width(420.0);
                        ui.label("Text encoding for the viewer, the editor and quick view:");
                        ui.add_space(4.0);
                        for e in crate::textenc::TextEncoding::ALL {
                            ui.radio_value(&mut choice, e, e.label());
                        }
                        ui.add_space(8.0);
                        ok_cancel(ui)
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, _| {
                        app.settings.text_encoding = choice;
                        let _ = app.settings.save();
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::Encoding { choice });
                }
            }
            Dialog::History { files, mut cursor } => {
                let list: Vec<String> = if files {
                    self.settings.file_history.iter().map(|p| fsutil::tilde(p)).collect()
                } else {
                    self.settings.command_history.clone()
                };
                if take(ctx, Key::ArrowDown, NONE) && cursor + 1 < list.len() {
                    cursor += 1;
                }
                if take(ctx, Key::ArrowUp, NONE) {
                    cursor = cursor.saturating_sub(1);
                }
                let mut pick: Option<(usize, bool)> = None; // (index, edit)
                let mut close = false;
                let title = if files { "Viewed/edited files history" } else { "Command history" };
                self.dialog_window(ctx, title, false, |ui| {
                    ui.set_min_width(560.0);
                    if list.is_empty() {
                        ui.label("Nothing here yet.");
                    }
                    egui::ScrollArea::vertical().max_height(360.0).stick_to_bottom(true).show(ui, |ui| {
                        for (i, t) in list.iter().enumerate() {
                            let r = ui.selectable_label(i == cursor, egui::RichText::new(t).monospace());
                            if r.clicked() {
                                cursor = i;
                            }
                            if r.double_clicked() {
                                pick = Some((i, false));
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if files {
                            if ui.button("View").clicked() {
                                pick = Some((cursor, false));
                            }
                            if ui.button("Edit").clicked() {
                                pick = Some((cursor, true));
                            }
                        } else if ui.button("Use").clicked() {
                            pick = Some((cursor, false));
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
                if enter && !list.is_empty() {
                    pick = Some((cursor, false));
                }
                if let Some((i, edit)) = pick {
                    if files {
                        if let Some(p) = self.settings.file_history.get(i).cloned() {
                            then = Some(Box::new(move |app, _| {
                                if edit {
                                    app.open_editor(&p);
                                } else {
                                    app.open_viewer(&p);
                                }
                            }));
                        }
                    } else if let Some(c) = self.settings.command_history.get(i).cloned() {
                        self.cmdline = c;
                    }
                } else if !(close || esc) {
                    keep = Some(Dialog::History { files, cursor });
                }
            }
            Dialog::Options { page, mut s } => {
                // One Settings window; changes apply immediately (macOS preferences style).
                let before = serde_json::to_string(&*s).unwrap_or_default();
                let mut page = page;
                let done = self
                    .dialog_window(ctx, "Settings", false, |ui| {
                        let line = self.pal.control_border;
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(124.0);
                                ui.set_height(380.0);
                                for p in OptPage::ALL {
                                    let r = ui.add_sized([124.0, 28.0], egui::SelectableLabel::new(page == p, p.label()));
                                    if r.clicked() {
                                        page = p;
                                    }
                                }
                            });
                            ui.add_space(6.0);
                            let (sep, _) = ui.allocate_exact_size(egui::Vec2::new(1.0, 380.0), egui::Sense::hover());
                            ui.painter().rect_filled(sep, 0.0, line);
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                ui.set_width(430.0);
                                egui::ScrollArea::vertical().max_height(380.0).min_scrolled_height(380.0).auto_shrink([false, false]).show(ui, |ui| {
                                    ui.set_width(414.0);
                                    ui.spacing_mut().item_spacing.y = 7.0;
                                    options_page(ui, page, &mut s);
                                });
                            });
                        });
                        ui.add_space(6.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| primary_button(ui, &self.pal, "Done").clicked()).inner
                    })
                    .unwrap_or(false);
                let changed = serde_json::to_string(&*s).unwrap_or_default() != before;
                if done || enter || esc {
                    if changed {
                        then = Some(Box::new(move |app, ctx| app.apply_settings(ctx, *s)));
                    }
                } else if changed {
                    let applied = s.clone();
                    then = Some(Box::new(move |app, ctx| app.apply_settings(ctx, *applied)));
                    keep = Some(Dialog::Options { page, s });
                } else {
                    keep = Some(Dialog::Options { page, s });
                }
            }
            Dialog::ConfirmQuit => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "The Midnight Commander", false, |ui| {
                        ui.label("Do you really want to quit myCommander?");
                        ok_cancel_labeled(ui, "Quit")
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(|app, ctx| {
                        app.quit_asked = true;
                        app.do_action(ctx, Action::Quit);
                    }));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::ConfirmQuit);
                }
            }
            Dialog::ConfirmExec { name } => {
                let (ok, cancel) = self
                    .dialog_window(ctx, "Open", false, |ui| {
                        ui.label(format!("Do you really want to execute \"{}\"?", name));
                        ok_cancel_labeled(ui, "Run")
                    })
                    .unwrap_or((false, false));
                if ok || enter {
                    then = Some(Box::new(move |app, ctx| app.run_command(ctx, format!("./{}", shell_quote(&name)))));
                } else if !(cancel || esc) {
                    keep = Some(Dialog::ConfirmExec { name });
                }
            }
            other => keep = Some(other),
        }
        (keep, then)
    }

    pub fn finish_input(&mut self, ctx: &egui::Context, kind: InputKind, v: String) {
        let base = self.act().dir.clone();
        let resolve = |v: &str| -> PathBuf {
            if v == "~" {
                fsutil::home_dir()
            } else if let Some(r) = v.strip_prefix("~/") {
                fsutil::home_dir().join(r)
            } else if v.starts_with("sftp://") || v.starts_with("ftp://") {
                crate::vfs::normalize(Path::new(v))
            } else if crate::vfs::is_remote(&base) && !v.starts_with('/') {
                if v == ".." {
                    crate::vfs::parent(&base).unwrap_or_else(|| base.clone())
                } else {
                    crate::vfs::join(&base, v.trim_end_matches('/'))
                }
            } else {
                base.join(v)
            }
        };
        match kind {
            InputKind::ViewFile => {
                let p = resolve(&v);
                self.open_viewer(&p);
            }
            InputKind::FilteredView => {
                if let Some(e) = self.act().current().filter(|e| !e.is_dir_like()).cloned() {
                    self.run_command(ctx, format!("{} {}", v, shell_quote(&e.name)));
                } else {
                    self.err("Put the cursor on a file first.");
                }
            }
            InputKind::QuickCd => {
                let p = resolve(&v);
                self.chdir_active(&p);
            }
            InputKind::ExternalPanelize => {
                if self.job.is_none() {
                    self.job = Some(Job { kind: JobKind::Panelize { panel: self.active }, shared: ops::start_shell(v, base) });
                }
            }
        }
    }

    /// OK in one of the Options pages.
    pub fn apply_settings(&mut self, ctx: &egui::Context, s: Settings) {
        let hidden_changed = s.show_hidden != self.panels[0].show_hidden;
        let mix_changed = s.mix_all_files != self.settings.mix_all_files;
        let style_changed = s.font_size != self.settings.font_size || s.theme != self.settings.theme;
        // Keep things the dialogs don't edit.
        let keep_hist = std::mem::take(&mut self.settings.command_history);
        let keep_files = std::mem::take(&mut self.settings.file_history);
        let keep_hot = std::mem::take(&mut self.settings.hotlist);
        self.settings = s;
        self.settings.command_history = keep_hist;
        self.settings.file_history = keep_files;
        self.settings.hotlist = keep_hot;
        if hidden_changed {
            let v = self.settings.show_hidden;
            for p in self.panels.iter_mut() {
                p.show_hidden = v;
                p.reload();
            }
        }
        for p in self.panels.iter_mut() {
            p.sizes_ver = 0;
        }
        if mix_changed {
            for p in self.panels.iter_mut() {
                p.sort.mix_all = self.settings.mix_all_files;
                p.resort();
            }
        }
        if style_changed {
            // Force the palette to be picked again for the chosen theme.
            self.pal.dark = !self.pal.dark;
            self.refresh_theme(ctx);
            self.restyle(ctx);
        }
        let _ = self.settings.save();
    }

    pub fn open_viewer(&mut self, p: &std::path::Path) {
        match crate::screens::Viewer::open(p, self.settings.text_encoding) {
            Ok(v) => {
                self.settings.remember_file(p);
                self.screen = crate::app::Screen::Viewer(Box::new(v));
            }
            Err(e) => self.err(e),
        }
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).strong().size(13.0));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).weak().size(11.5));
}

fn options_page(ui: &mut egui::Ui, page: OptPage, s: &mut Settings) {
    match page {
        OptPage::General => {
            heading(ui, "Startup and updates");
            ui.checkbox(&mut s.auto_save, "Remember folders and settings when quitting");
            ui.checkbox(&mut s.check_updates, "Check for new versions at startup");
            heading(ui, "Ask before");
            ui.checkbox(&mut s.confirm_delete, "Deleting files");
            ui.checkbox(&mut s.confirm_overwrite, "Overwriting files");
            ui.checkbox(&mut s.confirm_execute, "Running programs");
            ui.checkbox(&mut s.confirm_exit, "Quitting");
        }
        OptPage::Panels => {
            heading(ui, "Listing");
            ui.checkbox(&mut s.show_hidden, "Show hidden files");
            ui.checkbox(&mut s.dir_sizes, "Calculate folder sizes in the background");
            ui.checkbox(&mut s.show_timemachine, "Show Time Machine volumes in Places");
            ui.horizontal(|ui| {
                ui.label("Mouse wheel");
                egui::ComboBox::from_id_salt("wheel_dir").selected_text(s.wheel_dir.label()).width(220.0).show_ui(ui, |ui| {
                    for d in crate::settings::WheelDir::ALL {
                        ui.selectable_value(&mut s.wheel_dir, d, d.label());
                    }
                });
            });
            ui.checkbox(&mut s.mix_all_files, "Don't list folders first");
            ui.checkbox(&mut s.show_mini_status, "Show info line below the list");
            heading(ui, "Arrangement");
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.horizontal_split, false, "Side by side");
                ui.radio_value(&mut s.horizontal_split, true, "One above the other");
            });
            heading(ui, "Keyboard");
            ui.checkbox(&mut s.lynx_motion, "Left goes up, Right opens (Lynx-style)");
        }
        OptPage::Appearance => {
            heading(ui, "Theme");
            ui.horizontal(|ui| {
                ui.radio_value(&mut s.theme, ThemeChoice::System, "Follow macOS");
                ui.radio_value(&mut s.theme, ThemeChoice::Light, "Light");
                ui.radio_value(&mut s.theme, ThemeChoice::Dark, "Dark");
            });
            ui.horizontal(|ui| {
                ui.label("Text size");
                ui.add(egui::Slider::new(&mut s.font_size, 11.0..=20.0).step_by(0.5));
            });
            heading(ui, "Window elements");
            ui.checkbox(&mut s.show_menubar, "Menu bar");
            ui.checkbox(&mut s.show_toolbar, "Toolbar");
            ui.checkbox(&mut s.show_cmdline, "Command line");
            ui.checkbox(&mut s.show_keybar, "Function key bar (F1–F10)");
        }
        OptPage::QuickView => {
            heading(ui, "Show");
            ui.checkbox(&mut s.qv_images, "Images as images");
            hint(ui, "png, jpg, gif, webp, bmp, tiff, ico, svg, heic, first page of pdf");
            ui.checkbox(&mut s.qv_markdown, "Rendered Markdown");
            ui.checkbox(&mut s.qv_highlight, "Syntax highlighting");
            hint(ui, "source code, HTML, shell, config files …");
            ui.add_enabled_ui(s.qv_highlight, |ui| {
                ui.checkbox(&mut s.qv_line_numbers, "Line numbers");
                for (label, field) in [("Light theme", &mut s.qv_theme_light), ("Dark theme", &mut s.qv_theme_dark)] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(label).selected_text(field.clone()).show_ui(ui, |ui| {
                            for t in crate::preview::THEMES {
                                ui.selectable_value(field, t.to_string(), t);
                            }
                        });
                    });
                }
                ui.label("Language overrides");
                hint(ui, "One `extension=Syntax name` per line, e.g. `conf=INI`");
                ui.add(egui::TextEdit::multiline(&mut s.qv_syntax_map).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
            });
            heading(ui, "Text");
            ui.horizontal(|ui| {
                ui.label("Encoding");
                encoding_combo(ui, "qv_enc", &mut s.text_encoding);
            });
            ui.horizontal(|ui| {
                ui.label("Read at most");
                ui.add(egui::DragValue::new(&mut s.qv_max_kb).range(16..=8192).speed(16).suffix(" KB"));
            });
        }
    }
}

fn encoding_combo(ui: &mut egui::Ui, id: &str, value: &mut crate::textenc::TextEncoding) {
    egui::ComboBox::from_id_salt(id).selected_text(value.label()).width(320.0).show_ui(ui, |ui| {
        for e in crate::textenc::TextEncoding::ALL {
            ui.selectable_value(value, e, e.label());
        }
    });
}
