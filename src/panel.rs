//! One file panel: its folder, entries, cursor, marks, sort order and history.

use crate::fsutil::{self, Entry, Kind};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    Unsorted,
    Name,
    Version,
    Extension,
    Size,
    MTime,
    ATime,
    CTime,
    Inode,
}

impl SortMode {
    pub const ALL: [SortMode; 9] = [
        SortMode::Unsorted,
        SortMode::Name,
        SortMode::Version,
        SortMode::Extension,
        SortMode::Size,
        SortMode::MTime,
        SortMode::ATime,
        SortMode::CTime,
        SortMode::Inode,
    ];
    pub fn label(self) -> &'static str {
        match self {
            SortMode::Unsorted => "Unsorted",
            SortMode::Name => "Name",
            SortMode::Version => "Version",
            SortMode::Extension => "Extension",
            SortMode::Size => "Size",
            SortMode::MTime => "Modify time",
            SortMode::ATime => "Access time",
            SortMode::CTime => "Change time",
            SortMode::Inode => "Inode",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Listing {
    Full,
    Brief,
    Long,
}

impl Listing {
    pub fn next(self) -> Self {
        match self {
            Listing::Full => Listing::Brief,
            Listing::Brief => Listing::Long,
            Listing::Long => Listing::Full,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Listing::Full => "Full file list",
            Listing::Brief => "Brief file list",
            Listing::Long => "Long file list",
        }
    }
}

/// What a panel shows: a file list, or information about the other panel's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelView {
    Listing,
    Info,
    QuickView,
    Tree,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SortOptions {
    pub mode: SortMode,
    pub reverse: bool,
    pub case_sensitive: bool,
    pub exec_first: bool,
    pub mix_all: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        SortOptions { mode: SortMode::Name, reverse: false, case_sensitive: false, exec_first: false, mix_all: false }
    }
}

pub struct Panel {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub top: usize,
    pub sort: SortOptions,
    pub listing: Listing,
    pub view: PanelView,
    pub show_hidden: bool,
    pub filter: Option<String>,
    pub history: Vec<PathBuf>,
    pub history_pos: usize,
    pub error: Option<String>,
    pub quick_search: Option<String>,
    /// Rows visible on screen at the last draw (for PgUp/PgDn).
    pub page_rows: usize,
    /// Panelize: show this list of files (from Find or an external command) instead of the folder.
    pub panelized: Option<Vec<PathBuf>>,
    /// Tree view: folders the person opened.
    pub tree_open: std::collections::HashSet<PathBuf>,
    pub tree_scroll: usize,
    /// The last local folder shown here: where "back to local" (".." at a server root) leads.
    pub last_local: PathBuf,
    /// `dirsize::version()` the entries were last synced with.
    pub sizes_ver: u64,
}

impl Panel {
    pub fn new(dir: PathBuf) -> Self {
        let mut p = Panel {
            dir: dir.clone(),
            entries: Vec::new(),
            cursor: 0,
            top: 0,
            sort: SortOptions::default(),
            listing: Listing::Full,
            view: PanelView::Listing,
            show_hidden: false,
            filter: None,
            history: vec![dir.clone()],
            history_pos: 0,
            error: None,
            quick_search: None,
            page_rows: 20,
            panelized: None,
            tree_open: std::collections::HashSet::new(),
            tree_scroll: 0,
            last_local: if crate::vfs::is_remote(&dir) { fsutil::home_dir() } else { dir.clone() },
            sizes_ver: 0,
        };
        p.reload_keep(None);
        p
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    /// Re-read the folder, keeping marks and the cursor on the same name if possible.
    pub fn reload(&mut self) {
        crate::vfs::invalidate(&self.dir);
        crate::dirsize::invalidate();
        let name = self.current().map(|e| e.name.clone());
        self.reload_keep(name);
    }

    /// Fill folder sizes from the background calculation (PNL-14); `idx` identifies the panel.
    /// Returns true while results are still outstanding.
    pub fn sync_dir_sizes(&mut self, idx: usize, enabled: bool) -> bool {
        let local = !crate::vfs::is_remote(&self.dir) && self.panelized.is_none();
        if !enabled || !local {
            self.sizes_ver = 0;
            if self.entries.iter().any(|e| e.dir_size_auto) {
                for e in self.entries.iter_mut().filter(|e| e.dir_size_auto) {
                    e.dir_size = None;
                    e.dir_size_auto = false;
                }
            }
            return false;
        }
        let v = crate::dirsize::version();
        if v == self.sizes_ver {
            return false;
        }
        let want: Vec<(usize, (PathBuf, i64))> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == fsutil::Kind::Dir && !e.is_updir() && (e.dir_size.is_none() || e.dir_size_auto))
            .map(|(i, e)| (i, (e.path.clone(), e.mtime)))
            .collect();
        let keys: Vec<(PathBuf, i64)> = want.iter().map(|(_, k)| k.clone()).collect();
        let res = crate::dirsize::lookup(idx, &keys);
        let mut changed = false;
        for ((i, _), r) in want.iter().zip(res) {
            if let Some(s) = r {
                let e = &mut self.entries[*i];
                changed |= e.dir_size != Some(s);
                e.dir_size = Some(s);
                e.dir_size_auto = true;
            }
        }
        self.sizes_ver = v;
        if changed && self.sort.mode == SortMode::Size {
            self.resort();
        }
        crate::dirsize::busy()
    }

