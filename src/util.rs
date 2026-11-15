//! Small helpers shared by the window code: key handling, text shortening,
//! the type character, fonts and the widget style.

use crate::fsutil::{self, Entry, Kind};
use crate::theme::*;
use egui::{Color32, FontId, Key, Stroke, Vec2};
#[allow(unused_imports)]
use crate::fsutil::Highlight;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct M {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}
pub const NONE: M = M { ctrl: false, alt: false, shift: false, cmd: false };
pub const CTRL: M = M { ctrl: true, alt: false, shift: false, cmd: false };
pub const ALT: M = M { ctrl: false, alt: true, shift: false, cmd: false };
pub const SHIFT: M = M { ctrl: false, alt: false, shift: true, cmd: false };
pub const CMD: M = M { ctrl: false, alt: false, shift: false, cmd: true };
pub const CMD_SHIFT: M = M { ctrl: false, alt: false, shift: true, cmd: true };
pub const ALT_SHIFT: M = M { ctrl: false, alt: true, shift: true, cmd: false };

fn mods_of(m: &egui::Modifiers) -> M {
    M { ctrl: m.ctrl, alt: m.alt, shift: m.shift, cmd: m.mac_cmd }
}

/// Take a key press with exactly these modifiers out of the input queue.
/// With Alt or Ctrl, the character macOS types for that combination is removed too.
pub fn take(ctx: &egui::Context, key: Key, m: M) -> bool {
    ctx.input_mut(|i| {
        let pos = i.events.iter().position(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && mods_of(modifiers) == m)
        });
        if let Some(p) = pos {
            i.events.remove(p);
            if m.alt || m.ctrl {
                if let Some(t) = i.events.iter().position(|e| matches!(e, egui::Event::Text(_))) {
                    i.events.remove(t);
                }
            }
            true
        } else {
            false
        }
    })
}

pub fn take_text(ctx: &egui::Context, s: &str) -> bool {
    ctx.input_mut(|i| {
        if let Some(p) = i.events.iter().position(|e| matches!(e, egui::Event::Text(t) if t == s)) {
            i.events.remove(p);
            true
        } else {
            false
        }
    })
}

pub fn pressed(ctx: &egui::Context, key: Key) -> bool {
    ctx.input(|i| i.key_pressed(key))
}

pub fn drop_keys(ctx: &egui::Context, keys: &[Key]) {
    ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key, .. } if keys.contains(key))));
}

pub fn drop_text(ctx: &egui::Context) {
    ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_))));
}

/// mc's "type" column character before the name.
#[allow(dead_code)]
pub fn type_char(e: &Entry) -> char {
    match e.kind {
        Kind::UpDir | Kind::Dir => '/',
        Kind::Symlink => {
            if e.link_to_dir {
                '~'
            } else {
                '@'
            }
        }
        Kind::StaleLink => '!',
        Kind::Device => '+',
        Kind::Special => {
            if e.mode & 0o170000 == 0o140000 {
                '='
            } else {
                '|'
            }
        }
        Kind::File => {
            if e.mode & 0o111 != 0 {
                '*'
            } else {
                ' '
            }
        }
    }
}

/// Size column text (mc `string_file_size`).
pub fn size_cell(e: &Entry, width: usize) -> String {
    if e.is_updir() {
        return "UP--DIR".into();
    }
    if e.kind == Kind::Device {
        let (ma, mi) = ((e.rdev >> 24) & 0xff, e.rdev & 0xffffff);
        return format!("{},{}", ma, mi);
    }
    if e.is_dir_like() && e.dir_size.is_none() {
        return if e.kind == Kind::Symlink { "SYMLINK".into() } else { "<DIR>".into() };
    }
    fsutil::size_fit(e.shown_size(), width)
}

/// Mini status size (mc `string_file_size_brief`).
pub fn mini_size(e: &Entry) -> String {
    if e.is_dir_like() && e.dir_size.is_none() {
        if e.kind == Kind::Symlink {
            "SYMLINK".into()
        } else {
            "SUB-DIR".into()
        }
    } else {
        fsutil::size_fit(e.shown_size(), 7)
    }
}

pub fn hex_line(off: usize, bytes: &[u8]) -> String {
    let mut s = format!("{:08X}  ", off);
    for i in 0..16 {
        if i < bytes.len() {
            s.push_str(&format!("{:02X} ", bytes[i]));
        } else {
            s.push_str("   ");
        }
        if i == 7 {
            s.push(' ');
        }
    }
    s.push(' ');
    for &b in bytes {
        s.push(if (0x20..0x7f).contains(&b) { b as char } else { '.' });
    }
    s
}

