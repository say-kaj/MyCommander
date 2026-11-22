//! Full-window screens: the internal viewer (F3), the editor (F4), the command
//! output (Ctrl-O), and the `--screenshot` capture.

use crate::app::{App, Screen};
use crate::fsutil;
use crate::theme::*;
use crate::ui_main::draw_bar;
use crate::util::*;
use egui::{Align2, FontId, Key, Vec2};
use std::path::{Path, PathBuf};

pub struct Viewer {
    pub path: PathBuf,
    /// What is shown: the raw bytes in hex mode, the text (as UTF-8) otherwise.
    pub data: Vec<u8>,
    raw: Vec<u8>,
    text: Vec<u8>,
    pub encoding: crate::textenc::TextEncoding,
    pub truncated: bool,
    pub hex: bool,
    pub wrap: bool,
    pub lines: Vec<(usize, usize)>,
    pub search: String,
    pub asking_search: bool,
    pub asking_goto: bool,
    pub goto: String,
    pub match_at: Option<usize>,
    pub scroll_to_row: Option<usize>,
    pub message: Option<String>,
}

impl Viewer {
    pub fn open(path: &Path, want: crate::textenc::TextEncoding) -> Result<Viewer, String> {
        use std::io::Read;
        const LIMIT: u64 = 32 << 20;
        let f = std::fs::File::open(path).map_err(|e| format!("Cannot open \"{}\"\n{}", path.display(), e))?;
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut data = Vec::new();
        f.take(LIMIT).read_to_end(&mut data).map_err(|e| e.to_string())?;
        let binary = crate::textenc::looks_binary(&data, want);
        let (text, encoding) = if binary {
            (Vec::new(), crate::textenc::TextEncoding::Utf8)
        } else {
            let d = crate::textenc::decode(&data, want);
            (d.text.into_bytes(), d.enc)
        };
        let mut v = Viewer {
            path: path.to_path_buf(),
            truncated: len > LIMIT,
            hex: binary,
            data: if binary { data.clone() } else { text.clone() },
            raw: data,
            text,
            encoding,
            wrap: false,
            lines: Vec::new(),
            search: String::new(),
            asking_search: false,
            asking_goto: false,
            goto: String::new(),
            match_at: None,
            scroll_to_row: None,
            message: None,
        };
        v.split_lines();
        Ok(v)
    }

    /// Switch between the hex dump (raw bytes) and the text view (decoded text).
    pub fn set_hex(&mut self, hex: bool) {
        self.hex = hex;
        self.data = if hex { self.raw.clone() } else { self.text.clone() };
        self.match_at = None;
        self.split_lines();
    }

    fn split_lines(&mut self) {
        self.lines.clear();
        let mut start = 0;
        for (i, &b) in self.data.iter().enumerate() {
            if b == b'\n' {
                self.lines.push((start, i));
                start = i + 1;
            }
        }
        if start < self.data.len() || self.lines.is_empty() {
            self.lines.push((start, self.data.len()));
        }
    }