    pub fn reload_keep(&mut self, select: Option<String>) {
        let marked: std::collections::HashSet<String> =
            self.entries.iter().filter(|e| e.marked).map(|e| e.name.clone()).collect();
        let sizes: std::collections::HashMap<String, u64> =
            self.entries.iter().filter(|e| !e.dir_size_auto).filter_map(|e| e.dir_size.map(|s| (e.name.clone(), s))).collect();
        let old_cursor = self.cursor;
        let listed = match &self.panelized {
            Some(paths) => {
                let mut v = vec![fsutil::updir_entry(&self.dir)];
                for p in paths {
                    if let Some(mut e) = fsutil::read_entry(p) {
                        e.name = p.strip_prefix(&self.dir).map(|r| r.display().to_string()).unwrap_or_else(|_| p.display().to_string());
                        v.push(e);
                    }
                }
                Ok(v)
            }
            None => fsutil::list_dir(&self.dir, self.show_hidden),
        };
        match listed {
            Ok(mut list) => {
                self.error = None;
                if let Some(f) = &self.filter {
                    list.retain(|e| e.is_dir_like() || fsutil::wildcard_match(f, &e.name, false));
                }
                for e in list.iter_mut() {
                    e.marked = marked.contains(&e.name);
                    e.dir_size = sizes.get(&e.name).copied();
                }
                self.entries = list;
                self.sizes_ver = 0;
            }
            Err(err) => {
                self.error = Some(format!("Cannot read directory contents: {}", err));
                self.entries = vec![fsutil::updir_entry(&self.dir)];
            }
        }
        self.sort_entries();
        self.cursor = match select {
            Some(n) => self.entries.iter().position(|e| e.name == n).unwrap_or(old_cursor.min(self.entries.len().saturating_sub(1))),
            None => 0,
        };
        self.clamp();
    }

    fn rank(&self, e: &Entry) -> u8 {
        if e.is_updir() {
            return 0;
        }
        if self.sort.mix_all {
            return 3;
        }
        if e.is_dir_like() {
            1
        } else if self.sort.exec_first && e.is_executable() {
            2
        } else {
            3
        }
    }

