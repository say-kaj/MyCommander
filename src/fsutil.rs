//! File system helpers: directory entries, formatting of sizes, dates, permissions,
//! owner names, free space and the file-type highlight classes.

use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    UpDir,
    Dir,
    File,
    Symlink,
    StaleLink,
    Device,
    Special,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// True when the entry is a symlink pointing at a directory.
    pub link_to_dir: bool,
    pub size: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub ino: u64,
    pub rdev: u64,
    pub mtime: i64,
    pub atime: i64,
    pub ctime: i64,
    pub marked: bool,
    /// Size computed with Ctrl-Space for directories.
    pub dir_size: Option<u64>,
    /// `dir_size` came from the background calculation (PNL-14), not from Ctrl-Space.
    pub dir_size_auto: bool,
}

impl Entry {
    pub fn is_dir_like(&self) -> bool {
        matches!(self.kind, Kind::Dir | Kind::UpDir) || self.link_to_dir
    }
    pub fn is_updir(&self) -> bool {
        self.kind == Kind::UpDir
    }
    pub fn is_executable(&self) -> bool {
        !self.is_dir_like() && self.mode & 0o111 != 0
    }
    pub fn extension(&self) -> &str {
        if self.is_dir_like() {
            return "";
        }
        match self.name.rfind('.') {
            Some(0) | None => "",
            Some(i) => &self.name[i + 1..],
        }
    }
    /// Size used for marking totals and the size column.
    pub fn shown_size(&self) -> u64 {
        self.dir_size.unwrap_or(self.size)
    }
}

pub fn updir_entry(dir: &Path) -> Entry {
    let parent = crate::vfs::parent(dir).unwrap_or_else(|| dir.to_path_buf());
    if crate::vfs::is_remote(dir) && crate::vfs::parent(dir).is_none() {
        let mut e = crate::vfs::root_entry(&home_dir());
        e.kind = Kind::UpDir;
        return e;
    }
    if crate::vfs::is_remote(dir) {
        let mut e = crate::vfs::root_entry(&parent);
        e.kind = Kind::UpDir;
        return e;
    }
    let md = fs::metadata(&parent).ok();
    let mut e = entry_from_meta("..".into(), parent, md.as_ref(), Kind::UpDir);
    e.kind = Kind::UpDir;
    e
}

fn entry_from_meta(name: String, path: PathBuf, md: Option<&fs::Metadata>, kind: Kind) -> Entry {
    Entry {
        name,
        path,
        kind,
        link_to_dir: false,
        size: md.map(|m| m.len()).unwrap_or(0),
        mode: md.map(|m| m.mode()).unwrap_or(0),
        nlink: md.map(|m| m.nlink()).unwrap_or(0),
        uid: md.map(|m| m.uid()).unwrap_or(0),
        gid: md.map(|m| m.gid()).unwrap_or(0),
        ino: md.map(|m| m.ino()).unwrap_or(0),
        rdev: md.map(|m| m.rdev()).unwrap_or(0),
        mtime: md.map(|m| m.mtime()).unwrap_or(0),
        atime: md.map(|m| m.atime()).unwrap_or(0),
        ctime: md.map(|m| m.ctime()).unwrap_or(0),
        marked: false,
        dir_size: None,
        dir_size_auto: false,
    }
}

pub fn read_entry(path: &Path) -> Option<Entry> {
    if crate::vfs::is_remote(path) {
        return crate::vfs::entry_of(path);
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    let lmd = fs::symlink_metadata(path).ok()?;
    let ft = lmd.file_type();
    if ft.is_symlink() {
        match fs::metadata(path) {
            Ok(target) => {
                let mut e = entry_from_meta(name, path.to_path_buf(), Some(&target), Kind::Symlink);
                e.link_to_dir = target.is_dir();
                Some(e)
            }
            Err(_) => Some(entry_from_meta(name, path.to_path_buf(), Some(&lmd), Kind::StaleLink)),
        }
    } else {
        use std::os::unix::fs::FileTypeExt;
        let kind = if ft.is_dir() {
            Kind::Dir
        } else if ft.is_block_device() || ft.is_char_device() {
            Kind::Device
        } else if ft.is_fifo() || ft.is_socket() {
            Kind::Special
        } else {
            Kind::File
        };
        Some(entry_from_meta(name, path.to_path_buf(), Some(&lmd), kind))
    }
}

pub fn list_dir(dir: &Path, show_hidden: bool) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    // A server root also gets "..": it leads back to the local file system.
    if crate::vfs::parent(dir).is_some() || crate::vfs::is_remote(dir) {
        out.push(updir_entry(dir));
    }
    if crate::vfs::is_remote(dir) {
        out.extend(crate::vfs::list(dir)?.into_iter().filter(|e| show_hidden || !e.name.starts_with('.')));
        return Ok(out);
    }
    for de in fs::read_dir(dir)? {
        let Ok(de) = de else { continue };
        let name = de.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if let Some(e) = read_entry(&de.path()) {
            out.push(e);
        }
    }
    Ok(out)
}