    fn line_of(&self, off: usize) -> usize {
        match self.lines.binary_search_by(|&(s, _)| s.cmp(&off)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn row_of(&self, off: usize) -> usize {
        if self.hex {
            off / 16
        } else {
            self.line_of(off)
        }
    }

    pub fn find_next(&mut self) {
        if self.search.is_empty() {
            return;
        }
        let needle = self.search.to_lowercase().into_bytes();
        let start = self.match_at.map(|m| m + 1).unwrap_or(0);
        let hay = &self.data;
        let end = (hay.len() + 1).saturating_sub(needle.len());
        let found = (start..end).find(|&i| hay[i..i + needle.len()].iter().zip(&needle).all(|(a, b)| a.to_ascii_lowercase() == *b));
        match found {
            Some(i) => {
                self.match_at = Some(i);
                self.scroll_to_row = Some(self.row_of(i));
                self.message = None;
            }
            None => self.message = Some(format!("Search string not found: \"{}\"", self.search)),
        }
    }
}

pub struct Editor {
    pub path: PathBuf,
    pub text: String,
    pub original: String,
    pub is_new: bool,
    pub search: String,
    pub asking_search: bool,
    pub confirm_quit: bool,
    pub message: Option<String>,
    pub cursor_char: usize,
    pub line_col: (usize, usize),
    pub select_range: Option<(usize, usize)>,
    /// Set when this is a downloaded copy of a remote file: Save uploads it back.
    pub remote: Option<PathBuf>,
    /// Encoding the file was read with; Save writes it back the same way.
    pub encoding: crate::textenc::TextEncoding,
    pub bom: bool,
}

impl Editor {
    pub fn new(path: PathBuf, text: String, is_new: bool) -> Self {
        Editor {
            path,
            original: text.clone(),
            text,
            is_new,
            search: String::new(),
            asking_search: false,
            confirm_quit: false,
            message: None,
            cursor_char: 0,
            line_col: (1, 1),
            select_range: None,
            remote: None,
            encoding: crate::textenc::TextEncoding::Utf8,
            bom: false,
        }
    }

    fn modified(&self) -> bool {
        self.text != self.original
    }

    fn save(&mut self) -> bool {
        match std::fs::write(&self.path, crate::textenc::encode(&self.text, self.encoding, self.bom)) {
            Ok(()) => {
                if let Some(r) = &self.remote {
                    if let Err(e) = crate::vfs::upload(&self.path, r) {
                        self.message = Some(format!("Saved locally, upload failed: {e}"));
                        return false;
                    }
                }
                self.original = self.text.clone();
                self.is_new = false;
                self.message = Some(if self.remote.is_some() { "Saved and uploaded".into() } else { "Saved".into() });
                true
            }
            Err(e) => {
                self.message = Some(format!("Cannot save: {}", e));
                false
            }
        }
    }

    fn find(&mut self) {
        if self.search.is_empty() {
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        let needle: Vec<char> = self.search.to_lowercase().chars().collect();
        let n = needle.len();
        let start = self.select_range.map(|r| r.1).unwrap_or(self.cursor_char);
        let end = (chars.len() + 1).saturating_sub(n);
        let find_from = |from: usize| (from..end).find(|&i| chars[i..i + n].iter().zip(&needle).all(|(a, b)| a.to_lowercase().next() == Some(*b)));
        match find_from(start.min(end)).or_else(|| find_from(0)) {
            Some(i) => {
                self.select_range = Some((i, i + n));
                self.message = None;
            }
            None => self.message = Some(format!("Search string not found: \"{}\"", self.search)),
        }
    }
}

fn prompt_window(ctx: &egui::Context, pal: &Pal, title: &str, label: &str, buf: &mut String) -> (bool, bool) {
    let mut done = false;
    let mut cancel = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(egui::Frame::window(&ctx.style()).fill(pal.menu_bg).corner_radius(12.0).inner_margin(egui::Margin::same(18)))
        .show(ctx, |ui| {
            ui.label(label);
            let r = ui.add(egui::TextEdit::singleline(buf).desired_width(320.0).font(FontId::monospace(14.0)));
            r.request_focus();
            let (ok, c) = ok_cancel(ui);
            done = ok || ui.input(|i| i.key_pressed(Key::Enter));
            cancel = c || ui.input(|i| i.key_pressed(Key::Escape));
        });
    drop_keys(ctx, &[Key::Enter, Key::Escape]);
    (done, cancel)
}

/// Title-bar toolbar for the viewer, editor and output screens.
fn status_bar(ctx: &egui::Context, pal: &Pal, id: &str, fs: f32, left: String, right: String) {
    egui::TopBottomPanel::top(id.to_string())
        .exact_height(52.0)
        .frame(egui::Frame::NONE.fill(pal.toolbar).stroke(egui::Stroke::new(0.5_f32, pal.toolbar_line)).inner_margin(egui::Margin { left: 90, right: 14, top: 0, bottom: 0 }))
        .show(ctx, |ui| {
            let bg = ui.interact(ui.max_rect(), ui.id().with("drag"), egui::Sense::click_and_drag());
            if bg.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(left).font(FontId::proportional(fs + 1.0)).strong().color(pal.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(right).font(FontId::proportional(fs - 0.5)).color(pal.text_dim));
                });
            });
        });
}