    fn name_cmp(&self, a: &Entry, b: &Entry) -> Ordering {
        if self.sort.case_sensitive {
            a.name.cmp(&b.name)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name))
        }
    }

    /// mc rules (`_sort_name` and siblings): `..` first, directories before files,
    /// then the chosen key; Reverse flips the key but not the grouping.
    pub fn sort_entries(&mut self) {
        let mut v = std::mem::take(&mut self.entries);
        let mode = self.sort.mode;
        let rev = self.sort.reverse;
        v.sort_by(|a, b| {
            let r = self.rank(a).cmp(&self.rank(b));
            if r != Ordering::Equal {
                return r;
            }
            if mode == SortMode::Unsorted {
                return Ordering::Equal;
            }
            let key = match mode {
                SortMode::Unsorted => Ordering::Equal,
                SortMode::Name => self.name_cmp(a, b),
                SortMode::Version => fsutil::version_cmp(&a.name, &b.name),
                SortMode::Extension => {
                    let (ea, eb) = (a.extension(), b.extension());
                    let o = if self.sort.case_sensitive { ea.cmp(eb) } else { ea.to_lowercase().cmp(&eb.to_lowercase()) };
                    o.then_with(|| self.name_cmp(a, b))
                }
                SortMode::Size => a.shown_size().cmp(&b.shown_size()).then_with(|| self.name_cmp(a, b)),
                SortMode::MTime => a.mtime.cmp(&b.mtime).then_with(|| self.name_cmp(a, b)),
                SortMode::ATime => a.atime.cmp(&b.atime).then_with(|| self.name_cmp(a, b)),
                SortMode::CTime => a.ctime.cmp(&b.ctime).then_with(|| self.name_cmp(a, b)),
                SortMode::Inode => a.ino.cmp(&b.ino),
            };
            if rev {
                key.reverse()
            } else {
                key
            }
        });
        self.entries = v;
    }

    pub fn resort(&mut self) {
        let name = self.current().map(|e| e.name.clone());
        self.sort_entries();
        if let Some(n) = name {
            if let Some(i) = self.entries.iter().position(|e| e.name == n) {
                self.cursor = i;
            }
        }
        self.clamp();
    }

    pub fn clamp(&mut self) {
        if self.entries.is_empty() {
            self.cursor = 0;
            self.top = 0;
            return;
        }
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len() - 1;
        }
    }

    /// Change folder. `record` adds it to the directory history.
    pub fn chdir(&mut self, dir: &Path, record: bool) -> Result<(), String> {
        let dir = if crate::vfs::is_remote(dir) {
            // Remote: the listing itself proves the folder exists and is readable.
            if let Err(e) = crate::vfs::list(dir) {
                return Err(format!("Cannot change directory to \"{}\"\n{}", crate::vfs::display(dir), e));
            }
            dir.to_path_buf()
        } else {
            let dir = std::fs::canonicalize(dir).map_err(|e| format!("Cannot change directory to \"{}\"\n{}", dir.display(), e))?;
            if !dir.is_dir() {
                return Err(format!("\"{}\" is not a directory", dir.display()));
            }
            if let Err(e) = std::fs::read_dir(&dir) {
                return Err(format!("Cannot change directory to \"{}\"\n{}", dir.display(), e));
            }
            dir
        };
        let old = self.dir.clone();
        if !crate::vfs::is_remote(&dir) {
            self.last_local = dir.clone();
        }
        self.dir = dir.clone();
        self.filter = None;
        self.panelized = None;
        self.entries.clear();
        self.top = 0;
        // Coming up from a child folder puts the cursor on that child (mc behaviour).
        let select = if crate::vfs::parent(&old).as_deref() == Some(dir.as_path()) {
            Some(crate::vfs::base_name(&old))
        } else {
            None
        };
        self.reload_keep(select);
        if record {
            self.history.truncate(self.history_pos + 1);
            if self.history.last() != Some(&dir) {
                self.history.push(dir);
            }
            if self.history.len() > 60 {
                self.history.remove(0);
            }
            self.history_pos = self.history.len() - 1;
        }
        Ok(())
    }

    pub fn history_back(&mut self) -> Result<(), String> {
        if self.history_pos > 0 {
            self.history_pos -= 1;
            let d = self.history[self.history_pos].clone();
            self.chdir(&d, false)
        } else {
            Ok(())
        }
    }

    pub fn history_forward(&mut self) -> Result<(), String> {
        if self.history_pos + 1 < self.history.len() {
            self.history_pos += 1;
            let d = self.history[self.history_pos].clone();
            self.chdir(&d, false)
        } else {
            Ok(())
        }
    }

    pub fn go_parent(&mut self) -> Result<(), String> {
        if self.panelized.is_some() {
            // Leaving a panelized list returns to the folder itself.
            self.panelized = None;
            self.reload_keep(None);
            return Ok(());
        }
        if let Some(p) = crate::vfs::parent(&self.dir) {
            self.chdir(&p, true)
        } else if crate::vfs::is_remote(&self.dir) {
            // ".." at the root of a server leads back to the local folder we came from.
            let back = if self.last_local.is_dir() { self.last_local.clone() } else { fsutil::home_dir() };
            self.chdir(&back, true)
        } else {
            Ok(())
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let n = self.entries.len() as isize;
        let c = (self.cursor as isize + delta).clamp(0, n - 1);
        self.cursor = c as usize;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
    }

    /// mc `do_file_mark`: `..` can't be marked.
    pub fn set_mark(&mut self, idx: usize, on: bool) {
        if let Some(e) = self.entries.get_mut(idx) {
            if e.kind != Kind::UpDir {
                e.marked = on;
            }
        }
    }

    pub fn toggle_mark_current(&mut self) {
        let i = self.cursor;
        if let Some(on) = self.entries.get(i).map(|e| !e.marked) {
            self.set_mark(i, on);
        }
    }

    #[allow(dead_code)]
    pub fn marked_count(&self) -> usize {
        self.entries.iter().filter(|e| e.marked).count()
    }

    /// (count, total bytes) of the marked entries, as in "%s in %d files".
    pub fn marked_totals(&self) -> (usize, u64) {
        self.entries.iter().filter(|e| e.marked).fold((0, 0), |(c, s), e| {
            let add = if e.is_dir_like() { e.dir_size.unwrap_or(0) } else { e.size };
            (c + 1, s + add)
        })
    }

    /// Entries an operation applies to: the marked ones, or the one under the cursor.
    pub fn selection(&self) -> Vec<Entry> {
        let m: Vec<Entry> = self.entries.iter().filter(|e| e.marked).cloned().collect();
        if !m.is_empty() {
            return m;
        }
        match self.current() {
            Some(e) if !e.is_updir() => vec![e.clone()],
            _ => vec![],
        }
    }

    pub fn select_group(&mut self, pattern: &str, on: bool, files_only: bool, case_sensitive: bool) {
        for e in self.entries.iter_mut() {
            if e.kind == Kind::UpDir {
                continue;
            }
            if files_only && e.is_dir_like() {
                continue;
            }
            if fsutil::wildcard_match(pattern, &e.name, case_sensitive) {
                e.marked = on;
            }
        }
    }

    pub fn invert_selection(&mut self) {
        for e in self.entries.iter_mut() {
            if e.kind != Kind::UpDir && !e.is_dir_like() {
                e.marked = !e.marked;
            }
        }
    }

    pub fn unmark_all(&mut self) {
        for e in self.entries.iter_mut() {
            e.marked = false;
        }
    }

    /// Quick search: jump to the first name starting with the typed text.
    pub fn quick_search_jump(&mut self, from_next: bool) -> bool {
        let Some(q) = self.quick_search.clone() else { return false };
        if q.is_empty() {
            return true;
        }
        let q = q.to_lowercase();
        let n = self.entries.len();
        let start = if from_next { self.cursor + 1 } else { self.cursor };
        for k in 0..n {
            let i = (start + k) % n;
            if self.entries[i].name.to_lowercase().starts_with(&q) {
                self.cursor = i;
                return true;
            }
        }
        false
    }

    pub fn select_name(&mut self, name: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.cursor = i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ".." at the root of a server leads back to the local folder the panel came from.
    #[test]
    fn back_to_local_from_server_root() {
        let server = "/usr/libexec/sftp-server";
        if !Path::new(server).exists() {
            return;
        }
        std::env::set_var("MYCMD_SFTP_SERVER", server);
        let local = std::env::temp_dir();
        let mut p = Panel::new(local.clone());
        let root = crate::vfs::build(crate::vfs::Scheme::Sftp, "local", "/");
        p.chdir(&root, true).unwrap();
        assert!(crate::vfs::is_remote(&p.dir));
        assert!(p.entries.first().map(|e| e.is_updir()).unwrap_or(false), "server root must offer ..");
        p.go_parent().unwrap();
        assert!(!crate::vfs::is_remote(&p.dir));
        assert_eq!(std::fs::canonicalize(&p.dir).unwrap(), std::fs::canonicalize(&local).unwrap());
    }
}