/// Shorten in the middle with `~`, as mc does for long names.
pub fn truncate_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max <= 2 {
        return s.chars().take(max).collect();
    }
    let left = (max - 1) / 2;
    let right = max - 1 - left;
    let a: String = s.chars().take(left).collect();
    let b: String = s.chars().skip(n - right).collect();
    format!("{}~{}", a, b)
}

pub fn truncate_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let b: String = s.chars().skip(n - max.saturating_sub(3)).collect();
    format!("...{}", b)
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/+,:@%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && ((t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"'))) {
        t[1..t.len() - 1].to_string()
    } else {
        t.replace("\\ ", " ")
    }
}

/// A macOS default button: filled with the accent colour, white text.
pub fn primary_button(ui: &mut egui::Ui, pal: &Pal, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(pal.accent_text))
            .fill(pal.accent)
            .stroke(Stroke::NONE)
            .corner_radius(6.0_f32)
            .min_size(Vec2::new(78.0, 24.0)),
    )
}

pub fn plain_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).corner_radius(6.0_f32).min_size(Vec2::new(78.0, 24.0)))
}

/// Mac button order: Cancel on the left, the default (OK) on the right.
pub fn ok_cancel(ui: &mut egui::Ui) -> (bool, bool) {
    ok_cancel_labeled(ui, "OK")
}

pub fn ok_cancel_labeled(ui: &mut egui::Ui, ok_label: &str) -> (bool, bool) {
    let pal = ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("pal_dark"))).map(|d| if d { DARK } else { LIGHT }).unwrap_or(LIGHT);
    ui.add_space(4.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let ok = primary_button(ui, &pal, ok_label).clicked();
        let cancel = plain_button(ui, "Cancel").clicked();
        (ok, cancel)
    })
    .inner
}

