//! The LCARS look (SPEC §15, LCR): header with elbow, bar and menu row, the function-key column,
//! and the footer with command line and status bar. The shapes follow the LCARS design system:
//! 5 px gutter, elbow radii 60 / 30, 24 px bars, pill ends, labels bottom-right in Antonio.

use crate::app::{Action, App, MenuState};
use crate::menus::TITLES;
use crate::settings::McpLevel;
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Margin, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

pub const GUTTER: f32 = 5.0;
pub const COLUMN_W: f32 = 152.0;
pub const ELBOW_W: f32 = 212.0;
pub const ELBOW_H: f32 = 72.0;
pub const BAR_H: f32 = 24.0;
pub const NAV_H: f32 = 43.0;
pub const EDGE: f32 = 8.0;

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("lcars-bold".into()))
}

fn corners(nw: u8, ne: u8, sw: u8, se: u8) -> CornerRadius {
    CornerRadius { nw, ne, sw, se }
}

/// Colour of an interactive segment while hovered / pressed.
fn state_color(base: Color32, resp: &Response) -> Color32 {
    if resp.is_pointer_button_down_on() {
        base.gamma_multiply(0.85)
    } else if resp.hovered() {
        base.lerp_to_gamma(Color32::WHITE, 0.15)
    } else {
        base
    }
}

/// An elbow: `bottom` mirrors it vertically. The inner fillet is cut out with the ground colour.
pub fn elbow(p: &Painter, origin: Pos2, bottom: bool, tone: Color32, ground: Color32) {
    let outer = Rect::from_min_size(origin, Vec2::new(ELBOW_W, ELBOW_H));
    if bottom {
        p.rect_filled(outer, corners(0, 0, 60, 0), tone);
        p.rect_filled(Rect::from_min_size(origin + Vec2::new(COLUMN_W, 0.0), Vec2::new(ELBOW_W - COLUMN_W, ELBOW_H - BAR_H)), corners(0, 0, 30, 0), ground);
    } else {
        p.rect_filled(outer, corners(60, 0, 0, 0), tone);
        p.rect_filled(Rect::from_min_size(origin + Vec2::new(COLUMN_W, BAR_H), Vec2::new(ELBOW_W - COLUMN_W, ELBOW_H - BAR_H)), corners(30, 0, 0, 0), ground);
    }
}

/// A filled segment with a label (14 px caption). `start` puts the text on the left.
fn segment(p: &Painter, r: Rect, fill: Color32, radius: CornerRadius, label: &str, ink: Color32, start: bool) {
    p.rect_filled(r, radius, fill);
    if label.is_empty() {
        return;
    }
    let (pos, anchor) = if start { (Pos2::new(r.left() + 12.0, r.center().y + 2.5), Align2::LEFT_CENTER) } else { (Pos2::new(r.right() - 10.0, r.center().y + 2.5), Align2::RIGHT_CENTER) };
    p.text(pos, anchor, label.to_uppercase(), font(15.0), ink);
}

/// A button segment: label bottom-right, brighter on hover, `accent` when active.
fn button(ui: &mut Ui, id: egui::Id, r: Rect, label: &str, fill: Color32, radius: CornerRadius, active: bool, size: f32, pal: &crate::theme::Pal) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    let base = if active { pal.accent } else { fill };
    ui.painter().rect_filled(r, radius, state_color(base, &resp));
    ui.painter().text(Pos2::new(r.right() - 12.0, r.bottom() - 5.0), Align2::RIGHT_BOTTOM, label.to_uppercase(), bold(size), pal.on_fill);
    resp
}

fn text_w(ui: &Ui, s: &str, f: FontId) -> f32 {
    ui.fonts(|fo| fo.layout_no_wrap(s.to_string(), f, Color32::WHITE).size().x)
}

/// What the header asked for.
#[derive(Default)]
struct Head {
    menu: Option<usize>,
    settings: bool,
    classic: bool,
    hidden: bool,
    find: bool,
    minimize: bool,
    maximize: bool,
    close: bool,
}

