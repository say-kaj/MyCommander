//! Quick View content: images are shown as images, Markdown is rendered,
//! source code / HTML / shell scripts / config files are syntax highlighted.
//! Everything is configurable in Options > Quick view (see `settings.rs`).
//! Loading is cached per file (path, mtime, size, settings) so cursor moves are cheap.

use crate::settings::Settings;
use egui::{Color32, FontId, Pos2, Rect, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::path::{Path, PathBuf};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Names of the built-in highlighting themes offered in the options.
pub const THEMES: [&str; 7] = [
    "base16-ocean.dark",
    "base16-eighties.dark",
    "base16-mocha.dark",
    "Solarized (dark)",
    "base16-ocean.light",
    "InspiredGitHub",
    "Solarized (light)",
];

const MAX_LINES: usize = 20_000;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const IMAGE_EXTS: [&str; 10] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "ico", "jfif"];
/// Converted to a PNG by macOS `sips` (HEIC, first page of a PDF).
const SIPS_EXTS: [&str; 3] = ["heic", "heif", "pdf"];

type Segment = (Color32, String);

enum Content {
    Message(String),
    Dir(Vec<(bool, String)>),
    Image { tex: egui::TextureHandle, orig: [usize; 2], bytes: u64, note: String },
    Markdown(String),
    Code { lang: String, lines: Vec<Vec<Segment>>, bg: Color32, truncated: bool },
    Text(Vec<String>),
    Hex(Vec<String>),
}

#[derive(PartialEq, Eq, Clone)]
struct Key {
    path: PathBuf,
    mtime: i64,
    size: u64,
    sig: String,
}

struct Pending {
    entry: crate::fsutil::Entry,
    slot: std::sync::Arc<std::sync::Mutex<Option<Result<PathBuf, String>>>>,
}

const MAX_REMOTE_BYTES: u64 = 8 * 1024 * 1024;

pub struct Preview {
    pending: Option<Pending>,
    key: Option<Key>,
    content: Content,
    md_cache: CommonMarkCache,
    syntaxes: Option<SyntaxSet>,
    themes: Option<ThemeSet>,
}

impl Default for Preview {
    fn default() -> Self {
        Preview { pending: None, key: None, content: Content::Message(String::new()), md_cache: CommonMarkCache::default(), syntaxes: None, themes: None }
    }
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn signature(s: &Settings, dark: bool, show_hidden: bool) -> String {
    format!(
        "{}{}{}{}|{}|{}|{}|{}|{}",
        s.qv_images as u8, s.qv_markdown as u8, s.qv_highlight as u8, s.qv_line_numbers as u8,
        s.qv_max_kb, s.qv_theme_light, s.qv_theme_dark, s.qv_syntax_map, dark as u8 + 2 * show_hidden as u8
    )
}

/// Render an SVG to a bitmap (long side at most 2048 px).
fn render_svg(path: &Path) -> Result<(egui::ColorImage, [usize; 2]), String> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| e.to_string())?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));
    let scale = (2048.0 / w.max(h)).clamp(0.01, 4.0);
    let (pw, ph) = (((w * scale).ceil() as u32).max(1), ((h * scale).ceil() as u32).max(1));
    let mut pix = tiny_skia::Pixmap::new(pw, ph).ok_or("image too large")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    Ok((egui::ColorImage::from_rgba_premultiplied([pw as usize, ph as usize], pix.data()), [w as usize, h as usize]))
}

/// macOS only: let `sips` convert HEIC / the first PDF page to a PNG (long side 2048 px).
fn render_with_sips(path: &Path) -> Result<image::DynamicImage, String> {
    let tmp = std::env::temp_dir().join(format!("myCommander-qv-{}-{}.png", std::process::id(), path.to_string_lossy().len()));
    let out = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "2048"])
        .arg(path)
        .arg("--out")
        .arg(&tmp)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let img = image::open(&tmp).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    img
}