pub fn apply_style(style: &mut egui::Style, fs: f32, pal: &Pal) {
    use egui::{FontFamily, TextStyle};
    style.text_styles = [
        (TextStyle::Small, FontId::new(fs - 2.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(fs, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(fs, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(fs + 3.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(fs, FontFamily::Monospace)),
    ]
    .into();
    let mut v = if pal.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = pal.window;
    v.window_fill = pal.menu_bg;
    v.window_stroke = Stroke::new(0.5_f32, pal.control_border);
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: pal.shadow };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: pal.shadow };
    v.menu_corner_radius = egui::CornerRadius::same(8);
    v.override_text_color = Some(pal.text);
    v.extreme_bg_color = pal.field;
    v.faint_bg_color = pal.row_alt;
    v.selection.bg_fill = pal.accent.gamma_multiply(if pal.dark { 0.8 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0_f32, pal.accent);
    v.hyperlink_color = pal.accent;
    v.text_cursor.stroke = Stroke::new(1.5_f32, pal.accent);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open, &mut v.widgets.noninteractive] {
        w.corner_radius = egui::CornerRadius::same(6);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal.separator);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.inactive.weak_bg_fill = pal.control;
    v.widgets.inactive.bg_fill = pal.control;
    v.widgets.inactive.bg_stroke = Stroke::new(0.5_f32, pal.control_border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.hovered.weak_bg_fill = pal.control_hover;
    v.widgets.hovered.bg_fill = pal.control_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(0.5_f32, pal.control_border);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.weak_bg_fill = pal.sel_inactive;
    v.widgets.active.bg_fill = pal.accent;
    v.widgets.active.bg_stroke = Stroke::new(0.5_f32, pal.control_border);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, pal.text);
    v.widgets.active.expansion = 0.0;
    style.visuals = v;
    style.spacing.button_padding = Vec2::new(12.0, 4.0);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.interact_size.y = 24.0;
    style.spacing.window_margin = egui::Margin::same(18);
}

/// Use the Mac's own fonts: San Francisco for the interface, SF Mono / Menlo for code.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    let try_load = |path: &str, index: u32| -> Option<FontData> {
        std::fs::read(path).ok().map(|b| {
            let mut fd = FontData::from_owned(b);
            fd.index = index;
            fd
        })
    };
    for (p, i) in [("/System/Library/Fonts/SFNSMono.ttf", 0), ("/System/Library/Fonts/Menlo.ttc", 0)] {
        if let Some(fd) = try_load(p, i) {
            fonts.font_data.insert("mono".into(), std::sync::Arc::new(fd));
            fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "mono".into());
            break;
        }
    }
    for p in ["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/HelveticaNeue.ttc", "/System/Library/Fonts/Helvetica.ttc"] {
        if let Some(fd) = try_load(p, 0) {
            fonts.font_data.insert("system".into(), std::sync::Arc::new(fd));
            fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "system".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

/// Finder-style date: "Today, 14:05", "Yesterday, 09:12", else "14 Nov 2026, 23:53".
pub fn finder_date(t: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(t, 0).single() else { return String::new() };
    let today = Local::now().date_naive();
    let d = dt.date_naive();
    if d == today {
        dt.format("Today, %H:%M").to_string()
    } else if today.pred_opt() == Some(d) {
        dt.format("Yesterday, %H:%M").to_string()
    } else {
        dt.format("%-d %b %Y, %H:%M").to_string()
    }
}

/// Finder-style size: "--" for folders, otherwise "34 KB" / "1.2 MB".
pub fn finder_size(e: &Entry) -> String {
    if e.is_updir() {
        return String::new();
    }
    if e.is_dir_like() && e.dir_size.is_none() {
        return "--".into();
    }
    let s = e.shown_size();
    if s < 1000 {
        return format!("{} bytes", s);
    }
    let units = ["KB", "MB", "GB", "TB", "PB"];
    let mut v = s as f64 / 1000.0;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < units.len() {
        v /= 1000.0;
        i += 1;
    }
    if v < 10.0 {
        format!("{:.1} {}", v, units[i])
    } else {
        format!("{:.0} {}", v, units[i])
    }
}

/// A small file icon in the Finder spirit: blue folders, app tiles, and paper
/// sheets with a coloured band for the file's type group.
pub fn draw_file_icon(p: &egui::Painter, rect: egui::Rect, e: &Entry, pal: &Pal) {
    use egui::{pos2, Rect};
    let h = fsutil::highlight(e);
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), Vec2::splat(s));
    if e.name.ends_with(".app") && e.is_dir_like() {
        let tile = r.shrink(s * 0.06);
        p.rect_filled(tile, s * 0.22, Color32::from_rgb(120, 130, 150));
        p.rect_filled(tile.shrink(s * 0.16), s * 0.12, Color32::from_rgb(230, 232, 238));
        p.circle_filled(tile.center(), s * 0.14, pal.accent);
    } else if e.is_dir_like() {
        let body = Rect::from_min_max(pos2(r.left(), r.top() + s * 0.24), pos2(r.right(), r.bottom() - s * 0.1));
        let tab = Rect::from_min_max(pos2(r.left(), r.top() + s * 0.14), pos2(r.left() + s * 0.45, r.top() + s * 0.34));
        p.rect_filled(tab, s * 0.08, pal.folder_dark);
        p.rect_filled(body, s * 0.1, pal.folder);
        p.hline(body.left() + 1.0..=body.right() - 1.0, body.top() + s * 0.12, Stroke::new(1.0_f32, pal.folder_dark.gamma_multiply(0.6)));
        if e.is_updir() {
            let c = body.center() + Vec2::new(0.0, s * 0.04);
            let a = s * 0.18;
            p.line_segment([c + Vec2::new(0.0, a), c - Vec2::new(0.0, a)], Stroke::new(1.6_f32, Color32::WHITE));
            p.line_segment([c - Vec2::new(0.0, a), c + Vec2::new(-a * 0.8, -a * 0.2)], Stroke::new(1.6_f32, Color32::WHITE));
            p.line_segment([c - Vec2::new(0.0, a), c + Vec2::new(a * 0.8, -a * 0.2)], Stroke::new(1.6_f32, Color32::WHITE));
        }
    } else {
        let w = s * 0.74;
        let sheet = Rect::from_center_size(r.center(), Vec2::new(w, s * 0.96));
        let paper = if pal.dark { Color32::from_rgb(210, 210, 214) } else { Color32::WHITE };
        p.rect_filled(sheet, s * 0.08, paper);
        p.rect_stroke(sheet, s * 0.08, Stroke::new(1.0_f32, Color32::from_rgb(170, 170, 176)), egui::StrokeKind::Inside);
        // folded corner
        let c = sheet.right_top();
        let f = s * 0.24;
        p.add(egui::Shape::convex_polygon(
            vec![pos2(c.x - f, c.y), pos2(c.x, c.y + f), pos2(c.x - f, c.y + f)],
            Color32::from_rgb(200, 200, 206),
            Stroke::NONE,
        ));
        if h == fsutil::Highlight::Executable {
            let band = Rect::from_min_max(pos2(sheet.left() + 2.0, sheet.top() + s * 0.3), pos2(sheet.right() - 2.0, sheet.bottom() - 2.0));
            p.rect_filled(band, s * 0.05, Color32::from_rgb(40, 40, 44));
            p.text(band.center(), egui::Align2::CENTER_CENTER, ">_", FontId::monospace(s * 0.32), type_color(h));
        } else if h != fsutil::Highlight::Normal {
            let band = Rect::from_min_max(pos2(sheet.left() + 2.0, sheet.bottom() - s * 0.34), pos2(sheet.right() - 2.0, sheet.bottom() - s * 0.1));
            p.rect_filled(band, 2.0, type_color(h));
        } else {
            for k in 0..3 {
                let y = sheet.top() + s * (0.42 + 0.14 * k as f32);
                p.hline(sheet.left() + s * 0.12..=sheet.right() - s * 0.12, y, Stroke::new(1.0_f32, Color32::from_rgb(190, 190, 196)));
            }
        }
    }
    if matches!(e.kind, Kind::Symlink | Kind::StaleLink) {
        let b = Rect::from_min_size(pos2(r.left(), r.bottom() - s * 0.42), Vec2::splat(s * 0.42));
        p.rect_filled(b, 2.0, Color32::WHITE);
        let col = if e.kind == Kind::StaleLink { pal.danger } else { Color32::BLACK };
        p.line_segment([b.left_bottom() + Vec2::new(2.0, -2.0), b.right_top() + Vec2::new(-2.0, 2.0)], Stroke::new(1.4_f32, col));
        p.line_segment([b.right_top() + Vec2::new(-2.0, 2.0), b.right_top() + Vec2::new(-2.0 - s * 0.16, 2.0)], Stroke::new(1.4_f32, col));
        p.line_segment([b.right_top() + Vec2::new(-2.0, 2.0), b.right_top() + Vec2::new(-2.0, 2.0 + s * 0.16)], Stroke::new(1.4_f32, col));
    }
}