impl App {
    /// Header, footer and side column of the LCARS look. Returns the function key that was clicked
    /// (1–10) and the bottom edge of the header (where the pull-down menus start).
    pub fn draw_lcars_chrome(&mut self, ctx: &egui::Context) -> (Option<u8>, f32) {
        let head = self.lcars_head(ctx);
        self.lcars_foot(ctx);
        let hit = self.lcars_side(ctx);
        if let Some(i) = head.menu {
            let same = self.menu.as_ref().map(|m| !m.user && m.top == i).unwrap_or(false);
            self.menu = if same { None } else { Some(MenuState { top: i, sel: 0, user: false }) };
        }
        let idle = self.dialog.is_none() && self.job.is_none();
        if head.settings {
            self.menu = None;
            self.do_action(ctx, Action::Config);
        }
        if head.classic {
            self.menu = None;
            self.do_action(ctx, Action::ToggleLook);
        }
        if head.hidden && idle {
            self.do_action(ctx, Action::ToggleHidden);
        }
        if head.find && idle {
            self.do_action(ctx, Action::Find);
        }
        if head.minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if head.maximize {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        if head.close {
            self.do_action(ctx, Action::Quit);
        }
        (hit, ELBOW_H)
    }

    fn lcars_head(&mut self, ctx: &egui::Context) -> Head {
        let pal = self.pal;
        let t = pal.tones;
        let mut out = Head::default();
        egui::TopBottomPanel::top("lc_head").exact_height(ELBOW_H).frame(egui::Frame::NONE.fill(pal.window)).show(ctx, |ui| {
            let r = ui.max_rect();
            // Dragging the empty header moves the window.
            let bg = ui.interact(r, ui.id().with("lc_head_drag"), Sense::click_and_drag());
            if bg.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if bg.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            }
            let p = ui.painter().clone();
            elbow(&p, r.min, false, t[0], pal.window);
            p.text(Pos2::new(r.left() + COLUMN_W - 10.0, r.top() + ELBOW_H - 6.0), Align2::RIGHT_BOTTOM, "MYCOMMANDER", font(15.0), pal.on_fill);

            // Bar of segments.
            let x0 = r.left() + ELBOW_W;
            let x1 = r.right() - EDGE;
            let bar = Rect::from_min_max(Pos2::new(x0, r.top()), Pos2::new(x1, r.top() + BAR_H));
            // Window controls (LCR-9): minimise, maximise, close – square segments, 54 px, 5 px apart.
            let wc_w = 54.0;
            let controls_w = 3.0 * wc_w + 2.0 * GUTTER;
            let fixed = [(96.0, t[2], "Local"), (40.0, t[3], "")];
            let used: f32 = fixed.iter().map(|f| f.0 + GUTTER).sum::<f32>() + controls_w + GUTTER;
            let remote = crate::vfs::is_remote(&self.panels[self.active].dir);
            let main_w = (bar.width() - used).max(40.0);
            segment(&p, Rect::from_min_size(bar.min, Vec2::new(main_w, BAR_H)), t[0], CornerRadius::ZERO, &format!("File manager {}", env!("CARGO_PKG_VERSION")), pal.on_fill, true);
            let mut x = bar.left() + main_w + GUTTER;
            for (i, (w, tone, label)) in fixed.iter().enumerate() {
                let lab = if i == 0 && remote { "Server" } else { label };
                segment(&p, Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(*w, BAR_H)), *tone, CornerRadius::ZERO, lab, pal.on_fill, false);
                x += w + GUTTER;
            }
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            let controls = [(pal.win_min, "Minimize"), (pal.win_max, if maximized { "Restore" } else { "Maximize" }), (pal.win_close, "Close")];
            for (k, (color, tip)) in controls.iter().enumerate() {
                let rect = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(wc_w, BAR_H));
                let rad = if k == 2 { corners(0, 12, 0, 12) } else { CornerRadius::ZERO };
                let resp = ui.interact(rect, ui.id().with(("lc_wc", k)), Sense::click());
                p.rect_filled(rect, rad, state_color(*color, &resp));
                let c = rect.center();
                let st = Stroke::new(2.0_f32, pal.on_fill);
                match k {
                    0 => {
                        p.line_segment([c + Vec2::new(-7.0, 4.0), c + Vec2::new(7.0, 4.0)], st);
                    }
                    1 if maximized => {
                        p.rect_stroke(Rect::from_center_size(c + Vec2::new(2.0, -2.0), Vec2::splat(8.0)), 0.0, st, egui::StrokeKind::Middle);
                        p.rect_stroke(Rect::from_center_size(c + Vec2::new(-2.0, 2.0), Vec2::splat(8.0)), 0.0, st, egui::StrokeKind::Middle);
                    }
                    1 => {
                        p.rect_stroke(Rect::from_center_size(c, Vec2::new(12.0, 10.0)), 0.0, st, egui::StrokeKind::Middle);
                    }
                    _ => {
                        p.line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(5.0, 5.0)], st);
                        p.line_segment([c + Vec2::new(-5.0, 5.0), c + Vec2::new(5.0, -5.0)], st);
                    }
                }
                if resp.on_hover_text(*tip).clicked() {
                    match k {
                        0 => out.minimize = true,
                        1 => out.maximize = true,
                        _ => out.close = true,
                    }
                }
                x += wc_w + GUTTER;
            }

            // Menu row.
            let ny = r.top() + ELBOW_H - NAV_H;
            let mut x = x0;
            let menu_tones = [t[2], t[1], t[3], t[2]];
            for (i, title) in TITLES.iter().enumerate() {
                let w = text_w(ui, &title.to_uppercase(), bold(20.0)) + 28.0;
                let rect = Rect::from_min_size(Pos2::new(x, ny), Vec2::new(w, NAV_H));
                let open = self.menu.as_ref().map(|m| !m.user && m.top == i).unwrap_or(false);
                let resp = button(ui, ui.id().with(("lc_menu", i)), rect, title, menu_tones[i], CornerRadius::ZERO, open, 20.0, &pal);
                self.menu_x[i] = rect.left();
                if resp.clicked() {
                    out.menu = Some(i);
                }
                if resp.hovered() {
                    if let Some(m) = self.menu.as_mut() {
                        if !m.user && m.top != i {
                            m.top = i;
                            m.sel = 0;
                        }
                    }
                }
                x += w + GUTTER;
            }
            // Right group, laid out from the right edge: Classic, Settings, Find, Hidden.
            let hidden = self.panels[0].show_hidden;
            let items: [(&str, Color32, CornerRadius, bool, f32); 4] = [
                ("Hidden", t[1], CornerRadius::ZERO, hidden, 92.0),
                ("Find", t[2], CornerRadius::ZERO, false, 80.0),
                ("Settings", t[3], corners(24, 0, 24, 0), false, 116.0),
                ("Classic", t[5], corners(0, 24, 0, 24), false, 116.0),
            ];
            let total: f32 = items.iter().map(|i| i.4 + GUTTER).sum::<f32>() - GUTTER;
            let mut rx = x1 - total;
            let title_room = rx - x - GUTTER;
            if title_room > 150.0 {
                p.text(Pos2::new(rx - 12.0, ny + NAV_H - 4.0), Align2::RIGHT_BOTTOM, "MYCOMMANDER", bold(38.0), pal.text);
            }
            for (i, (label, fill, rad, active, w)) in items.iter().enumerate() {
                let rect = Rect::from_min_size(Pos2::new(rx, ny), Vec2::new(*w, NAV_H));
                let resp = button(ui, ui.id().with(("lc_right", i)), rect, label, *fill, *rad, *active, 20.0, &pal);
                let resp = resp.on_hover_text(match i {
                    0 => "Show or hide hidden files (⇧⌘.)",
                    1 => "Find files (⇧⌘F)",
                    2 => "Settings (⌘,)",
                    _ => "Back to the classic look",
                });
                if resp.clicked() {
                    match i {
                        0 => out.hidden = true,
                        1 => out.find = true,
                        2 => out.settings = true,
                        _ => out.classic = true,
                    }
                }
                rx += w + GUTTER;
            }
        });
        out
    }

    fn lcars_foot(&mut self, ctx: &egui::Context) {
        let pal = self.pal;
        let t = pal.tones;
        let cmd_focus_ok = self.dialog.is_none() && self.menu.is_none() && self.job.is_none() && self.panels[self.active].quick_search.is_none();
        egui::TopBottomPanel::bottom("lc_foot").exact_height(ELBOW_H).frame(egui::Frame::NONE.fill(pal.window)).show(ctx, |ui| {
            let r = ui.max_rect();
            let p = ui.painter().clone();
            elbow(&p, r.min, true, t[0], pal.window);
            let status = self.status.as_ref().map(|(s, _)| s.clone());
            let corner = if self.job.is_some() { "Working" } else if self.xmap { "Ctrl-X" } else { "Ready" };
            p.text(Pos2::new(r.left() + COLUMN_W - 10.0, r.top() + 6.0), Align2::RIGHT_TOP, corner.to_uppercase(), font(15.0), pal.on_fill);

            let x0 = r.left() + ELBOW_W;
            let x1 = r.right() - EDGE;
            // Command line.
            let cy = r.top();
            let label_w = 120.0;
            segment_button(&p, Rect::from_min_size(Pos2::new(x0, cy), Vec2::new(label_w, NAV_H)), t[3], "Command", &pal);
            let field = Rect::from_min_max(Pos2::new(x0 + label_w + GUTTER, cy), Pos2::new(x1, cy + NAV_H));
            p.rect(field, corners(0, 22, 0, 22), pal.field, Stroke::new(2.0_f32, pal.control_border), egui::StrokeKind::Inside);
            let prompt = format!("{} ›", crate::util::truncate_left(&crate::fsutil::tilde(&self.panels[self.active].dir), 34));
            p.text(Pos2::new(field.left() + 16.0, field.center().y), Align2::LEFT_CENTER, &prompt, font(19.0), t[3]);
            let pw = text_w(ui, &prompt, font(19.0));
            let inner = Rect::from_min_max(Pos2::new(field.left() + 24.0 + pw, field.top() + 3.0), Pos2::new(field.right() - 18.0, field.bottom() - 3.0));
            let te = egui::TextEdit::singleline(&mut self.cmdline)
                .font(font(20.0))
                .frame(false)
                .hint_text(egui::RichText::new("Type a shell command and press Return").color(pal.text_dim))
                .desired_width(inner.width())
                .vertical_align(egui::Align::Center)
                .id(egui::Id::new("cmdline"));
            let resp = ui.put(inner, te);
            if cmd_focus_ok && !resp.has_focus() {
                resp.request_focus();
            }

            // Status bar.
            let by = r.bottom() - BAR_H;
            let bar = Rect::from_min_max(Pos2::new(x0, by), Pos2::new(x1, r.bottom()));
            use crate::update::State as U;
            let (upd_label, upd_click): (String, Option<U>) = match crate::update::state() {
                U::Available { version, url } => (format!("Update {version}"), Some(U::Available { version, url })),
                U::Installing(m) => (m, None),
                U::Restart => ("Restarting".into(), None),
                U::Failed(_) => ("Update failed".into(), Some(U::Idle)),
                _ => (format!("v{}", env!("CARGO_PKG_VERSION")), None),
            };
            let level = match crate::control::level() {
                McpLevel::Off => "Agent off",
                McpLevel::Read => "Agent read",
                McpLevel::Control => "Agent control",
                McpLevel::Full => "Agent full",
            };
            let user = std::env::var("USER").unwrap_or_default();
            let fixed = [(150.0, t[2]), (120.0, t[1]), (74.0, t[4])];
            let used: f32 = fixed.iter().map(|f| f.0 + GUTTER).sum();
            let main_w = (bar.width() - used).max(40.0);
            segment(&p, Rect::from_min_size(bar.min, Vec2::new(main_w, BAR_H)), t[0], CornerRadius::ZERO, status.as_deref().unwrap_or(&crate::fsutil::tilde(&self.panels[self.active].dir)), pal.on_fill, true);
            let mut x = bar.left() + main_w + GUTTER;
            let upd_rect = Rect::from_min_size(Pos2::new(x, by), Vec2::new(fixed[0].0, BAR_H));
            let resp = ui.interact(upd_rect, ui.id().with("lc_update"), Sense::click());
            let upd_fill = if upd_click.is_some() { state_color(pal.accent, &resp) } else { fixed[0].1 };
            segment(&p, upd_rect, upd_fill, CornerRadius::ZERO, &upd_label, pal.on_fill, false);
            if let Some(st) = upd_click {
                if resp.on_hover_text("Click to install").clicked() {
                    match st {
                        U::Available { version, url } => crate::update::install_async(version, url),
                        _ => crate::update::check_async(),
                    }
                }
            }
            x += fixed[0].0 + GUTTER;
            segment(&p, Rect::from_min_size(Pos2::new(x, by), Vec2::new(fixed[1].0, BAR_H)), fixed[1].1, CornerRadius::ZERO, level, pal.on_fill, false);
            x += fixed[1].0 + GUTTER;
            segment(&p, Rect::from_min_size(Pos2::new(x, by), Vec2::new(fixed[2].0, BAR_H)), fixed[2].1, corners(0, 12, 0, 12), &user, pal.on_fill, false);
        });
    }

    /// The ten function keys as the menu column. Returns the key (1–10) that was clicked.
    fn lcars_side(&mut self, ctx: &egui::Context) -> Option<u8> {
        let pal = self.pal;
        let t = pal.tones;
        let mut hit = None;
        let names = ["Help", "Menu", "View", "Edit", "Copy", "RenMov", "Mkdir", "Delete", "PullDn", "Quit"];
        let tones = [t[2], t[1], t[3], t[2], t[1], t[3], t[2], t[1], t[3], t[4]];
        egui::SidePanel::left("lc_side")
            .exact_width(COLUMN_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(pal.window).inner_margin(Margin { left: 0, right: 0, top: GUTTER as i8, bottom: GUTTER as i8 }))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                let bh = 40.0;
                let mut y = r.top();
                for (i, name) in names.iter().enumerate() {
                    let rect = Rect::from_min_size(Pos2::new(r.left(), y), Vec2::new(COLUMN_W, bh));
                    let resp = ui.interact(rect, ui.id().with(("lc_fkey", i)), Sense::click());
                    ui.painter().rect_filled(rect, CornerRadius::ZERO, state_color(tones[i], &resp));
                    ui.painter().text(Pos2::new(rect.left() + 12.0, rect.center().y + 1.0), Align2::LEFT_CENTER, format!("F{}", i + 1), font(15.0), pal.on_fill.gamma_multiply(0.75));
                    ui.painter().text(Pos2::new(rect.right() - 12.0, rect.bottom() - 6.0), Align2::RIGHT_BOTTOM, name.to_uppercase(), bold(19.0), pal.on_fill);
                    if resp.clicked() {
                        hit = Some(i as u8 + 1);
                    }
                    y += bh + GUTTER;
                }
                if y < r.bottom() - 6.0 {
                    ui.painter().rect_filled(Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), r.bottom())), CornerRadius::ZERO, t[4]);
                }
            });
        hit
    }

    /// Title bar of a dialog in the LCARS look: caps, black gaps, black title block (LCR-7).
    pub fn lcars_dialog_title(&self, ui: &mut Ui, title: &str, error: bool) {
        let pal = self.pal;
        let tone = if error { pal.danger } else { pal.tones[0] };
        let cap = tone.gamma_multiply(0.86);
        let w = ui.available_width().max(360.0);
        let h = 34.0;
        let (r, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
        let p = ui.painter();
        let g = 10.0;
        p.rect_filled(Rect::from_min_size(r.min, Vec2::new(h, h)), corners(17, 0, 17, 0), cap);
        p.rect_filled(Rect::from_min_size(Pos2::new(r.right() - h, r.top()), Vec2::new(h, h)), corners(0, 17, 0, 17), cap);
        p.rect_filled(Rect::from_min_max(Pos2::new(r.left() + h + g, r.top()), Pos2::new(r.right() - h - g, r.bottom())), CornerRadius::ZERO, tone);
        let text = title.to_uppercase();
        let tw = text_w(ui, &text, bold(25.0));
        let block = Rect::from_min_size(Pos2::new(r.left() + h + g, r.top()), Vec2::new(tw + 24.0, h));
        p.rect_filled(block, CornerRadius::ZERO, pal.window);
        p.text(Pos2::new(block.left() + 12.0, block.center().y + 1.0), Align2::LEFT_CENTER, text, bold(25.0), pal.text);
        ui.add_space(8.0);
    }
}