impl App {
    pub fn draw_viewer(&mut self, ctx: &egui::Context) -> bool {
        let fs = self.settings.font_size;
        let pal = self.pal;
        let Screen::Viewer(v) = &mut self.screen else { return false };
        let mono = FontId::monospace(fs);
        let mut close = false;
        if !v.asking_search && !v.asking_goto {
            if take(ctx, Key::F3, NONE) || take(ctx, Key::F10, NONE) || take(ctx, Key::Escape, NONE) || take_text(ctx, "q") {
                close = true;
            }
            if take(ctx, Key::F2, NONE) {
                v.wrap = !v.wrap;
            }
            if take(ctx, Key::F4, NONE) {
                v.set_hex(!v.hex);
                v.scroll_to_row = v.match_at.map(|m| v.row_of(m));
            }
            if take(ctx, Key::F7, NONE) || take_text(ctx, "/") || take(ctx, Key::F, CMD) {
                v.asking_search = true;
            }
            if take(ctx, Key::F5, NONE) {
                v.asking_goto = true;
            }
            if take_text(ctx, "n") || take(ctx, Key::F17, NONE) || take(ctx, Key::G, CMD) {
                v.find_next();
            }
            drop_text(ctx);
        }
        let labels: Vec<(&str, Option<u8>)> = vec![
            ("Help", None),
            (if v.wrap { "UnWrap" } else { "Wrap" }, Some(2)),
            ("Quit", Some(3)),
            (if v.hex { "Ascii" } else { "Hex" }, Some(4)),
            ("Goto", Some(5)),
            ("", None),
            ("Search", Some(7)),
            ("", None),
            ("", None),
            ("Quit", Some(10)),
        ];
        match draw_bar(ctx, "viewer_bar", fs, &pal, &labels) {
            Some(2) => v.wrap = !v.wrap,
            Some(3) | Some(10) => close = true,
            Some(4) => v.set_hex(!v.hex),
            Some(5) => v.asking_goto = true,
            Some(7) => v.asking_search = true,
            _ => {}
        }
        let info = format!(
            "{} bytes{}  ·  {} lines  ·  {}",
            fsutil::size_sep(v.data.len() as u64),
            if v.truncated { " (first 32 MB)" } else { "" },
            v.lines.len(),
            if v.hex {
                "hex".to_string()
            } else if v.wrap {
                format!("wrap  ·  {}", v.encoding.short())
            } else {
                format!("no wrap  ·  {}", v.encoding.short())
            }
        );
        status_bar(ctx, &pal, "viewer_status", fs, v.path.display().to_string(), info);
        let row_h = ctx.fonts(|f| f.row_height(&mono));
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(pal.code_bg).inner_margin(egui::Margin::symmetric(16, 10))).show(ctx, |ui| {
            if let Some(msg) = &v.message {
                ui.colored_label(pal.danger, msg);
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut sa = if v.wrap && !v.hex { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() }.auto_shrink([false, false]);
            if let Some(r) = v.scroll_to_row.take() {
                sa = sa.vertical_scroll_offset((r as f32 * row_h - 40.0).max(0.0));
            }
            let match_at = v.match_at;
            let mlen = v.search.len().max(1);
            if v.hex {
                let n = v.data.len().div_ceil(16).max(1);
                sa.show_rows(ui, row_h, n, |ui, range| {
                    for r in range {
                        let s = r * 16;
                        let e = (s + 16).min(v.data.len());
                        let hit = match_at.map(|m| m < e && m + mlen > s).unwrap_or(false);
                        ui.label(egui::RichText::new(hex_line(s, &v.data[s..e])).font(mono.clone()).color(if hit { pal.marked_text } else { pal.code_text }));
                    }
                });
            } else {
                let n = v.lines.len();
                let wrap = v.wrap;
                sa.show_rows(ui, row_h, n, |ui, range| {
                    for r in range {
                        let (s, e) = v.lines[r];
                        let text = String::from_utf8_lossy(&v.data[s..e]).replace('\t', "    ");
                        let hit = match_at.map(|m| m >= s && m <= e).unwrap_or(false);
                        let label = egui::Label::new(egui::RichText::new(text).font(mono.clone()).color(if hit { pal.marked_text } else { pal.code_text }));
                        ui.add(if wrap { label.wrap() } else { label.extend() });
                    }
                });
            }
        });
        if v.asking_search {
            let (done, cancel) = prompt_window(ctx, &pal, "Search", "Enter search string:", &mut v.search);
            if done {
                v.asking_search = false;
                v.match_at = None;
                v.find_next();
            } else if cancel {
                v.asking_search = false;
            }
        } else if v.asking_goto {
            let (done, cancel) = prompt_window(ctx, &pal, "Goto line", "Line number:", &mut v.goto);
            if done {
                if let Ok(n) = v.goto.trim().parse::<usize>() {
                    let line = n.saturating_sub(1).min(v.lines.len().saturating_sub(1));
                    v.scroll_to_row = Some(if v.hex { v.lines[line].0 / 16 } else { line });
                }
                v.asking_goto = false;
            } else if cancel {
                v.asking_goto = false;
            }
        }
        if close {
            self.screen = Screen::Main;
        }
        true
    }