pub const HELP: &[(&str, &str)] = &[
    ("", "Panels"),
    ("Tab", "Switch between the left and right panel"),
    ("Up / Down", "Move the cursor"),
    ("PgUp / PgDn", "Move one page"),
    ("Home / End", "First / last entry (empty command line)"),
    ("Enter", "Open folder, run program, or open file with its app"),
    ("Ctrl-PgUp, Cmd-Up", "Go to the parent folder"),
    ("Insert, Ctrl-T, Space", "Mark / unmark and move down"),
    ("Shift-Up/Down", "Mark while moving"),
    ("+  -  *", "Select group, unselect group, invert (empty command line)"),
    ("Right click", "Mark / unmark the file under the mouse"),
    ("Ctrl-S", "Quick search: type the start of a name"),
    ("Ctrl-R, Cmd-R", "Re-read both panels"),
    ("Ctrl-U", "Swap the panels"),
    ("Alt-. , Cmd-Shift-.", "Show / hide hidden files"),
    ("Alt-T", "Next listing format (full, brief, long)"),
    ("Ctrl-Space", "Compute the size of the folder under the cursor"),
    ("Alt-O", "Show this folder in the other panel"),
    ("Alt-Y / Alt-U", "Folder history back / forward"),
    ("", "Files"),
    ("F3", "View file (F2 wrap, F4 hex, F7 search)"),
    ("F4 / Shift-F4", "Edit file / new file (F2 save, F10 quit)"),
    ("F5", "Copy to the other panel"),
    ("F6", "Rename or move"),
    ("F7", "Create a folder"),
    ("F8, Cmd-Backspace", "Delete"),
    ("Ctrl-X c / s", "Change permissions / make a symlink"),
    ("Ctrl-X d", "Compare the two folders"),
    ("Ctrl-X i / q", "Info / quick view in the other panel"),
    ("", "Commands"),
    ("F2", "User menu (Finder, Terminal, Quick Look, zip)"),
    ("F9", "Pull-down menu"),
    ("Alt-?, Cmd-F", "Find file"),
    ("Ctrl-\\, Cmd-D", "Directory hotlist"),
    ("Enter on a command", "Run the typed shell command in this folder"),
    ("Ctrl-O", "Show the output of the last command"),
    ("Alt-Enter", "Put the file name on the command line"),
    ("Alt-P / Alt-N", "Command history"),
    ("F10", "Quit"),
];
