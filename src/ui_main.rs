//! Main screen chrome in a macOS style: the unified toolbar in the title bar,
//! the pop-up menus (F9 / "⋯"), the command field, the function-key bar and
//! the Info / Quick View panel modes. The file lists are in `panel_view.rs`.

use crate::app::{Action, App, Dialog, MenuState};
use crate::menus::{items, TITLES};
use crate::fsutil::{self, Kind};
use crate::panel::PanelView;
use crate::theme::*;
use crate::util::*;
use egui::{Align2, FontId, Key, Pos2, Rect, Sense, Stroke, Vec2};

/// F2 user menu: myCommander's built-in entries, then the lines of the menu file
/// (Command > Edit menu file).
pub fn user_menu_entries_all() -> Vec<(String, String, Option<Action>, bool)> {
    let mut v: Vec<(String, String, Option<Action>, bool)> = vec![
        ("Show in Finder".into(), String::new(), Some(Action::OpenFinder), true),
        ("Open Terminal here".into(), String::new(), Some(Action::OpenTerminal), true),
        ("Quick Look".into(), String::new(), Some(Action::QuickLook), true),
        ("Copy full path".into(), String::new(), Some(Action::CopyPath), true),
        ("Compress to .zip".into(), String::new(), Some(Action::Zip), true),
    ];
    let custom = crate::settings::user_menu_entries();
    v.push((String::new(), String::new(), None, false));
    if custom.is_empty() {
        v.push(("Add your own commands…".into(), String::new(), Some(Action::EditMenuFile), true));
    } else {
        for (i, (label, _)) in custom.into_iter().enumerate() {
            v.push((label, String::new(), Some(Action::UserCommand(i)), true));
        }
        v.push((String::new(), String::new(), None, false));
        v.push(("Edit menu file…".into(), String::new(), Some(Action::EditMenuFile), true));
    }
    v
}

/// The function-key bar, macOS style: a small key cap ("F5") and a label.
pub fn draw_bar(ctx: &egui::Context, id: &str, fs: f32, pal: &Pal, labels: &[(&str, Option<u8>)]) -> Option<u8> {
    let mut hit = None;
    let h = fs + 16.0;
    egui::TopBottomPanel::bottom(id.to_string())
        .exact_height(h)
        .frame(egui::Frame::NONE.fill(pal.toolbar).stroke(Stroke::new(0.5_f32, pal.toolbar_line)))
        .show(ctx, |ui| {
            let full = ui.max_rect().shrink2(Vec2::new(8.0, 0.0));
            let w = full.width() / labels.len() as f32;
            for (i, (label, key)) in labels.iter().enumerate() {
                if label.is_empty() {
                    continue;
                }
                let r = Rect::from_min_size(Pos2::new(full.left() + i as f32 * w, full.top()), Vec2::new(w, h)).shrink2(Vec2::new(2.0, 4.0));
                let resp = ui.interact(r, ui.id().with((id, i)), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 6.0, pal.control_hover);
                }
                let cap_font = FontId::proportional(fs - 3.0);
                let cap_text = format!("F{}", i + 1);
                let g = ui.painter().layout_no_wrap(cap_text, cap_font, pal.text_dim);
                let cap = Rect::from_min_size(Pos2::new(r.left() + 6.0, r.center().y - 8.0), Vec2::new(g.size().x + 8.0, 16.0));
                ui.painter().rect_stroke(cap, 4.0, Stroke::new(1.0_f32, pal.control_border), egui::StrokeKind::Inside);
                ui.painter().galley(Pos2::new(cap.left() + 4.0, cap.center().y - g.size().y / 2.0), g, pal.text_dim);
                ui.painter().text(Pos2::new(cap.right() + 6.0, r.center().y), Align2::LEFT_CENTER, *label, FontId::proportional(fs - 0.5), pal.text);
                if resp.clicked() {
                    hit = *key;
                }
            }
        });
    hit
}