/// Show or hide the macOS window buttons (red, yellow, green). The LCARS look draws its own (LCR-9).
#[cfg(target_os = "macos")]
pub fn set_traffic_lights_hidden(frame: &eframe::Frame, hidden: bool) {
    use std::ffi::{c_char, c_void};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[link(name = "objc")]
    extern "C" {
        fn objc_msgSend();
        fn sel_registerName(name: *const c_char) -> *mut c_void;
    }
    let Ok(handle) = frame.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: plain Objective-C messages to the window's own view / window / buttons, on the main thread.
    unsafe {
        let send = objc_msgSend as *const ();
        let get: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(send);
        let button_at: extern "C" fn(*mut c_void, *mut c_void, isize) -> *mut c_void = std::mem::transmute(send);
        let set_hidden: extern "C" fn(*mut c_void, *mut c_void, bool) = std::mem::transmute(send);
        let window = get(h.ns_view.as_ptr(), sel_registerName(c"window".as_ptr()));
        if window.is_null() {
            return;
        }
        for kind in 0..3isize {
            let b = button_at(window, sel_registerName(c"standardWindowButton:".as_ptr()), kind);
            if !b.is_null() {
                set_hidden(b, sel_registerName(c"setHidden:".as_ptr()), hidden);
                if std::env::var_os("MYCMD_DEBUG_LIGHTS").is_some() {
                    // Test aid: read the state back and report it.
                    let is_hidden: extern "C" fn(*mut c_void, *mut c_void) -> bool = std::mem::transmute(send);
                    eprintln!("traffic light {kind}: hidden = {}", is_hidden(b, sel_registerName(c"isHidden".as_ptr())));
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_traffic_lights_hidden(_frame: &eframe::Frame, _hidden: bool) {}

/// A non-interactive label segment (the "Command" label next to the field).
fn segment_button(p: &Painter, r: Rect, fill: Color32, label: &str, pal: &crate::theme::Pal) {
    p.rect_filled(r, CornerRadius::ZERO, fill);
    p.text(Pos2::new(r.right() - 12.0, r.bottom() - 5.0), Align2::RIGHT_BOTTOM, label.to_uppercase(), bold(20.0), pal.on_fill);
}