    pub fn draw_editor(&mut self, ctx: &egui::Context) -> bool {
        let fs = self.settings.font_size;
        let pal = self.pal;
        let Screen::Editor(ed) = &mut self.screen else { return false };
        let mono = FontId::monospace(fs);
        let mut save = false;
        let mut quit = false;
        if !ed.asking_search && !ed.confirm_quit {
            if take(ctx, Key::F2, NONE) || take(ctx, Key::S, CMD) {
                save = true;
            }
            if take(ctx, Key::F10, NONE) || take(ctx, Key::Escape, NONE) {
                quit = true;
            }
            if take(ctx, Key::F7, NONE) || take(ctx, Key::F, CMD) {
                ed.asking_search = true;
            }
            if take(ctx, Key::F17, NONE) || take(ctx, Key::G, CMD) {
                ed.find();
            }
        }
        let labels: Vec<(&str, Option<u8>)> = vec![
            ("Help", None),
            ("Save", Some(2)),
            ("", None),
            ("", None),
            ("", None),
            ("", None),
            ("Search", Some(7)),
            ("", None),
            ("", None),
            ("Quit", Some(10)),
        ];
        match draw_bar(ctx, "editor_bar", fs, &pal, &labels) {
            Some(2) => save = true,
            Some(7) => ed.asking_search = true,
            Some(10) => quit = true,
            _ => {}
        }
        let left = format!("{}{}", if ed.modified() { "* " } else { "" }, ed.path.display());
        let mut right = format!("Ln {}  Col {}  ·  {} bytes", ed.line_col.0, ed.line_col.1, fsutil::size_sep(ed.text.len() as u64));
        if let Some(m) = &ed.message {
            right = format!("{}   ·   {}", m, right);
        }
        status_bar(ctx, &pal, "editor_status", fs, left, right);
        let id = egui::Id::new("editor_text");
        let busy = ed.asking_search || ed.confirm_quit;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(pal.code_bg).inner_margin(egui::Margin::symmetric(14, 8))).show(ctx, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                let out = egui::TextEdit::multiline(&mut ed.text)
                    .id(id)
                    .font(mono.clone())
                    .text_color(pal.code_text)
                    .frame(false)
                    .code_editor()
                    .lock_focus(true)
                    .desired_width(f32::INFINITY)
                    .desired_rows(40)
                    .show(ui);
                if !busy && !out.response.has_focus() {
                    out.response.request_focus();
                }
                if let Some((a, b)) = ed.select_range.take() {
                    let mut st = out.state.clone();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
                    st.store(ui.ctx(), id);
                    let pos = out.galley.pos_from_ccursor(egui::text::CCursor::new(b));
                    ui.scroll_to_rect(pos.translate(out.galley_pos.to_vec2()), Some(egui::Align::Center));
                    // Remember where the match ended so F7/Cmd-G continues from there.
                    ed.cursor_char = b;
                    ed.message = Some(format!("Found \"{}\"", ed.search));
                } else if let Some(r) = out.cursor_range {
                    ed.cursor_char = r.primary.ccursor.index;
                }
                let before: String = ed.text.chars().take(ed.cursor_char).collect();
                let line = before.matches('\n').count() + 1;
                let col = before.rsplit('\n').next().map(|s| s.chars().count()).unwrap_or(0) + 1;
                ed.line_col = (line, col);
            });
        });
        if ed.asking_search {
            let (done, cancel) = prompt_window(ctx, &pal, "Search", "Enter search string:", &mut ed.search);
            if done {
                ed.asking_search = false;
                ed.find();
            } else if cancel {
                ed.asking_search = false;
            }
        }
        if save {
            ed.save();
        }
        let mut close = false;
        if quit {
            if ed.modified() && !(ed.is_new && ed.text.is_empty()) {
                ed.confirm_quit = true;
            } else {
                close = true;
            }
        }
        if ed.confirm_quit {
            let mut choice = 0;
            let name = ed.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            egui::Window::new("Quit")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .frame(egui::Frame::window(&ctx.style()).fill(pal.menu_bg).corner_radius(12.0).inner_margin(egui::Margin::same(18)))
                .show(ctx, |ui| {
                    ui.label(format!("File \"{}\" was modified.\nSave before close?", name));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if crate::util::lbutton(ui, "Yes").clicked() {
                            choice = 1;
                        }
                        if crate::util::lbutton(ui, "No").clicked() {
                            choice = 2;
                        }
                        if crate::util::lbutton(ui, "Cancel").clicked() {
                            choice = 3;
                        }
                    });
                });
            if choice == 1 {
                if ed.save() {
                    close = true;
                } else {
                    ed.confirm_quit = false;
                }
            } else if choice == 2 {
                close = true;
            } else if choice == 3 || take(ctx, Key::Escape, NONE) {
                ed.confirm_quit = false;
            }
        }
        if close {
            self.screen = Screen::Main;
            self.reload_both();
        }
        true
    }

    pub fn draw_output(&mut self, ctx: &egui::Context) -> bool {
        if !matches!(self.screen, Screen::Output) {
            return false;
        }
        let fs = self.settings.font_size;
        let mono = FontId::monospace(fs);
        if take(ctx, Key::O, CTRL) || take(ctx, Key::Escape, NONE) || take(ctx, Key::F10, NONE) || take(ctx, Key::Enter, NONE) {
            self.screen = Screen::Main;
            return true;
        }
        let mut labels: Vec<(&str, Option<u8>)> = vec![("", None); 9];
        labels.push(("Panels", Some(10)));
        if draw_bar(ctx, "output_bar", fs, &self.pal, &labels) == Some(10) {
            self.screen = Screen::Main;
        }
        status_bar(ctx, &self.pal, "out_status", fs, "Command output".into(), "Ctrl-O, Enter or Esc: back to the panels".into());
        let pal = self.pal;
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(pal.code_bg).inner_margin(egui::Margin::symmetric(16, 10))).show(ctx, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                let text = if self.last_output.is_empty() {
                    "No command has been run yet. Type a command at the bottom and press Enter."
                } else {
                    &self.last_output
                };
                ui.add(egui::Label::new(egui::RichText::new(text).font(mono.clone()).color(pal.code_text)).extend());
            });
        });
        true
    }

    /// `--screenshot <file>`: draw about 120 frames, save the window as PNG, exit.
    pub fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frame += 1;
        ctx.request_repaint();
        if self.frame == 60 {
            // Show a cursor and a marked file so the picture shows the colours.
            let p = &mut self.panels[0];
            if p.entries.len() > 4 {
                p.cursor = 3;
                p.set_mark(2, true);
            }
        }
        if self.frame == 70 {
            // Test aid: MYCMD_SHOT_QV=<file> shows that file in the right panel's Quick View.
            if let Ok(f) = std::env::var("MYCMD_SHOT_QV") {
                let f = std::path::PathBuf::from(f);
                if let (Some(dir), Some(name)) = (f.parent(), f.file_name()) {
                    self.panels[0] = crate::panel::Panel::new(dir.to_path_buf());
                    self.panels[0].select_name(&name.to_string_lossy());
                    self.panels[1].view = crate::panel::PanelView::QuickView;
                }
            }
        }
        if self.frame == 80 {
            // Test aid: MYCMD_SHOT_MENU=<0..4> opens that mc menu for the picture.
            if let Some(i) = std::env::var("MYCMD_SHOT_MENU").ok().and_then(|v| v.parse::<usize>().ok()) {
                self.menu = Some(crate::app::MenuState { top: i.min(3), sel: 0, user: false });
            }
        }
        if self.frame == 80 {
            // Test aid: MYCMD_SHOT_OPT=<0..3> opens that Settings tab for the picture.
            if let Some(i) = std::env::var("MYCMD_SHOT_OPT").ok().and_then(|v| v.parse::<usize>().ok()) {
                let page = crate::app::OptPage::ALL[i.min(3)];
                self.dialog = Some(crate::app::Dialog::Options { page, s: Box::new(self.settings.clone()) });
            }
        }
        if self.frame == 80 {
            // Test aids: MYCMD_SHOT_HELP=<0..2> opens that Help page, MYCMD_SHOT_ABOUT=1 the About box.
            if let Some(i) = std::env::var("MYCMD_SHOT_HELP").ok().and_then(|v| v.parse::<usize>().ok()) {
                self.dialog = Some(crate::app::Dialog::Help { page: i.min(2) });
            }
            if std::env::var("MYCMD_SHOT_ABOUT").is_ok() {
                self.dialog = Some(crate::app::Dialog::About);
            }
        }
        if self.frame == 80 && std::env::var("MYCMD_SHOT_PLACES").is_ok() {
            // Test aid: MYCMD_SHOT_PLACES=1 opens the Places dialog for the picture.
            self.do_action(ctx, crate::app::Action::Hotlist);
        }
        if self.frame >= 120 && !self.screenshot_requested {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
        });
        if let Some(img) = shot {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for c in &img.pixels {
                buf.extend_from_slice(&[c.r(), c.g(), c.b(), 255]);
            }
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                Some(im) => {
                    if let Err(e) = im.save(&path) {
                        eprintln!("myCommander: cannot save screenshot: {}", e);
                    }
                }
                None => eprintln!("myCommander: bad screenshot buffer"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frame > 900 {
            eprintln!("myCommander: screenshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