/// A rounded toolbar button; returns the response.
fn tool_button(ui: &mut egui::Ui, pal: &Pal, text: &str, selected: bool, tip: &str) -> egui::Response {
    let font = FontId::proportional(13.0);
    let g = ui.painter().layout_no_wrap(text.to_string(), font.clone(), pal.text);
    let size = Vec2::new(g.size().x + 20.0, 26.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected {
        pal.sel_inactive
    } else if resp.is_pointer_button_down_on() {
        pal.sel_inactive
    } else if resp.hovered() {
        pal.control_hover
    } else {
        pal.control
    };
    ui.painter().rect_filled(r, 6.0, fill);
    ui.painter().rect_stroke(r, 6.0, Stroke::new(0.5_f32, pal.control_border), egui::StrokeKind::Inside);
    ui.painter().galley(r.center() - g.size() / 2.0, g, pal.text);
    resp.on_hover_text(tip)
}

impl App {
    /// The unified toolbar next to the window buttons. Returns its bottom edge.
    pub fn draw_toolbar(&mut self, ctx: &egui::Context) -> f32 {
        let pal = self.pal;
        let height = 52.0;
        let mut action: Option<Action> = None;
        let mut search_go = false;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(height)
            .frame(egui::Frame::NONE.fill(pal.toolbar).stroke(Stroke::new(0.5_f32, pal.toolbar_line)))
            .show(ctx, |ui| {
                // Dragging the empty toolbar moves the window, double-click zooms (title bar behaviour).
                let bg = ui.interact(ui.max_rect(), ui.id().with("toolbar_bg"), Sense::click_and_drag());
                if bg.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if bg.double_clicked() {
                    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
                ui.horizontal_centered(|ui| {
                    ui.add_space(84.0); // room for the traffic-light buttons
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        let hidden = self.panels[0].show_hidden;
                        if tool_button(ui, &pal, if hidden { "Hide Hidden" } else { "Show Hidden" }, hidden, "Show or hide hidden files (⇧⌘.)").clicked() {
                            action = Some(Action::ToggleHidden);
                        }
                        let field = egui::TextEdit::singleline(&mut self.search_text)
                            .id(egui::Id::new("toolbar_search"))
                            .hint_text("🔍 Find files")
                            .desired_width(170.0)
                            .margin(egui::Margin::symmetric(8, 4));
                        let r = ui.add(field);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.search_text.trim().is_empty() {
                            search_go = true;
                        }
                    });
                });
            });
        if let Some(a) = action {
            if self.dialog.is_none() && self.job.is_none() {
                self.do_action(ctx, a);
            }
        }
        if search_go {
            let pat = self.search_text.trim();
            let pattern = if pat.contains('*') || pat.contains('?') { pat.to_string() } else { format!("*{}*", pat) };
            let fp = crate::ops::FindParams {
                start: self.panels[self.active].dir.clone(),
                pattern,
                content: String::new(),
                case_sensitive: false,
                skip_hidden: true,
            };
            self.dialog = Some(Dialog::FindResults { shared: crate::ops::start_find(fp), cursor: 0 });
            drop_keys(ctx, &[Key::Enter]);
        }
        height
    }

    /// mc's menu bar — Left, File, Command, Options, Right — in a macOS style.
    /// Click a title or press F9; arrows move between menus. Returns its bottom edge.
    pub fn draw_menubar(&mut self, ctx: &egui::Context, top: f32) -> f32 {
        let pal = self.pal;
        let as_titlebar = !self.settings.show_toolbar;
        let height = if as_titlebar { 40.0 } else { 30.0 };
        let mut clicked: Option<usize> = None;
        let fs = self.settings.font_size;
        egui::TopBottomPanel::top("mc_menubar")
            .exact_height(height)
            .frame(egui::Frame::NONE.fill(pal.window).stroke(Stroke::new(0.5_f32, pal.toolbar_line)))
            .show(ctx, |ui| {
                if as_titlebar {
                    let bg = ui.interact(ui.max_rect(), ui.id().with("menubar_drag"), Sense::click_and_drag());
                    if bg.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
                ui.horizontal_centered(|ui| {
                    ui.add_space(if as_titlebar { 84.0 } else { 10.0 });
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, t) in TITLES.iter().enumerate() {
                        let open = self.menu.as_ref().map(|m| !m.user && m.top == i).unwrap_or(false);
                        let font = FontId::proportional(fs);
                        let color = if open { pal.accent_text } else { pal.text };
                        let g = ui.painter().layout_no_wrap(t.to_string(), font, color);
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 20.0, 22.0), Sense::click());
                        if open {
                            ui.painter().rect_filled(r, 5.0, pal.accent);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 5.0, pal.control_hover);
                        }
                        ui.painter().galley(r.center() - g.size() / 2.0, g, color);
                        self.menu_x[i] = r.left();
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                        // Moving across the bar while a menu is open switches menus (like a real menu bar).
                        if resp.hovered() {
                            if let Some(m) = self.menu.as_mut() {
                                if !m.user && m.top != i {
                                    m.top = i;
                                    m.sel = 0;
                                }
                            }
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        let p = &self.panels[self.active];
                        let side = if self.active == 0 { "Left panel" } else { "Right panel" };
                        ui.label(egui::RichText::new(format!("{} · {}", side, p.sort.mode.label())).small().color(pal.text_faint));
                    });
                });
            });
        if let Some(i) = clicked {
            let same = self.menu.as_ref().map(|m| !m.user && m.top == i).unwrap_or(false);
            self.menu = if same { None } else { Some(MenuState { top: i, sel: 0, user: false }) };
        }
        top + height
    }

    pub fn draw_menu_popup(&mut self, ctx: &egui::Context, menubar_bottom: f32) {
        let Some(state) = self.menu.as_ref() else { return };
        let pal = self.pal;
        let user = state.user;
        let entries: Vec<(String, String, Option<Action>, bool)> = if user {
            user_menu_entries_all()
        } else {
            items(state.top).into_iter().map(|m| (m.label.to_string(), m.key.to_string(), m.action, m.enabled)).collect()
        };
        let selectable: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.2.is_some() && e.3).map(|(i, _)| i).collect();
        if selectable.is_empty() {
            self.menu = None;
            return;
        }
        let mut sel = state.sel;
        if !selectable.contains(&sel) {
            sel = selectable[0];
        }
        let mut top = state.top;
        let mut close = false;
        let mut run: Option<Action> = None;
        if take(ctx, Key::Escape, NONE) || take(ctx, Key::F10, NONE) || take(ctx, Key::F9, NONE) {
            close = true;
        }
        if take(ctx, Key::ArrowDown, NONE) {
            let p = selectable.iter().position(|&i| i == sel).unwrap_or(0);
            sel = selectable[(p + 1) % selectable.len()];
        }
        if take(ctx, Key::ArrowUp, NONE) {
            let p = selectable.iter().position(|&i| i == sel).unwrap_or(0);
            sel = selectable[(p + selectable.len() - 1) % selectable.len()];
        }
        if take(ctx, Key::Home, NONE) {
            sel = selectable[0];
        }
        if take(ctx, Key::End, NONE) {
            sel = *selectable.last().unwrap();
        }
        if !user {
            if take(ctx, Key::ArrowRight, NONE) {
                top = (top + 1) % 5;
            }
            if take(ctx, Key::ArrowLeft, NONE) {
                top = (top + 4) % 5;
            }
        }
        if take(ctx, Key::Enter, NONE) {
            run = entries[sel].2;
        }
        drop_text(ctx);
        if top != state.top {
            self.menu = Some(MenuState { top, sel: 0, user });
            return;
        }
        let fs = self.settings.font_size;
        let font = FontId::proportional(fs);
        let width = 300.0;
        let pos = if user {
            Pos2::new(ctx.screen_rect().center().x - width / 2.0, ctx.screen_rect().center().y - 140.0)
        } else {
            let x = self.menu_x[top].min(ctx.screen_rect().right() - width - 16.0);
            Pos2::new(x, menubar_bottom + 2.0)
        };
        let row_h = fs + 9.0;
        let resp = egui::Area::new(egui::Id::new("menu_popup")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(pal.menu_bg)
                .stroke(Stroke::new(0.5_f32, pal.control_border))
                .corner_radius(9.0)
                .inner_margin(egui::Margin::same(5))
                .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: pal.shadow })
                .show(ui, |ui| {
                    ui.set_width(width);
                    if user {
                        ui.label(egui::RichText::new("  User menu").color(pal.text_dim).small());
                    }
                    for (i, (label, key, action, enabled)) in entries.iter().enumerate() {
                        if action.is_none() {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(width, 9.0), Sense::hover());
                            ui.painter().hline(r.left() + 8.0..=r.right() - 8.0, r.center().y, Stroke::new(1.0_f32, pal.separator));
                            continue;
                        }
                        let sense = if *enabled { Sense::click() } else { Sense::hover() };
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(width, row_h), sense);
                        if resp.hovered() && *enabled {
                            sel = i;
                        }
                        let selected = i == sel && *enabled;
                        if selected {
                            ui.painter().rect_filled(r, 5.0, pal.accent);
                        }
                        let fg = if !*enabled {
                            pal.text_faint
                        } else if selected {
                            pal.accent_text
                        } else {
                            pal.text
                        };
                        ui.painter().text(Pos2::new(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, label, font.clone(), fg);
                        ui.painter().text(
                            Pos2::new(r.right() - 12.0, r.center().y),
                            Align2::RIGHT_CENTER,
                            key,
                            FontId::proportional(fs - 1.0),
                            if selected { pal.accent_text } else { pal.text_faint },
                        );
                        if resp.clicked() && *enabled {
                            run = *action;
                        }
                    }
                });
        });
        let over_bar = ctx.input(|i| i.pointer.interact_pos().map(|p| p.y < menubar_bottom).unwrap_or(false));
        if ctx.input(|i| i.pointer.any_click()) && !resp.response.hovered() && run.is_none() && !over_bar {
            close = true;
        }
        if let Some(m) = self.menu.as_mut() {
            m.sel = sel;
        }
        if close {
            self.menu = None;
        }
        if let Some(a) = run {
            self.menu = None;
            self.do_action(ctx, a);
        }
    }

    pub fn draw_cmdline(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let search_focus = ctx.memory(|m| m.has_focus(egui::Id::new("toolbar_search")));
        let focus_ok = self.dialog.is_none() && self.menu.is_none() && self.job.is_none() && !search_focus && self.panels[self.active].quick_search.is_none();
        let fs = self.settings.font_size;
        egui::TopBottomPanel::bottom("cmdline")
            .exact_height(fs + 22.0)
            .frame(egui::Frame::NONE.fill(pal.window).inner_margin(egui::Margin::symmetric(12, 5)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let prompt = if self.xmap {
                        "⌃X then: C permissions · S symlink · D compare · I info · Q quick view · H hotlist".to_string()
                    } else {
                        format!("{} ›", fsutil::tilde(&self.panels[self.active].dir))
                    };
                    ui.label(egui::RichText::new(truncate_left(&prompt, 50)).color(pal.text_dim));
                    let te = egui::TextEdit::singleline(&mut self.cmdline)
                        .font(FontId::monospace(fs))
                        .hint_text("Type a shell command and press Return — it runs in this folder")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 3))
                        .id(egui::Id::new("cmdline"));
                    let r = ui.add(te);
                    if focus_ok && !r.has_focus() {
                        r.request_focus();
                    }
                });
            });
    }

    pub fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((msg, until)) = self.status.clone() else { return };
        let pal = self.pal;
        let now = ctx.input(|i| i.time);
        if now > until {
            self.status = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        egui::Area::new(egui::Id::new("toast")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -100.0)).show(ctx, |ui| {
            egui::Frame::NONE
                .fill(pal.menu_bg)
                .stroke(Stroke::new(0.5_f32, pal.control_border))
                .corner_radius(10.0)
                .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: pal.shadow })
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(msg).color(pal.text));
                });
        });
    }

    pub fn draw_panels(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(pal.window).inner_margin(egui::Margin::symmetric(10, 8))).show(ctx, |ui| {
            let full = ui.max_rect();
            let gap = 10.0;
            let rects = if self.settings.horizontal_split {
                let h = (full.height() - gap) / 2.0;
                [
                    Rect::from_min_size(full.min, Vec2::new(full.width(), h)),
                    Rect::from_min_size(Pos2::new(full.left(), full.top() + h + gap), Vec2::new(full.width(), h)),
                ]
            } else {
                let w = (full.width() - gap) / 2.0;
                [
                    Rect::from_min_size(full.min, Vec2::new(w, full.height())),
                    Rect::from_min_size(Pos2::new(full.left() + w + gap, full.top()), Vec2::new(w, full.height())),
                ]
            };
            for (i, rect) in rects.into_iter().enumerate() {
                match self.panels[i].view {
                    PanelView::Listing => self.draw_listing(ui, i, rect),
                    PanelView::Info => self.draw_info(ui, i, rect),
                    PanelView::QuickView => self.draw_quickview(ui, i, rect),
                    PanelView::Tree => self.draw_tree(ui, i, rect),
                }
            }
        });
    }

    /// A rounded white card; the active panel gets an accent focus ring.
    pub fn card(&self, ui: &egui::Ui, rect: Rect, active: bool) {
        let pal = self.pal;
        let p = ui.painter();
        p.add(egui::epaint::Shadow { offset: [0, 1], blur: 6, spread: 0, color: pal.shadow.gamma_multiply(0.35) }.as_shape(rect, 10));
        p.rect_filled(rect, 10.0, pal.card);
        p.rect_stroke(rect, 10.0, Stroke::new(0.5_f32, pal.control_border), egui::StrokeKind::Inside);
        if active {
            p.rect_stroke(rect.expand(1.5), 11.5, Stroke::new(2.5_f32, pal.accent.gamma_multiply(0.55)), egui::StrokeKind::Outside);
        }
    }

    fn mode_header(&self, ui: &egui::Ui, rect: Rect, title: &str) -> Rect {
        let pal = self.pal;
        self.card(ui, rect, false);
        let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
        ui.painter().text(Pos2::new(head.left() + 14.0, head.center().y), Align2::LEFT_CENTER, title, FontId::proportional(14.0), pal.text);
        ui.painter().hline(rect.left()..=rect.right(), head.bottom(), Stroke::new(1.0_f32, pal.separator));
        Rect::from_min_max(Pos2::new(rect.left() + 14.0, head.bottom() + 10.0), rect.max - Vec2::new(14.0, 10.0))
    }

    /// mc's Tree panel mode: the folder tree from the disk root to the other
    /// panel's folder. Clicking a folder shows it in the other panel; the
    /// triangle opens or closes it.
    pub fn draw_tree(&mut self, ui: &mut egui::Ui, idx: usize, rect: Rect) {
        let pal = self.pal;
        let inner = self.mode_header(ui, rect, "Tree");
        let fs = self.settings.font_size;
        let font = FontId::proportional(fs);
        let row_h = fs + 9.0;
        let other = 1 - idx;
        let target = self.panels[other].dir.clone();
        // Folders on the way to the other panel's folder are always open.
        let mut open: std::collections::HashSet<std::path::PathBuf> = self.panels[idx].tree_open.clone();
        for a in target.ancestors() {
            open.insert(a.to_path_buf());
        }
        let show_hidden = self.panels[other].show_hidden;
        let mut rows: Vec<(usize, std::path::PathBuf)> = Vec::new();
        fn walk(p: &std::path::Path, depth: usize, open: &std::collections::HashSet<std::path::PathBuf>, hidden: bool, rows: &mut Vec<(usize, std::path::PathBuf)>) {
            rows.push((depth, p.to_path_buf()));
            if rows.len() > 4000 || !open.contains(p) {
                return;
            }
            let Ok(rd) = std::fs::read_dir(p) else { return };
            let mut kids: Vec<std::path::PathBuf> = rd
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter(|e| hidden || !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect();
            kids.sort_by_key(|k| k.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default());
            for k in kids {
                walk(&k, depth + 1, open, hidden, rows);
            }
        }
        walk(std::path::Path::new("/"), 0, &open, show_hidden, &mut rows);
        let visible = ((inner.height() / row_h) as usize).max(1);
        let cur = rows.iter().position(|(_, p)| *p == target).unwrap_or(0);
        let scroll = &mut self.panels[idx].tree_scroll;
        if ui.rect_contains_pointer(inner) {
            let d = ui.input(|i| i.raw_scroll_delta.y);
            if d != 0.0 {
                let steps = (d / row_h).round() as isize;
                *scroll = (*scroll as isize - steps).clamp(0, rows.len().saturating_sub(visible) as isize) as usize;
            }
        }
        if cur < *scroll || cur >= *scroll + visible {
            *scroll = cur.saturating_sub(visible / 2);
        }
        let start = *scroll;
        let painter = ui.painter_at(inner);
        let mut go: Option<std::path::PathBuf> = None;
        let mut toggle: Option<std::path::PathBuf> = None;
        for (k, (depth, path)) in rows.iter().enumerate().skip(start).take(visible) {
            let y = inner.top() + (k - start) as f32 * row_h;
            let r = Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width(), row_h));
            let selected = *path == target;
            if selected {
                painter.rect_filled(r, 5.0, pal.accent);
            }
            let x = inner.left() + 6.0 + *depth as f32 * 16.0;
            let tri = Rect::from_min_size(Pos2::new(x, y), Vec2::new(14.0, row_h));
            let is_open = open.contains(path);
            painter.text(tri.center(), Align2::CENTER_CENTER, if is_open { "▾" } else { "▸" }, font.clone(), if selected { pal.accent_text } else { pal.text_dim });
            let fake = fsutil::Entry {
                name: String::new(),
                path: path.clone(),
                kind: Kind::Dir,
                link_to_dir: false,
                size: 0,
                mode: 0o755,
                nlink: 0,
                uid: 0,
                gid: 0,
                ino: 0,
                rdev: 0,
                mtime: 0,
                atime: 0,
                ctime: 0,
                marked: false,
                dir_size: None,
            };
            let icon = Rect::from_min_size(Pos2::new(x + 16.0, y + 3.0), Vec2::splat(row_h - 6.0));
            draw_file_icon(&painter, icon, &fake, &pal);
            let name = if path.parent().is_none() { "Macintosh HD".to_string() } else { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
            painter.text(Pos2::new(icon.right() + 6.0, y + row_h / 2.0), Align2::LEFT_CENTER, name, font.clone(), if selected { pal.accent_text } else { pal.text });
            let tri_resp = ui.interact(tri, ui.id().with(("tree_tri", idx, k)), Sense::click());
            let row_resp = ui.interact(r, ui.id().with(("tree_row", idx, k)), Sense::click());
            if tri_resp.clicked() {
                toggle = Some(path.clone());
            } else if row_resp.clicked() {
                go = Some(path.clone());
            }
        }
        if let Some(t) = toggle {
            let set = &mut self.panels[idx].tree_open;
            if !set.remove(&t) {
                set.insert(t.clone());
            }
            if open.contains(&t) && target.starts_with(&t) && t != target {
                // Closing a folder above the current one moves the other panel up to it.
                go = Some(t);
            }
        }
        if let Some(g) = go {
            if let Err(e) = self.panels[other].chdir(&g, true) {
                self.err(e);
            }
            self.active = other;
        }
    }

    pub fn draw_info(&mut self, ui: &mut egui::Ui, idx: usize, rect: Rect) {
        let pal = self.pal;
        let inner = self.mode_header(ui, rect, "Info");
        let painter = ui.painter_at(rect);
        let fs = self.settings.font_size;
        let font = FontId::proportional(fs);
        let row_h = fs + 9.0;
        let other = &self.panels[1 - idx];
        let mut y = inner.top();
        if let Some(e) = other.current() {
            let icon = Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::splat(48.0));
            draw_file_icon(&painter, icon, e, &pal);
            let name = if e.is_updir() { "..".to_string() } else { e.name.clone() };
            painter.text(Pos2::new(icon.right() + 12.0, y + 8.0), Align2::LEFT_TOP, truncate_middle(&name, 40), FontId::proportional(fs + 3.0), pal.text);
            let kind = kind_label(fsutil::highlight(e), e.extension(), e.name.ends_with(".app"));
            painter.text(Pos2::new(icon.right() + 12.0, y + 30.0), Align2::LEFT_TOP, format!("{} · {}", kind, finder_size(e)), font.clone(), pal.text_dim);
            y += 64.0;
            let mut lines: Vec<(&str, String)> = vec![
                ("Where", fsutil::tilde(&other.dir)),
                ("Size", format!("{} bytes ({})", fsutil::size_sep(e.shown_size()), fsutil::size_human(e.shown_size()))),
                ("Modified", fsutil::full_date(e.mtime)),
                ("Changed", fsutil::full_date(e.ctime)),
                ("Opened", fsutil::full_date(e.atime)),
                ("Permissions", format!("{}  ({:04o})", fsutil::perm_string(e), e.mode & 0o7777)),
                ("Owner", format!("{} · {}", fsutil::user_name(e.uid), fsutil::group_name(e.gid))),
                ("Links / Inode", format!("{} · {}", e.nlink, e.ino)),
            ];
            if matches!(e.kind, Kind::Symlink | Kind::StaleLink) {
                lines.push(("Points to", fsutil::read_link(&e.path).unwrap_or_default()));
            }
            if let Some((free, total)) = fsutil::free_space(&other.dir) {
                let pct = if total > 0 { free * 100 / total } else { 0 };
                lines.push(("Available", format!("{} of {} ({}%)", fsutil::size_human(free), fsutil::size_human(total), pct)));
            }
            let files = other.entries.iter().filter(|e| !e.is_dir_like()).count();
            let dirs = other.entries.iter().filter(|e| e.is_dir_like() && !e.is_updir()).count();
            lines.push(("Folder holds", format!("{} files, {} folders", files, dirs)));
            for (k, v) in lines {
                if y + row_h > inner.bottom() {
                    break;
                }
                painter.text(Pos2::new(inner.left() + 100.0, y), Align2::RIGHT_TOP, k, font.clone(), pal.text_dim);
                painter.text(Pos2::new(inner.left() + 112.0, y), Align2::LEFT_TOP, truncate_middle(&v, 60), font.clone(), pal.text);
                y += row_h;
            }
        }
        if ui.interact(rect, ui.id().with(("info", idx)), Sense::click()).double_clicked() {
            self.panels[idx].view = PanelView::Listing;
        }
    }

    pub fn draw_quickview(&mut self, ui: &mut egui::Ui, idx: usize, rect: Rect) {
        let pal = self.pal;
        let other = &self.panels[1 - idx];
        let name = other.current().map(|e| e.name.clone()).unwrap_or_default();
        let inner = self.mode_header(ui, rect, &format!("Quick View — {}", truncate_middle(&name, 40)));
        let mono = self.mono();
        let entry = self.panels[1 - idx].current().cloned();
        let show_hidden = self.panels[1 - idx].show_hidden;
        self.preview.show(ui, inner, entry.as_ref(), show_hidden, &self.settings, pal.dark, pal.text, pal.text_dim, mono);
        if ui.interact(rect, ui.id().with(("qv", idx)), Sense::click()).double_clicked() {
            self.panels[idx].view = PanelView::Listing;
        }
    }
}
