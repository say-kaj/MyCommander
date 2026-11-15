//! One file panel drawn like a Finder list: a clickable path bar, column
//! headers that sort, striped rows with file icons, a blue selection bar and a
//! footer with item count, free space and the marked summary (mc's
//! "%s in %d files"). The rules behind it (directories first, `..` can't be
//! marked, sort modes) live in `panel.rs`.

use crate::app::App;
use crate::fsutil::{self, Kind};
use crate::panel::{Listing, Panel, SortMode};
use crate::theme::*;
use crate::util::*;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::path::PathBuf;

impl App {
    pub fn draw_listing(&mut self, ui: &mut egui::Ui, idx: usize, rect: Rect) {
        let pal = self.pal;
        let active = idx == self.active;
        let fs = self.settings.font_size;
        let font = FontId::proportional(fs);
        let small = FontId::proportional(fs - 1.5);
        let row_h = (fs + 9.0).round();
        self.card(ui, rect, active);
        let painter = ui.painter_at(rect);

        // ---- Path bar (breadcrumbs) -----------------------------------------
        let path_h = 32.0;
        let path_bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), path_h));
        let mut crumbs: Vec<(String, PathBuf)> = Vec::new();
        let dir = self.panels[idx].dir.clone();
        let home = fsutil::home_dir();
        if crate::vfs::is_remote(&dir) {
            // sftp://host/a/b  →  [host] [a] [b]
            let mut cur = Some(dir.clone());
            while let Some(c) = cur {
                let up = crate::vfs::parent(&c);
                let name = if up.is_none() {
                    format!("⇄ {}", crate::vfs::parse(&c).map(|l| l.host).unwrap_or_default())
                } else {
                    crate::vfs::base_name(&c)
                };
                crumbs.push((name, c));
                cur = up;
            }
        }
        for anc in dir.ancestors().filter(|_| !crate::vfs::is_remote(&dir)) {
            let name = if anc == home {
                "🏠 ".to_string() + &anc.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            } else if anc.parent().is_none() {
                "Macintosh HD".to_string()
            } else {
                anc.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            };
            crumbs.push((name, anc.to_path_buf()));
            if anc == home {
                break;
            }
        }
        crumbs.reverse();
        // Fit from the right: drop leading crumbs that don't fit.
        let widths: Vec<f32> = crumbs.iter().map(|(n, _)| ui.fonts(|f| f.layout_no_wrap(n.clone(), small.clone(), pal.text).size().x) + 22.0).collect();
        let avail = path_bar.width() - 24.0;
        let mut first = 0;
        while first + 1 < crumbs.len() && widths[first..].iter().sum::<f32>() > avail {
            first += 1;
        }
        let mut x = path_bar.left() + 12.0;
        if first > 0 {
            painter.text(Pos2::new(x, path_bar.center().y), Align2::LEFT_CENTER, "… ›", small.clone(), pal.text_faint);
            x += 26.0;
        }
        let mut go_to: Option<PathBuf> = None;
        for (i, (name, path)) in crumbs.iter().enumerate().skip(first) {
            let last = i + 1 == crumbs.len();
            let g = painter.layout_no_wrap(name.clone(), small.clone(), if last { pal.text } else { pal.text_dim });
            let r = Rect::from_min_size(Pos2::new(x - 4.0, path_bar.center().y - 10.0), Vec2::new(g.size().x + 8.0, 20.0));
            let resp = ui.interact(r, ui.id().with(("crumb", idx, i)), Sense::click());
            if resp.hovered() {
                painter.rect_filled(r, 5.0, pal.control_hover);
            }
            painter.galley(Pos2::new(x, path_bar.center().y - g.size().y / 2.0), g.clone(), pal.text);
            x += g.size().x + 6.0;
            if !last {
                painter.text(Pos2::new(x, path_bar.center().y), Align2::LEFT_CENTER, "›", small.clone(), pal.text_faint);
                x += 12.0;
            }
            if resp.clicked() {
                go_to = Some(path.clone());
            }
        }
        if self.panels[idx].panelized.is_some() {
            let g = painter.layout_no_wrap("Panelized".to_string(), small.clone(), pal.accent_text);
            let r = Rect::from_min_size(Pos2::new(path_bar.right() - g.size().x - 26.0, path_bar.center().y - 10.0), Vec2::new(g.size().x + 14.0, 20.0));
            painter.rect_filled(r, 10.0, pal.accent);
            painter.galley(Pos2::new(r.left() + 7.0, r.center().y - g.size().y / 2.0), g, pal.accent_text);
        }
        painter.hline(rect.left()..=rect.right(), path_bar.bottom(), Stroke::new(1.0_f32, pal.separator));

        // ---- Areas ----------------------------------------------------------
        let footer_h = if self.settings.show_mini_status { 28.0 } else { 0.0 };
        let inner = Rect::from_min_max(Pos2::new(rect.left() + 6.0, path_bar.bottom()), Pos2::new(rect.right() - 6.0, rect.bottom() - footer_h));
        let header = Rect::from_min_size(inner.min, Vec2::new(inner.width(), row_h + 2.0));
        let list = Rect::from_min_max(Pos2::new(inner.left(), header.bottom() + 2.0), Pos2::new(inner.right(), inner.bottom()));
        let footer = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - footer_h), rect.max);

        // ---- Columns --------------------------------------------------------
        let listing = self.panels[idx].listing;
        let sort = self.panels[idx].sort.clone();
        let mut cols: Vec<(&str, f32, Option<SortMode>, bool)> = match listing {
            Listing::Full => {
                let date = 150.0;
                let size = 78.0;
                let kind = if inner.width() > 560.0 { 120.0 } else { 0.0 };
                let name = (inner.width() - date - size - kind).max(120.0);
                let mut v = vec![("Name", name, Some(SortMode::Name), false), ("Date Modified", date, Some(SortMode::MTime), false), ("Size", size, Some(SortMode::Size), true)];
                if kind > 0.0 {
                    v.push(("Kind", kind, Some(SortMode::Extension), false));
                }
                v
            }
            Listing::Brief => {
                let w = inner.width() / 2.0;
                vec![("Name", w, Some(SortMode::Name), false), ("", inner.width() - w, None, false)]
            }
            Listing::Long => {
                let perm = 96.0;
                let own = 70.0;
                let size = 84.0;
                let date = 104.0;
                let name = (inner.width() - perm - own * 2.0 - size - date).max(110.0);
                vec![
                    ("Name", name, Some(SortMode::Name), false),
                    ("Permissions", perm, None, false),
                    ("Owner", own, None, false),
                    ("Group", own, None, false),
                    ("Size", size, Some(SortMode::Size), true),
                    ("Modified", date, Some(SortMode::MTime), false),
                ]
            }
        };
        let total: f32 = cols.iter().map(|c| c.1).sum();
        if total > inner.width() {
            let k = inner.width() / total;
            for c in cols.iter_mut() {
                c.1 *= k;
            }
        }
        let mut xs = vec![inner.left()];
        for c in &cols {
            xs.push(xs.last().unwrap() + c.1);
        }

        // ---- Header (click to sort, again to reverse) -----------------------
        for (ci, c) in cols.iter().enumerate() {
            if c.0.is_empty() {
                continue;
            }
            let r = Rect::from_min_max(Pos2::new(xs[ci], header.top()), Pos2::new(xs[ci + 1], header.bottom()));
            let tx = if c.3 { r.right() - 10.0 } else { r.left() + if ci == 0 { 8.0 } else { 8.0 } };
            let align = if c.3 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
            let sorted = c.2 == Some(sort.mode);
            let g = painter.layout_no_wrap(c.0.to_string(), small.clone(), if sorted { pal.text } else { pal.text_dim });
            let gw = g.size().x;
            let pos = align.anchor_size(Pos2::new(tx, r.center().y), g.size());
            painter.galley(pos.min, g, pal.text_dim);
            if sorted {
                // little chevron like Finder's sort indicator
                let cx = if c.3 { pos.min.x - 10.0 } else { pos.min.x + gw + 10.0 };
                let cy = r.center().y;
                let d = if sort.reverse { -3.0 } else { 3.0 };
                painter.line_segment([Pos2::new(cx - 4.0, cy + d * 0.5), Pos2::new(cx, cy - d * 0.5)], Stroke::new(1.4_f32, pal.text_dim));
                painter.line_segment([Pos2::new(cx, cy - d * 0.5), Pos2::new(cx + 4.0, cy + d * 0.5)], Stroke::new(1.4_f32, pal.text_dim));
            }
            if ci > 0 {
                painter.vline(r.left(), r.top() + 6.0..=r.bottom() - 6.0, Stroke::new(1.0_f32, pal.separator));
            }
            if let Some(m) = c.2 {
                let resp = ui.interact(r, ui.id().with(("hdr", idx, ci)), Sense::click());
                if resp.clicked() {
                    let p = &mut self.panels[idx];
                    if p.sort.mode == m {
                        p.sort.reverse = !p.sort.reverse;
                    } else {
                        p.sort.mode = m;
                        p.sort.reverse = false;
                    }
                    p.resort();
                    self.active = idx;
                }
                if resp.hovered() {
                    painter.rect_filled(r.shrink(2.0), 4.0, pal.control_hover.gamma_multiply(0.5));
                }
            }
        }
        painter.hline(inner.left()..=inner.right(), header.bottom(), Stroke::new(1.0_f32, pal.separator));

        // ---- Scrolling ------------------------------------------------------
        let per_col = ((list.height() / row_h).floor() as usize).max(1);
        let ncols = if listing == Listing::Brief { 2 } else { 1 };
        let capacity = per_col * ncols;
        {
            let p: &mut Panel = &mut self.panels[idx];
            p.page_rows = per_col;
            p.clamp();
            if p.cursor < p.top {
                p.top = if ncols == 2 { p.cursor - p.cursor % per_col } else { p.cursor };
            }
            if p.cursor >= p.top + capacity {
                p.top = if ncols == 2 { (p.cursor / per_col + 1).saturating_sub(ncols) * per_col } else { p.cursor + 1 - capacity };
            }
            if ncols == 1 && p.entries.len() >= capacity && p.top + capacity > p.entries.len() {
                p.top = p.entries.len() - capacity;
            }
        }
        let list_resp = ui.interact(list, ui.id().with(("list", idx)), Sense::click());
        if ui.rect_contains_pointer(list) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.5 {
                let acc_id = ui.id().with(("scroll_acc", idx));
                let mut acc: f32 = ui.data(|d| d.get_temp(acc_id).unwrap_or(0.0)) + scroll;
                let steps = (acc / row_h).trunc() as isize;
                if steps != 0 {
                    acc -= steps as f32 * row_h;
                    self.panels[idx].move_cursor(-steps);
                }
                ui.data_mut(|d| d.insert_temp(acc_id, acc));
            }
        }

        // ---- Rows -----------------------------------------------------------
        let p = &self.panels[idx];
        let painter_list = ui.painter_at(list);
        let mut click_target: Option<(usize, bool, bool)> = None;
        for slot in 0..capacity {
            let row = slot % per_col;
            let col = slot / per_col;
            let y = list.top() + row as f32 * row_h;
            let (x0, x1) = if ncols == 2 { (xs[col], xs[col + 1]) } else { (inner.left(), inner.right()) };
            let rr = Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_h));
            if col == 0 && row % 2 == 1 {
                painter_list.rect_filled(Rect::from_min_max(Pos2::new(inner.left(), y), Pos2::new(inner.right(), y + row_h)), 5.0, pal.row_alt);
            }
            let i = p.top + slot;
            if i >= p.entries.len() {
                continue;
            }
            let e = &p.entries[i];
            let is_cursor = i == p.cursor;
            let sel_bar = rr.shrink2(Vec2::new(2.0, 1.0));
            if is_cursor && active {
                painter_list.rect_filled(sel_bar, 5.0, pal.accent);
            } else if e.marked {
                painter_list.rect_filled(sel_bar, 5.0, pal.marked_bg);
                if is_cursor {
                    painter_list.rect_stroke(sel_bar, 5.0, Stroke::new(1.5_f32, pal.sel_inactive), egui::StrokeKind::Inside);
                }
            } else if is_cursor {
                painter_list.rect_filled(sel_bar, 5.0, pal.sel_inactive);
            }
            let on_accent = is_cursor && active;
            let fg = if on_accent {
                pal.accent_text
            } else if e.marked {
                pal.marked_text
            } else if e.is_updir() || fsutil::highlight(e) == fsutil::Highlight::Temp {
                pal.text_dim
            } else {
                pal.text
            };
            let dim = if on_accent { pal.accent_text } else if e.marked { pal.marked_text } else { pal.text_dim };
            let ty = y + row_h / 2.0;
            // Icon + name
            let name_x1 = if ncols == 2 { x1 } else { xs[1] };
            let icon = Rect::from_min_size(Pos2::new(x0 + 8.0, y + 3.0), Vec2::splat(row_h - 6.0));
            draw_file_icon(&painter_list, icon, e, &pal);
            let mut name = if e.is_updir() { "Enclosing folder".to_string() } else { e.name.clone() };
            if e.marked {
                name = format!("✓ {}", name);
            }
            let max_w = name_x1 - icon.right() - 16.0;
            let shown = fit_text(ui, &name, &font, max_w);
            painter_list.text(Pos2::new(icon.right() + 7.0, ty), Align2::LEFT_CENTER, shown, font.clone(), fg);
            match listing {
                Listing::Full => {
                    let date = if e.is_updir() { String::new() } else { finder_date(e.mtime) };
                    painter_list.text(Pos2::new(xs[1] + 8.0, ty), Align2::LEFT_CENTER, fit_text(ui, &date, &font, xs[2] - xs[1] - 12.0), font.clone(), dim);
                    painter_list.text(Pos2::new(xs[3] - 10.0, ty), Align2::RIGHT_CENTER, finder_size(e), font.clone(), dim);
                    if cols.len() > 3 && !e.is_updir() {
                        let k = kind_label(fsutil::highlight(e), e.extension(), e.name.ends_with(".app") && e.is_dir_like());
                        painter_list.text(Pos2::new(xs[3] + 8.0, ty), Align2::LEFT_CENTER, fit_text(ui, &k, &font, xs[4] - xs[3] - 12.0), font.clone(), dim);
                    }
                }
                Listing::Brief => {}
                Listing::Long => {
                    let mono = FontId::monospace(fs - 1.0);
                    if !e.is_updir() {
                        painter_list.text(Pos2::new(xs[1] + 8.0, ty), Align2::LEFT_CENTER, fsutil::perm_string(e), mono, dim);
                        painter_list.text(Pos2::new(xs[2] + 8.0, ty), Align2::LEFT_CENTER, fit_text(ui, &fsutil::user_name(e.uid), &font, xs[3] - xs[2] - 12.0), font.clone(), dim);
                        painter_list.text(Pos2::new(xs[3] + 8.0, ty), Align2::LEFT_CENTER, fit_text(ui, &fsutil::group_name(e.gid), &font, xs[4] - xs[3] - 12.0), font.clone(), dim);
                    }
                    painter_list.text(Pos2::new(xs[5] - 10.0, ty), Align2::RIGHT_CENTER, size_cell(e, 9), font.clone(), dim);
                    painter_list.text(Pos2::new(xs[5] + 8.0, ty), Align2::LEFT_CENTER, fsutil::file_date(e.mtime), font.clone(), dim);
                }
            }
            if let Some(pos) = list_resp.interact_pointer_pos() {
                if rr.contains(pos) {
                    if list_resp.double_clicked() {
                        click_target = Some((i, true, false));
                    } else if list_resp.secondary_clicked() {
                        click_target = Some((i, false, true));
                    } else if list_resp.clicked() {
                        click_target = Some((i, false, false));
                    }
                }
            }
        }
        if ncols == 2 {
            painter.vline(xs[1], list.top()..=list.bottom(), Stroke::new(1.0_f32, pal.separator));
        }

        // ---- Footer ---------------------------------------------------------
        if footer_h > 0.0 {
        painter.hline(rect.left()..=rect.right(), footer.top(), Stroke::new(1.0_f32, pal.separator));
        let fy = footer.center().y;
        if let Some(err) = &p.error {
            painter.text(Pos2::new(footer.left() + 14.0, fy), Align2::LEFT_CENTER, err, small.clone(), pal.danger);
        } else if let Some(q) = &p.quick_search {
            let r = Rect::from_min_size(Pos2::new(footer.left() + 10.0, fy - 10.0), Vec2::new(220.0, 20.0));
            painter.rect_filled(r, 10.0, pal.field);
            painter.rect_stroke(r, 10.0, Stroke::new(1.5_f32, pal.accent), egui::StrokeKind::Inside);
            painter.text(Pos2::new(r.left() + 10.0, fy), Align2::LEFT_CENTER, format!("🔍 {}▏", q), small.clone(), pal.text);
        } else {
            let items = p.entries.iter().filter(|e| !e.is_updir()).count();
            let mut t = format!("{} item{}", items, if items == 1 { "" } else { "s" });
            if let Some((free, _)) = fsutil::free_space(&p.dir) {
                t.push_str(&format!(", {} available", fsutil::size_human(free).replace(" K", " KB").replace(" M", " MB").replace(" G", " GB").replace(" T", " TB")));
            }
            if let Some(f) = &p.filter {
                t.push_str(&format!("  ·  filter {}", f));
            }
            painter.text(Pos2::new(footer.left() + 14.0, fy), Align2::LEFT_CENTER, t, small.clone(), pal.text_dim);
        }
        let (n, bytes) = p.marked_totals();
        let right = if n > 0 {
            Some((format!("{} selected · {} bytes", n, fsutil::size_sep(bytes)), pal.marked_text))
        } else {
            p.current().filter(|e| !e.is_updir()).map(|e| {
                let s = if matches!(e.kind, Kind::Symlink | Kind::StaleLink) {
                    format!("→ {}", fsutil::read_link(&e.path).unwrap_or_else(|| "?".into()))
                } else {
                    format!("{}  {}", mini_size(e), fsutil::perm_string(e))
                };
                (s, pal.text_faint)
            })
        };
        if let Some((t, c)) = right {
            let t = fit_text(ui, &t, &small, footer.width() * 0.5);
            painter.text(Pos2::new(footer.right() - 14.0, fy), Align2::RIGHT_CENTER, t, small.clone(), c);
        }

        }

        // ---- Mouse results --------------------------------------------------
        if list_resp.clicked() || list_resp.secondary_clicked() || list_resp.double_clicked() {
            self.active = idx;
            self.menu = None;
        }
        if let Some(path) = go_to {
            self.active = idx;
            if let Err(e) = self.panels[idx].chdir(&path, true) {
                self.err(e);
            }
        }
        if let Some((i, double, secondary)) = click_target {
            self.active = idx;
            self.panels[idx].cursor = i;
            let cmd = ui.input(|inp| inp.modifiers.command);
            if secondary || cmd {
                self.panels[idx].toggle_mark_current();
            } else if double {
                let ctx = ui.ctx().clone();
                self.enter(&ctx);
            }
        }
    }
}

/// Shorten text with "…" in the middle so it fits `max_w` pixels.
pub fn fit_text(ui: &egui::Ui, s: &str, font: &FontId, max_w: f32) -> String {
    let w = |t: &str| ui.fonts(|f| f.layout_no_wrap(t.to_string(), font.clone(), egui::Color32::WHITE).size().x);
    if max_w <= 0.0 {
        return String::new();
    }
    if w(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let (mut lo, mut hi) = (1usize, chars.len());
    let make = |k: usize| -> String {
        let left = k.div_ceil(2);
        let right = k / 2;
        let a: String = chars[..left].iter().collect();
        let b: String = chars[chars.len() - right..].iter().collect();
        format!("{}…{}", a, b)
    };
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if w(&make(mid)) <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    make(lo)
}