impl Preview {
    /// Draw the preview of `entry` (or a hint) into `rect`.
    #[allow(clippy::too_many_arguments)]
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, entry: Option<&crate::fsutil::Entry>, show_hidden: bool, s: &Settings, dark: bool, text: Color32, dim: Color32, mono: FontId) {
        match entry {
            None => {
                self.content = Content::Message("Nothing selected".into());
                self.key = None;
            }
            Some(e) => {
                let key = Key { path: e.path.clone(), mtime: e.mtime, size: e.size, sig: signature(s, dark, show_hidden) };
                if self.key.as_ref() != Some(&key) {
                    self.pending = None;
                    if crate::vfs::is_remote(&e.path) {
                        self.content = if e.is_dir_like() {
                            Content::Message(format!("{} – remote folder", e.name))
                        } else if e.size > MAX_REMOTE_BYTES {
                            Content::Message(format!("Remote file too large for quick view ({} MB) – press F3 to download it", e.size / 1024 / 1024))
                        } else {
                            let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
                            let (s2, p2, sz) = (slot.clone(), e.path.clone(), e.size);
                            std::thread::spawn(move || {
                                let r = crate::vfs::fetch(&p2, sz, MAX_REMOTE_BYTES).map_err(|e| e.to_string());
                                *s2.lock().unwrap() = Some(r);
                            });
                            self.pending = Some(Pending { entry: e.clone(), slot });
                            Content::Message("Loading …".into())
                        };
                    } else {
                        self.content = self.load(ui.ctx(), e, s, dark, show_hidden);
                    }
                    self.md_cache = CommonMarkCache::default();
                    self.key = Some(key);
                }
                if let Some(p) = &self.pending {
                    let done = p.slot.lock().unwrap().take();
                    match done {
                        Some(res) => {
                            let entry = p.entry.clone();
                            self.pending = None;
                            self.content = match res {
                                Ok(local) => {
                                    let mut le = entry;
                                    le.path = local;
                                    self.load(ui.ctx(), &le, s, dark, show_hidden)
                                }
                                Err(msg) => Content::Message(msg),
                            };
                            self.md_cache = CommonMarkCache::default();
                        }
                        None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
                    }
                }
            }
        }
        self.paint(ui, rect, text, dim, mono);
    }

    fn load(&mut self, ctx: &egui::Context, e: &crate::fsutil::Entry, s: &Settings, dark: bool, show_hidden: bool) -> Content {
        if e.is_dir_like() {
            let mut v = Vec::new();
            if let Ok(list) = crate::fsutil::list_dir(&e.path, show_hidden) {
                for c in list.iter().filter(|c| !c.is_updir()).take(500) {
                    v.push((c.is_dir_like(), c.name.clone()));
                }
            }
            return Content::Dir(v);
        }
        let ext = ext_of(&e.path);
        if s.qv_images && ext == "svg" {
            return match render_svg(&e.path) {
                Ok((ci, size)) => Content::Image { tex: ctx.load_texture(format!("qv:{}", e.path.display()), ci, egui::TextureOptions::LINEAR), orig: size, bytes: e.size, note: "SVG".into() },
                Err(err) => Content::Message(format!("Cannot render SVG: {err}")),
            };
        }
        if s.qv_images && SIPS_EXTS.contains(&ext.as_str()) {
            let note = if ext == "pdf" { "PDF, page 1" } else { "HEIC" };
            return match render_with_sips(&e.path) {
                Ok(img) => {
                    let orig = [img.width() as usize, img.height() as usize];
                    let rgba = img.to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    Content::Image { tex: ctx.load_texture(format!("qv:{}", e.path.display()), ci, egui::TextureOptions::LINEAR), orig, bytes: e.size, note: note.into() }
                }
                Err(err) => Content::Message(format!("Cannot render {note}: {err}")),
            };
        }
        if s.qv_images && IMAGE_EXTS.contains(&ext.as_str()) {
            if e.size > MAX_IMAGE_BYTES {
                return Content::Message(format!("Image too large to preview ({} MB)", e.size / 1024 / 1024));
            }
            return match image::open(&e.path) {
                Ok(img) => {
                    let orig = [img.width() as usize, img.height() as usize];
                    let img = if img.width().max(img.height()) > 2048 { img.thumbnail(2048, 2048) } else { img };
                    let rgba = img.to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    let tex = ctx.load_texture(format!("qv:{}", e.path.display()), ci, egui::TextureOptions::LINEAR);
                    Content::Image { tex, orig, bytes: e.size, note: String::new() }
                }
                Err(err) => Content::Message(format!("Cannot decode image: {err}")),
            };
        }
        // Text-like content: read up to the configured limit.
        use std::io::Read;
        let limit = (s.qv_max_kb.max(1) as u64) * 1024;
        let mut buf = Vec::new();
        match std::fs::File::open(&e.path) {
            Ok(f) => {
                let _ = f.take(limit).read_to_end(&mut buf);
            }
            Err(err) => return Content::Message(format!("Cannot read: {err}")),
        }
        let truncated = e.size > limit;
        if buf.iter().take(4096).any(|&b| b == 0) {
            return Content::Hex(buf.chunks(16).take(2000).enumerate().map(|(i, c)| crate::util::hex_line(i * 16, c)).collect());
        }
        let text = String::from_utf8_lossy(&buf).to_string();
        if s.qv_markdown && matches!(ext.as_str(), "md" | "markdown" | "mdown") {
            return Content::Markdown(text);
        }
        if s.qv_highlight {
            if let Some(c) = self.highlight(&e.path, &text, s, dark, truncated) {
                return c;
            }
        }
        Content::Text(text.lines().take(MAX_LINES).map(|l| l.replace('\t', "    ")).collect())
    }

    fn highlight(&mut self, path: &Path, text: &str, s: &Settings, dark: bool, truncated: bool) -> Option<Content> {
        let ss = self.syntaxes.get_or_insert_with(two_face::syntax::extra_newlines);
        let ts = self.themes.get_or_insert_with(ThemeSet::load_defaults);
        let ext = ext_of(path);
        let fname = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // 1. user mapping "ext=Syntax name", 2. extension, 3. well-known names, 4. shebang
        let mapped = s
            .qv_syntax_map
            .lines()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| k.trim().trim_start_matches('.').eq_ignore_ascii_case(&ext) || k.trim() == fname)
            .and_then(|(_, v)| ss.find_syntax_by_name(v.trim()));
        let bash = || ss.find_syntax_by_name("Bourne Again Shell (bash)");
        let n = fname.trim_start_matches('.');
        let builtin = match ext.as_str() {
            "zsh" | "command" | "bash" | "sh" => bash(),
            _ if matches!(n, "zshrc" | "bashrc" | "profile" | "zprofile" | "bash_profile" | "zshenv") => bash(),
            _ if fname == "Cargo.lock" => ss.find_syntax_by_extension("toml"),
            _ if matches!(fname.as_str(), "Dockerfile" | "Makefile") => ss.find_syntax_by_extension(&fname),
            _ => None,
        };
        let syn = mapped
            .or_else(|| if ext.is_empty() { None } else { ss.find_syntax_by_extension(&ext) })
            .or(builtin)
            .or_else(|| ss.find_syntax_by_first_line(text.lines().next().unwrap_or("")))?;
        let tname = if dark { &s.qv_theme_dark } else { &s.qv_theme_light };
        let theme: &Theme = ts.themes.get(tname.as_str()).or_else(|| ts.themes.values().next())?;
        let bg = theme.settings.background.map(|c| Color32::from_rgb(c.r, c.g, c.b)).unwrap_or(if dark { Color32::from_gray(30) } else { Color32::WHITE });
        let mut hl = HighlightLines::new(syn, theme);
        let mut lines = Vec::new();
        let lang = syn.name.clone();
        for (n, line) in syntect::util::LinesWithEndings::from(text).take(MAX_LINES).enumerate() {
            let regions = hl.highlight_line(line, ss).ok()?;
            let mut segs: Vec<Segment> = Vec::with_capacity(regions.len() + 1);
            if s.qv_line_numbers {
                segs.push((Color32::from_gray(if dark { 120 } else { 150 }), format!("{:>4}  ", n + 1)));
            }
            for (st, t) in regions {
                let t = t.trim_end_matches(['\n', '\r']).replace('\t', "    ");
                if !t.is_empty() {
                    segs.push((Color32::from_rgb(st.foreground.r, st.foreground.g, st.foreground.b), t));
                }
            }
            lines.push(segs);
        }
        Some(Content::Code { lang, lines, bg, truncated })
    }

    fn paint(&mut self, ui: &mut egui::Ui, rect: Rect, text: Color32, dim: Color32, mono: FontId) {
        let row_h = mono.size + 4.0;
        match &self.content {
            Content::Message(m) => {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, m, FontId::proportional(14.0), dim);
            }
            Content::Dir(v) => {
                let mut r = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                egui::ScrollArea::vertical().id_salt("qv_dir").auto_shrink([false, false]).show_rows(&mut r, row_h, v.len(), |ui, range| {
                    for (is_dir, name) in &v[range] {
                        ui.label(egui::RichText::new(format!("{}{}", if *is_dir { "▸ " } else { "   " }, name)).font(mono.clone()).color(text));
                    }
                });
            }
            Content::Image { tex, orig, bytes, note } => {
                let mut caption = format!("{} × {} px · {}", orig[0], orig[1], crate::fsutil::size_human(*bytes));
                if !note.is_empty() {
                    caption = format!("{note} · {caption}");
                }
                let img_area = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - 22.0));
                let ts = tex.size_vec2();
                let scale = (img_area.width() / ts.x).min(img_area.height() / ts.y);
                let r = Rect::from_center_size(img_area.center(), ts * scale);
                // faint backdrop so transparent images stay visible
                ui.painter().rect_filled(r, 2.0, Color32::from_gray(128).gamma_multiply(0.25));
                ui.painter().image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                ui.painter().text(Pos2::new(rect.center().x, rect.bottom() - 10.0), egui::Align2::CENTER_CENTER, caption, FontId::proportional(12.0), dim);
            }
            Content::Markdown(md) => {
                let mut r = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("qv_md").show(&mut r, |ui| {
                    ui.set_width(rect.width() - 12.0);
                    CommonMarkViewer::new().show(ui, &mut self.md_cache, md);
                });
            }
            Content::Code { lang, lines, bg, truncated } => {
                ui.painter().rect_filled(rect.expand(6.0), 6.0, *bg);
                let mut r = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let total = lines.len();
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("qv_code").show_rows(&mut r, row_h, total, |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for l in &lines[range] {
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = f32::INFINITY;
                        for (c, t) in l {
                            job.append(t, 0.0, egui::TextFormat { font_id: mono.clone(), color: *c, ..Default::default() });
                        }
                        if l.is_empty() {
                            job.append(" ", 0.0, egui::TextFormat { font_id: mono.clone(), color: text, ..Default::default() });
                        }
                        ui.add(egui::Label::new(job).extend().selectable(false));
                    }
                });
                let mut tag = lang.clone();
                if *truncated {
                    tag.push_str(" · truncated");
                }
                let g = ui.painter().layout_no_wrap(tag, FontId::proportional(11.0), dim);
                let tr = Rect::from_min_size(Pos2::new(rect.right() - g.size().x - 10.0, rect.top() - 2.0), g.size() + Vec2::new(8.0, 2.0));
                ui.painter().rect_filled(tr, 4.0, bg.gamma_multiply(0.85));
                ui.painter().galley(tr.min + Vec2::new(4.0, 1.0), g, dim);
            }
            Content::Text(lines) => {
                let mut r = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("qv_text").show_rows(&mut r, row_h, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for l in &lines[range] {
                        ui.add(egui::Label::new(egui::RichText::new(if l.is_empty() { " " } else { l }).font(mono.clone()).color(text)).extend().selectable(false));
                    }
                });
            }
            Content::Hex(lines) => {
                let mut r = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("qv_hex").show_rows(&mut r, row_h, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for l in &lines[range] {
                        ui.add(egui::Label::new(egui::RichText::new(l).font(mono.clone()).color(text)).extend().selectable(false));
                    }
                });
            }
        }
    }
}