/// mc `size_trunc_len`: print the number as is if it fits `width`, otherwise divide by
/// 1024 (rounded) and add a unit letter until it fits.
pub fn size_fit(mut size: u64, width: usize) -> String {
    let units = ["", "K", "M", "G", "T", "P", "E"];
    let mut i = 0;
    loop {
        let s = format!("{}{}", size, units[i]);
        if s.chars().count() <= width || i + 1 == units.len() {
            return s;
        }
        size = (size + 512) >> 10;
        i += 1;
    }
}

/// Human readable size for totals ("1.4 MB" style, used in dialogs and the footer).
pub fn size_human(size: u64) -> String {
    let units = ["B", "K", "M", "G", "T", "P"];
    let mut v = size as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < units.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", size)
    } else if v < 10.0 {
        format!("{:.1} {}", v, units[i])
    } else {
        format!("{:.0} {}", v, units[i])
    }
}

/// Number with thousands separators.
pub fn size_sep(size: u64) -> String {
    let s = size.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub const OLD_FILE_AGE: i64 = 15_552_000; // 180 days, from _file_date
pub const FUTURE_TOLERANCE: i64 = 3_600; // 1 hour, from _file_date

/// mc `file_date`: recent files show the time, old (or far-future) files the year.
pub fn file_date(t: i64) -> String {
    use chrono::{Local, TimeZone};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let Some(dt) = Local.timestamp_opt(t, 0).single() else {
        return "(invalid)".into();
    };
    let old = now - t > OLD_FILE_AGE || t - now > FUTURE_TOLERANCE;
    if old {
        dt.format("%b %e  %Y").to_string()
    } else {
        dt.format("%b %e %H:%M").to_string()
    }
}

pub fn full_date(t: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(t, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

pub fn perm_string(e: &Entry) -> String {
    let t = match e.kind {
        Kind::Dir | Kind::UpDir => 'd',
        Kind::Symlink | Kind::StaleLink => 'l',
        Kind::Device => {
            if e.mode & 0o170000 == 0o060000 {
                'b'
            } else {
                'c'
            }
        }
        Kind::Special => {
            if e.mode & 0o170000 == 0o140000 {
                's'
            } else {
                'p'
            }
        }
        Kind::File => '-',
    };
    let m = e.mode;
    let mut s = String::with_capacity(10);
    s.push(t);
    let bits = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (i, (b, c)) in bits.iter().enumerate() {
        let mut ch = if m & b != 0 { *c } else { '-' };
        if i == 2 && m & 0o4000 != 0 {
            ch = if ch == 'x' { 's' } else { 'S' };
        }
        if i == 5 && m & 0o2000 != 0 {
            ch = if ch == 'x' { 's' } else { 'S' };
        }
        if i == 8 && m & 0o1000 != 0 {
            ch = if ch == 'x' { 't' } else { 'T' };
        }
        s.push(ch);
    }
    s
}

pub fn user_name(uid: u32) -> String {
    unsafe {
        let pw = libc::getpwuid(uid);
        if pw.is_null() || (*pw).pw_name.is_null() {
            return uid.to_string();
        }
        CStr::from_ptr((*pw).pw_name).to_string_lossy().to_string()
    }
}

pub fn group_name(gid: u32) -> String {
    unsafe {
        let gr = libc::getgrgid(gid);
        if gr.is_null() || (*gr).gr_name.is_null() {
            return gid.to_string();
        }
        CStr::from_ptr((*gr).gr_name).to_string_lossy().to_string()
    }
}

/// (free bytes, total bytes) of the volume holding `path`.
pub fn free_space(path: &Path) -> Option<(u64, u64)> {
    if crate::vfs::is_remote(path) {
        return None;
    }
    let c = CString::new(path.as_os_str().to_string_lossy().as_bytes()).ok()?;
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        let bs = st.f_frsize as u64;
        Some((st.f_bavail as u64 * bs, st.f_blocks as u64 * bs))
    }
}

pub fn read_link(path: &Path) -> Option<String> {
    fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
}

pub fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// File highlight groups from filehighlight.ini.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlight {
    Normal,
    Directory,
    Executable,
    Symlink,
    StaleLink,
    Device,
    Special,
    Core,
    Temp,
    Archive,
    Doc,
    Source,
    Media,
    Graph,
    Database,
}

const ARCHIVE: &str = "7z;Z;ace;apk;arc;arj;ark;bz2;cab;cpio;deb;gz;lha;lz;lz4;lzh;lzma;lzo;rar;rpm;tar;tbz;tbz2;tgz;tlz;txz;tzo;tzst;vsix;xz;zip;zoo;zst;dmg;pkg";
const DOC: &str = "chm;css;ctl;diz;doc;docm;docx;dtd;fodg;fodp;fods;fodt;htm;html;json;letter;lsm;mail;man;markdown;md;me;mkd;msg;nroff;odg;odp;ods;odt;pdf;po;ppt;pptm;pptx;ps;rtf;sgml;shtml;tex;text;txt;xls;xlsm;xlsx;xml;xsd;xslt;pages;numbers;key";
const SOURCE: &str = "ada;asm;awk;bash;c;c++;caml;cc;cgi;cpp;cxx;diff;erl;go;h;h++;hh;hi;hpp;hs;inc;jasm;jav;java;js;m4;mak;mjs;ml;mli;mll;mlp;mly;pas;patch;php;phps;pl;pm;prg;py;rb;s;sas;sh;sl;st;swift;tcl;tk;tsx;xq;rs;ts;toml;m;mm;kt;zsh";
const MEDIA: &str = "3gp;aac;ac3;ape;asf;avi;awb;dts;flac;flv;it;m3u;m4a;m4v;med;mid;midi;mkv;mod;mol;mov;mp2;mp3;mp4;mpeg;mpg;mpl;ogg;ogv;opus;s3m;ts;umx;vob;wav;webm;wma;wmv;xm;y4m";
const GRAPH: &str = "ai;avif;bmp;cdr;eps;gif;heic;heif;ico;jp2;jpeg;jpg;jxl;omf;pcx;pic;png;psb;psd;rle;svg;tga;tif;tiff;webp;wmf;xbm;xcf;xpm;icns";
const DATABASE: &str = "cdx;dat;db;dbf;dbi;dbx;fox;mdb;mdn;mdx;msql;mssql;pgsql;sql;ssql;sqlite";
const TEMP: &str = "~;$$$;bak;part;tmp";

fn in_list(list: &str, ext: &str) -> bool {
    !ext.is_empty() && list.split(';').any(|x| x.eq_ignore_ascii_case(ext))
}

pub fn highlight(e: &Entry) -> Highlight {
    match e.kind {
        Kind::UpDir | Kind::Dir => return Highlight::Directory,
        Kind::StaleLink => return Highlight::StaleLink,
        Kind::Device => return Highlight::Device,
        Kind::Special => return Highlight::Special,
        Kind::Symlink => {
            if e.link_to_dir {
                return Highlight::Directory;
            }
            return Highlight::Symlink;
        }
        Kind::File => {}
    }
    if e.mode & 0o111 != 0 {
        return Highlight::Executable;
    }
    let n = &e.name;
    if n.starts_with("core") && n[4..].trim_start_matches('.').chars().all(|c| c.is_ascii_digit()) && (n.len() == 4 || n.as_bytes()[4] == b'.') {
        return Highlight::Core;
    }
    let ext = e.extension();
    if n.starts_with('#') || n.ends_with('~') || in_list(TEMP, ext) {
        return Highlight::Temp;
    }
    if in_list(ARCHIVE, ext) {
        Highlight::Archive
    } else if in_list(DOC, ext) {
        Highlight::Doc
    } else if in_list(SOURCE, ext) {
        Highlight::Source
    } else if in_list(MEDIA, ext) {
        Highlight::Media
    } else if in_list(GRAPH, ext) {
        Highlight::Graph
    } else if in_list(DATABASE, ext) {
        Highlight::Database
    } else {
        Highlight::Normal
    }
}

/// Shell-style wildcard match (`*`, `?`, `[abc]`) used by select group and copy masks.
pub fn wildcard_match(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = if case_sensitive {
        (pattern.chars().collect(), text.chars().collect())
    } else {
        (pattern.to_lowercase().chars().collect(), text.to_lowercase().chars().collect())
    };
    fn m(p: &[char], t: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        match p[0] {
            '*' => (0..=t.len()).any(|i| m(&p[1..], &t[i..])),
            '?' => !t.is_empty() && m(&p[1..], &t[1..]),
            '[' => {
                if t.is_empty() {
                    return false;
                }
                if let Some(end) = p.iter().position(|&c| c == ']') {
                    let set = &p[1..end];
                    let (neg, set) = if set.first() == Some(&'!') || set.first() == Some(&'^') {
                        (true, &set[1..])
                    } else {
                        (false, set)
                    };
                    let mut hit = false;
                    let mut i = 0;
                    while i < set.len() {
                        if i + 2 < set.len() && set[i + 1] == '-' {
                            if set[i] <= t[0] && t[0] <= set[i + 2] {
                                hit = true;
                            }
                            i += 3;
                        } else {
                            if set[i] == t[0] {
                                hit = true;
                            }
                            i += 1;
                        }
                    }
                    hit != neg && m(&p[end + 1..], &t[1..])
                } else {
                    t[0] == '[' && m(&p[1..], &t[1..])
                }
            }
            c => !t.is_empty() && t[0] == c && m(&p[1..], &t[1..]),
        }
    }
    m(&p, &t)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Path shown with `~` for the home folder.
pub fn tilde(path: &Path) -> String {
    let home = home_dir();
    if let Ok(rest) = path.strip_prefix(&home) {
        if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        }
    } else {
        path.display().to_string()
    }
}

/// Compare version-aware ("file2" < "file10"), used by the Version sort.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < ab.len() && j < bb.len() {
        if ab[i].is_ascii_digit() && bb[j].is_ascii_digit() {
            let si = i;
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            let na = a[si..i].trim_start_matches('0');
            let nb = b[sj..j].trim_start_matches('0');
            let o = na.len().cmp(&nb.len()).then(na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
        } else {
            let o = ab[i].cmp(&bb[j]);
            if o != Ordering::Equal {
                return o;
            }
            i += 1;
            j += 1;
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j))
}

/// Is this file really a program (Mach-O binary or `#!` script)? A document with an execute bit
/// (common on network shares) is not, and opens with its app (PNL-17).
pub fn is_program(path: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    let Ok(mut f) = fs::File::open(path) else { return false };
    if f.read_exact(&mut b).is_err() {
        return false;
    }
    matches!(
        u32::from_be_bytes(b),
        0xfeedface | 0xfeedfacf | 0xcafebabe | 0xcffaedfe | 0xcefaedfe | 0xbebafeca
    ) || &b[..2] == b"#!"
}

static TRASH_RESULT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Move files to the Trash through Finder (Put Back works), in the background.
pub fn trash_async(paths: Vec<PathBuf>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let list: Vec<String> = paths.iter().map(|p| format!("POSIX file \"{}\"", esc(&p.to_string_lossy()))).collect();
        let script = format!("tell application \"Finder\" to delete {{{}}}", list.join(", "));
        let out = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output();
        let msg = match out {
            Ok(o) if o.status.success() => format!("Moved {} item{} to the Trash", paths.len(), if paths.len() == 1 { "" } else { "s" }),
            Ok(o) => format!("Cannot move to the Trash: {}", String::from_utf8_lossy(&o.stderr).trim()),
            Err(e) => format!("Cannot move to the Trash: {e}"),
        };
        *TRASH_RESULT.lock().unwrap() = Some(msg);
        ctx.request_repaint();
    });
}

pub fn take_trash_result() -> Option<String> {
    TRASH_RESULT.lock().unwrap().take()
}

#[cfg(test)]
mod program_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn documents_with_an_execute_bit_are_not_programs() {
        let d = std::env::temp_dir().join(format!("mycmd-prog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let p = d.join(name);
            fs::write(&p, bytes).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        assert!(!is_program(&write("photo.jpg", &[0xff, 0xd8, 0xff, 0xe0, 0, 0x10])));
        assert!(!is_program(&write("notes.txt", b"hello world")));
        assert!(!is_program(&write("tiny", b"a")));
        assert!(is_program(&write("run.sh", b"#!/bin/sh\necho hi\n")));
        assert!(is_program(&write("macho", &[0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0])));
        let _ = fs::remove_dir_all(&d);
    }
}
