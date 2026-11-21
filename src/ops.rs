//! Background jobs: copy, move, delete, directory size, find file and shell commands.
//! The window polls a shared `Progress`; questions (overwrite?) are answered through it.

use crate::fsutil;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::fs::FileTimes;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Copy,
    Move,
    Delete,
}

impl OpKind {
    pub fn label(self) -> &'static str {
        match self {
            OpKind::Copy => "Copy",
            OpKind::Move => "Move",
            OpKind::Delete => "Delete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    All,
    None,
    Skip,
    Abort,
}

#[derive(Clone, Debug)]
pub struct Question {
    pub text: String,
    pub src_info: String,
    pub dst_info: String,
}

#[derive(Default)]
pub struct Progress {
    pub title: String,
    pub current: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub file_bytes_done: u64,
    pub file_bytes_total: u64,
    pub started: Option<std::time::Instant>,
    pub finished: bool,
    pub errors: Vec<String>,
    pub question: Option<Question>,
    pub answer: Option<Answer>,
    pub cancel: bool,
    /// Results of a find job.
    pub found: Vec<PathBuf>,
    /// Output of a shell command / directory size result.
    pub output: String,
    pub dir_size: Option<u64>,
    pub sizes: Vec<(PathBuf, u64)>,
    pub exit_code: Option<i32>,
}

pub type Shared = Arc<Mutex<Progress>>;

pub fn new_shared(title: &str) -> Shared {
    Arc::new(Mutex::new(Progress { title: title.into(), started: Some(std::time::Instant::now()), ..Default::default() }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Overwrite {
    Ask,
    AllYes,
    AllNo,
}

struct Worker {
    p: Shared,
    overwrite: Overwrite,
}

impl Worker {
    fn cancelled(&self) -> bool {
        self.p.lock().unwrap().cancel
    }

    fn error(&self, msg: String) {
        self.p.lock().unwrap().errors.push(msg);
    }

    fn ask(&self, q: Question) -> Answer {
        {
            let mut g = self.p.lock().unwrap();
            g.answer = None;
            g.question = Some(q);
        }
        loop {
            thread::sleep(Duration::from_millis(30));
            let mut g = self.p.lock().unwrap();
            if g.cancel {
                g.question = None;
                return Answer::Abort;
            }
            if let Some(a) = g.answer.take() {
                g.question = None;
                return a;
            }
        }
    }

    /// True if `dst` may be written.
    fn may_overwrite(&mut self, src: &Path, dst: &Path) -> Result<bool, ()> {
        if fs::symlink_metadata(dst).is_err() {
            return Ok(true);
        }
        match self.overwrite {
            Overwrite::AllYes => return Ok(true),
            Overwrite::AllNo => return Ok(false),
            Overwrite::Ask => {}
        }
        let info = |p: &Path| {
            fs::metadata(p)
                .map(|m| {
                    use std::os::unix::fs::MetadataExt;
                    format!("{} bytes, {}", fsutil::size_sep(m.len()), fsutil::full_date(m.mtime()))
                })
                .unwrap_or_default()
        };
        let a = self.ask(Question {
            text: format!("Target file already exists!\n{}", dst.display()),
            src_info: format!("New     : {}", info(src)),
            dst_info: format!("Existing: {}", info(dst)),
        });
        match a {
            Answer::Yes => Ok(true),
            Answer::No | Answer::Skip => Ok(false),
            Answer::All => {
                self.overwrite = Overwrite::AllYes;
                Ok(true)
            }
            Answer::None => {
                self.overwrite = Overwrite::AllNo;
                Ok(false)
            }
            Answer::Abort => Err(()),
        }
    }

    fn set_current(&self, path: &Path, size: u64) {
        let mut g = self.p.lock().unwrap();
        g.current = path.display().to_string();
        g.file_bytes_done = 0;
        g.file_bytes_total = size;
    }

    fn file_done(&self) {
        self.p.lock().unwrap().files_done += 1;
    }

    fn copy_file(&mut self, src: &Path, dst: &Path) -> Result<(), ()> {
        let md = match fs::metadata(src) {
            Ok(m) => m,
            Err(e) => {
                self.error(format!("Cannot stat source file \"{}\"\n{}", src.display(), e));
                return Ok(());
            }
        };
        if !self.may_overwrite(src, dst)? {
            let mut g = self.p.lock().unwrap();
            g.bytes_done += md.len();
            g.files_done += 1;
            return Ok(());
        }
        self.set_current(src, md.len());
        // APFS: a copy-on-write clone is instant and takes no space (what Finder does).
        if clone_file(src, dst) {
            let mut g = self.p.lock().unwrap();
            g.bytes_done += md.len();
            g.file_bytes_done = md.len();
            g.files_done += 1;
            return Ok(());
        }
        let mut inp = match fs::File::open(src) {
            Ok(f) => f,
            Err(e) => {
                self.error(format!("Cannot open source file \"{}\"\n{}", src.display(), e));
                return Ok(());
            }
        };
        if fs::symlink_metadata(dst).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            let _ = fs::remove_file(dst);
        }
        let mut out = match fs::File::create(dst) {
            Ok(f) => f,
            Err(e) => {
                self.error(format!("Cannot create target file \"{}\"\n{}", dst.display(), e));
                return Ok(());
            }
        };
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if self.cancelled() {
                drop(out);
                let _ = fs::remove_file(dst);
                return Err(());
            }
            let n = match inp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    self.error(format!("Cannot read source file \"{}\"\n{}", src.display(), e));
                    break;
                }
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                self.error(format!("Cannot write target file \"{}\"\n{}", dst.display(), e));
                break;
            }
            let mut g = self.p.lock().unwrap();
            g.bytes_done += n as u64;
            g.file_bytes_done += n as u64;
        }
        // Preserve attributes: permissions and times.
        let _ = out.set_permissions(md.permissions());
        if let (Ok(m), Ok(a)) = (md.modified(), md.accessed()) {
            let _ = out.set_times(FileTimes::new().set_modified(m).set_accessed(a));
        }
        self.file_done();
        Ok(())
    }

    fn copy_tree(&mut self, src: &Path, dst: &Path) -> Result<(), ()> {
        if self.cancelled() {
            return Err(());
        }
        let lmd = match fs::symlink_metadata(src) {
            Ok(m) => m,
            Err(e) => {
                self.error(format!("Cannot stat source file \"{}\"\n{}", src.display(), e));
                return Ok(());
            }
        };
        if lmd.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(src) {
                if !self.may_overwrite(src, dst)? {
                    return Ok(());
                }
                let _ = fs::remove_file(dst);
                if let Err(e) = std::os::unix::fs::symlink(&target, dst) {
                    self.error(format!("Cannot create target symlink \"{}\"\n{}", dst.display(), e));
                }
            }
            self.file_done();
            return Ok(());
        }
        if lmd.is_dir() {
            if dst.starts_with(src) {
                self.error(format!("Cannot copy/move directory \"{}\" into itself", src.display()));
                return Ok(());
            }
            if !dst.is_dir() {
                if let Err(e) = fs::create_dir(dst) {
                    self.error(format!("Cannot create target directory \"{}\"\n{}", dst.display(), e));
                    return Ok(());
                }
            }
            let mut children: Vec<PathBuf> = match fs::read_dir(src) {
                Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
                Err(e) => {
                    self.error(format!("Cannot read directory \"{}\"\n{}", src.display(), e));
                    return Ok(());
                }
            };
            children.sort();
            for c in children {
                let name = c.file_name().unwrap().to_owned();
                self.copy_tree(&c, &dst.join(name))?;
            }
            let _ = fs::set_permissions(dst, lmd.permissions());
            if let Ok(f) = fs::File::open(dst) {
                if let Ok(m) = lmd.modified() {
                    let _ = f.set_times(FileTimes::new().set_modified(m));
                }
            }
            return Ok(());
        }
        self.copy_file(src, dst)
    }

    fn delete_tree(&mut self, path: &Path) -> Result<(), ()> {
        if self.cancelled() {
            return Err(());
        }
        let lmd = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                self.error(format!("Cannot stat \"{}\"\n{}", path.display(), e));
                return Ok(());
            }
        };
        if lmd.is_dir() {
            if let Ok(rd) = fs::read_dir(path) {
                for c in rd.flatten() {
                    self.delete_tree(&c.path())?;
                }
            }
            self.set_current(path, 0);
            if let Err(e) = fs::remove_dir(path) {
                self.error(format!("Cannot remove directory \"{}\"\n{}", path.display(), e));
            }
        } else {
            self.set_current(path, lmd.len());
            if let Err(e) = fs::remove_file(path) {
                self.error(format!("Cannot remove file \"{}\"\n{}", path.display(), e));
            }
            let mut g = self.p.lock().unwrap();
            g.files_done += 1;
            g.bytes_done += lmd.len();
        }
        Ok(())
    }

    fn move_one(&mut self, src: &Path, dst: &Path) -> Result<(), ()> {
        if src == dst {
            self.error(format!("\"{}\"\nand\n\"{}\"\nare the same file", src.display(), dst.display()));
            return Ok(());
        }
        let src_is_dir = fs::symlink_metadata(src).map(|m| m.is_dir()).unwrap_or(false);
        if src_is_dir && dst.starts_with(src) {
            self.error(format!("Cannot move directory \"{}\" into itself", src.display()));
            return Ok(());
        }
        if !src_is_dir || !dst.exists() {
            if !src_is_dir && !self.may_overwrite(src, dst)? {
                return Ok(());
            }
            self.set_current(src, 0);
            match fs::rename(src, dst) {
                Ok(()) => {
                    let (f, b) = count_tree(dst);
                    let mut g = self.p.lock().unwrap();
                    g.files_done += f;
                    g.bytes_done += b;
                    return Ok(());
                }
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {}
                Err(e) => {
                    self.error(format!("Cannot move file \"{}\" to \"{}\"\n{}", src.display(), dst.display(), e));
                    return Ok(());
                }
            }
            // Other volume: copy then delete (overwrite already confirmed).
            let saved = self.overwrite;
            self.overwrite = Overwrite::AllYes;
            let r = self.copy_tree(src, dst);
            self.overwrite = saved;
            r?;
            if self.p.lock().unwrap().errors.is_empty() {
                let (done, bytes) = {
                    let g = self.p.lock().unwrap();
                    (g.files_done, g.bytes_done)
                };
                self.delete_tree(src)?;
                let mut g = self.p.lock().unwrap();
                g.files_done = done;
                g.bytes_done = bytes;
            }
            return Ok(());
        }
        // Directory onto an existing directory: merge contents.
        if let Ok(rd) = fs::read_dir(src) {
            for c in rd.flatten() {
                self.move_one(&c.path(), &dst.join(c.file_name()))?;
            }
        }
        let _ = fs::remove_dir(src);
        Ok(())
    }
}

// ---------------------------------------------------------------- remote (sftp / ftp) operations

fn any_remote(sources: &[PathBuf], dest: &Option<PathBuf>) -> bool {
    sources.iter().chain(dest.iter()).any(|p| crate::vfs::is_remote(p))
}

fn io_entry(p: &Path) -> Option<fsutil::Entry> {
    if crate::vfs::is_remote(p) {
        crate::vfs::entry_of(p)
    } else {
        fsutil::read_entry(p)
    }
}

fn io_children(p: &Path) -> std::io::Result<Vec<fsutil::Entry>> {
    if crate::vfs::is_remote(p) {
        crate::vfs::list(p)
    } else {
        Ok(fsutil::list_dir(p, true)?.into_iter().filter(|e| !e.is_updir()).collect())
    }
}

fn count_any(e: &fsutil::Entry) -> (u64, u64) {
    if e.kind == fsutil::Kind::Dir {
        let (mut f, mut b) = (0, 0);
        if let Ok(kids) = io_children(&e.path) {
            for k in kids {
                let (cf, cb) = count_any(&k);
                f += cf;
                b += cb;
            }
        }
        (f, b)
    } else {
        (1, e.size)
    }
}

impl Worker {
    fn r_may_overwrite(&mut self, src: &fsutil::Entry, dst: &Path) -> Result<bool, ()> {
        let Some(existing) = io_entry(dst) else { return Ok(true) };
        match self.overwrite {
            Overwrite::AllYes => return Ok(true),
            Overwrite::AllNo => return Ok(false),
            Overwrite::Ask => {}
        }
        let info = |e: &fsutil::Entry| format!("{} bytes, {}", fsutil::size_sep(e.size), fsutil::full_date(e.mtime));
        let a = self.ask(Question {
            text: format!("Target file already exists!\n{}", crate::vfs::display(dst)),
            src_info: format!("New     : {}", info(src)),
            dst_info: format!("Existing: {}", info(&existing)),
        });
        match a {
            Answer::Yes => Ok(true),
            Answer::No | Answer::Skip => Ok(false),
            Answer::All => {
                self.overwrite = Overwrite::AllYes;
                Ok(true)
            }
            Answer::None => {
                self.overwrite = Overwrite::AllNo;
                Ok(false)
            }
            Answer::Abort => Err(()),
        }
    }

    fn r_copy_file(&mut self, src: &fsutil::Entry, dst: &Path) -> Result<(), ()> {
        use crate::vfs;
        if self.cancelled() {
            return Err(());
        }
        if !self.r_may_overwrite(src, dst)? {
            let mut g = self.p.lock().unwrap();
            g.bytes_done += src.size;
            g.files_done += 1;
            return Ok(());
        }
        self.set_current(&src.path, src.size);
        let res = match (vfs::is_remote(&src.path), vfs::is_remote(dst)) {
            (true, false) => vfs::download(&src.path, dst),
            (false, true) => vfs::upload(&src.path, dst),
            (true, true) => {
                let tmp = std::env::temp_dir().join(format!("myCommander-xfer-{}", std::process::id()));
                vfs::download(&src.path, &tmp).and_then(|_| {
                    let r = vfs::upload(&tmp, dst);
                    let _ = fs::remove_file(&tmp);
                    r
                })
            }
            (false, false) => fs::copy(&src.path, dst).map(|_| ()),
        };
        match res {
            Ok(()) => {
                let mut g = self.p.lock().unwrap();
                g.bytes_done += src.size;
                g.files_done += 1;
            }
            Err(e) => self.error(format!("Cannot copy \"{}\" to \"{}\"\n{}", vfs::display(&src.path), vfs::display(dst), e)),
        }
        Ok(())
    }

    fn r_make_dir(&mut self, dst: &Path) -> bool {
        if crate::vfs::is_dir(dst) {
            return true;
        }
        let r = if crate::vfs::is_remote(dst) { crate::vfs::mkdir(dst) } else { fs::create_dir(dst) };
        match r {
            Ok(()) => true,
            Err(e) => {
                self.error(format!("Cannot create directory \"{}\"\n{}", crate::vfs::display(dst), e));
                false
            }
        }
    }

    fn r_copy_tree(&mut self, src: &fsutil::Entry, dst: &Path) -> Result<(), ()> {
        if self.cancelled() {
            return Err(());
        }
        if src.kind == fsutil::Kind::Dir {
            if dst.starts_with(&src.path) {
                self.error(format!("Cannot copy/move directory \"{}\" into itself", crate::vfs::display(&src.path)));
                return Ok(());
            }
            if !self.r_make_dir(dst) {
                return Ok(());
            }
            self.set_current(&src.path, 0);
            let kids = match io_children(&src.path) {
                Ok(k) => k,
                Err(e) => {
                    self.error(format!("Cannot read directory \"{}\"\n{}", crate::vfs::display(&src.path), e));
                    return Ok(());
                }
            };
            for k in kids {
                let t = crate::vfs::join(dst, &k.name);
                self.r_copy_tree(&k, &t)?;
            }
            Ok(())
        } else {
            self.r_copy_file(src, dst)
        }
    }

    fn r_delete_tree(&mut self, e: &fsutil::Entry) -> Result<(), ()> {
        use crate::vfs;
        if self.cancelled() {
            return Err(());
        }
        let remote = vfs::is_remote(&e.path);
        if e.kind == fsutil::Kind::Dir {
            match io_children(&e.path) {
                Ok(kids) => {
                    for k in kids {
                        self.r_delete_tree(&k)?;
                    }
                }
                Err(err) => self.error(format!("Cannot read directory \"{}\"\n{}", vfs::display(&e.path), err)),
            }
            self.set_current(&e.path, 0);
            let r = if remote { vfs::remove(&e.path, true) } else { fs::remove_dir(&e.path) };
            if let Err(err) = r {
                self.error(format!("Cannot remove directory \"{}\"\n{}", vfs::display(&e.path), err));
            }
        } else {
            self.set_current(&e.path, e.size);
            let r = if remote { vfs::remove(&e.path, false) } else { fs::remove_file(&e.path) };
            if let Err(err) = r {
                self.error(format!("Cannot remove file \"{}\"\n{}", vfs::display(&e.path), err));
            }
            let mut g = self.p.lock().unwrap();
            g.files_done += 1;
            g.bytes_done += e.size;
        }
        Ok(())
    }

    fn r_move_one(&mut self, src: &fsutil::Entry, dst: &Path) -> Result<(), ()> {
        use crate::vfs;
        if src.path == dst {
            self.error(format!("\"{}\"\nand\n\"{}\"\nare the same file", vfs::display(&src.path), vfs::display(dst)));
            return Ok(());
        }
        // Same remote host and a free target name: a server-side rename is enough.
        if vfs::same_host(&src.path, dst) && !vfs::exists(dst) {
            self.set_current(&src.path, 0);
            match vfs::rename(&src.path, dst) {
                Ok(()) => {
                    let (f, b) = count_any(src);
                    let mut g = self.p.lock().unwrap();
                    g.files_done += f;
                    g.bytes_done += b;
                }
                Err(e) => self.error(format!("Cannot move \"{}\"\n{}", vfs::display(&src.path), e)),
            }
            return Ok(());
        }
        let before = self.p.lock().unwrap().errors.len();
        let saved = self.overwrite;
        self.r_copy_tree(src, dst)?;
        self.overwrite = saved;
        if self.p.lock().unwrap().errors.len() == before {
            let (done, bytes) = {
                let g = self.p.lock().unwrap();
                (g.files_done, g.bytes_done)
            };
            self.r_delete_tree(src)?;
            let mut g = self.p.lock().unwrap();
            g.files_done = done;
            g.bytes_done = bytes;
        }
        Ok(())
    }
}

fn start_remote_op(kind: OpKind, sources: Vec<PathBuf>, dest: Option<PathBuf>, ask_overwrite: bool) -> Shared {
    let p = new_shared(kind.label());
    let shared = p.clone();
    thread::spawn(move || {
        let mut w = Worker { p: shared.clone(), overwrite: if ask_overwrite { Overwrite::Ask } else { Overwrite::AllYes } };
        let entries: Vec<fsutil::Entry> = sources.iter().filter_map(|s| io_entry(s)).collect();
        if entries.len() != sources.len() {
            w.error("Cannot read one of the source files".into());
        }
        let (mut files, mut bytes) = (0, 0);
        for e in &entries {
            let (f, b) = count_any(e);
            files += f;
            bytes += b;
        }
        {
            let mut g = shared.lock().unwrap();
            g.files_total = files;
            g.bytes_total = bytes;
        }
        let many = entries.len() > 1;
        for e in &entries {
            let target = dest.as_ref().map(|d| if many || crate::vfs::is_dir(d) { crate::vfs::join(d, &e.name) } else { d.clone() });
            let r = match (kind, target) {
                (OpKind::Copy, Some(t)) => {
                    if t == e.path {
                        w.error(format!("\"{}\"\nand\n\"{}\"\nare the same file", crate::vfs::display(&e.path), crate::vfs::display(&t)));
                        Ok(())
                    } else {
                        w.r_copy_tree(e, &t)
                    }
                }
                (OpKind::Move, Some(t)) => w.r_move_one(e, &t),
                (OpKind::Delete, _) => w.r_delete_tree(e),
                _ => Ok(()),
            };
            if r.is_err() {
                break;
            }
        }
        shared.lock().unwrap().finished = true;
    });
    p
}

/// Clone `src` to `dst` (same APFS volume only; `dst` is replaced). False = not possible.
#[cfg(target_os = "macos")]
fn clone_file(src: &Path, dst: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let (Ok(s), Ok(d)) = (CString::new(src.as_os_str().as_bytes()), CString::new(dst.as_os_str().as_bytes())) else { return false };
    if fs::symlink_metadata(dst).is_ok() {
        // clonefile refuses to overwrite; remove only after we know cloning can work.
        let probe = dst.with_extension(format!("mycmd-clone-{}", std::process::id()));
        let Ok(p) = CString::new(probe.as_os_str().as_bytes()) else { return false };
        if unsafe { libc::clonefile(s.as_ptr(), p.as_ptr(), 0) } != 0 {
            return false;
        }
        return fs::rename(&probe, dst).is_ok();
    }
    unsafe { libc::clonefile(s.as_ptr(), d.as_ptr(), 0) == 0 }
}

#[cfg(not(target_os = "macos"))]
fn clone_file(_: &Path, _: &Path) -> bool {
    false
}

/// (files, bytes) below a path, not following symlinks (fast bulk enumeration).
pub fn count_tree(path: &Path) -> (u64, u64) {
    crate::dirsize::count(path)
}

/// Where a source ends up: inside `dest` when that is a folder (or several sources),
/// otherwise `dest` itself is the new name.
pub fn target_for(src: &Path, dest: &Path, many: bool) -> PathBuf {
    if dest.is_dir() || many {
        dest.join(src.file_name().unwrap_or_default())
    } else {
        dest.to_path_buf()
    }
}

#[allow(dead_code)]
pub fn start_file_op(kind: OpKind, sources: Vec<PathBuf>, dest: Option<PathBuf>) -> Shared {
    start_file_op_with(kind, sources, dest, true)
}

/// `ask_overwrite` false = Settings > General "overwrite" off: replace without asking.
pub fn start_file_op_with(kind: OpKind, sources: Vec<PathBuf>, dest: Option<PathBuf>, ask_overwrite: bool) -> Shared {
    if any_remote(&sources, &dest) {
        return start_remote_op(kind, sources, dest, ask_overwrite);
    }
    let p = new_shared(kind.label());
    let shared = p.clone();
    thread::spawn(move || {
        let (mut files, mut bytes) = (0, 0);
        for s in &sources {
            let (f, b) = count_tree(s);
            files += f;
            bytes += b;
        }
        {
            let mut g = shared.lock().unwrap();
            g.files_total = files;
            g.bytes_total = bytes;
        }
        let mut w = Worker { p: shared.clone(), overwrite: if ask_overwrite { Overwrite::Ask } else { Overwrite::AllYes } };
        let many = sources.len() > 1;
        if let Some(d) = &dest {
            if many && !d.is_dir() {
                if let Err(e) = fs::create_dir_all(d) {
                    w.error(format!("Cannot create target directory \"{}\"\n{}", d.display(), e));
                    shared.lock().unwrap().finished = true;
                    return;
                }
            }
        }
        for s in &sources {
            let r = match kind {
                OpKind::Copy => {
                    let t = target_for(s, dest.as_ref().unwrap(), many);
                    if &t == s {
                        w.error(format!("\"{}\"\nand\n\"{}\"\nare the same file", s.display(), t.display()));
                        Ok(())
                    } else {
                        w.copy_tree(s, &t)
                    }
                }
                OpKind::Move => {
                    let t = target_for(s, dest.as_ref().unwrap(), many);
                    w.move_one(s, &t)
                }
                OpKind::Delete => w.delete_tree(s),
            };
            if r.is_err() {
                break;
            }
        }
        shared.lock().unwrap().finished = true;
    });
    p
}

pub fn start_dir_size(path: PathBuf) -> Shared {
    let p = new_shared("Directory size");
    let shared = p.clone();
    thread::spawn(move || {
        let (_, b) = count_tree(&path);
        let mut g = shared.lock().unwrap();
        g.dir_size = Some(b);
        g.finished = true;
    });
    p
}

/// Sizes of several folders, one after another (Command > Show directory sizes).
pub fn start_dir_sizes(paths: Vec<PathBuf>) -> Shared {
    let p = new_shared("Directory sizes");
    let shared = p.clone();
    thread::spawn(move || {
        for path in paths {
            if shared.lock().unwrap().cancel {
                break;
            }
            shared.lock().unwrap().current = path.display().to_string();
            let (_, b) = count_tree(&path);
            shared.lock().unwrap().sizes.push((path, b));
        }
        shared.lock().unwrap().finished = true;
    });
    p
}

pub struct FindParams {
    pub start: PathBuf,
    pub pattern: String,
    pub content: String,
    pub case_sensitive: bool,
    pub skip_hidden: bool,
}

pub fn start_find(fp: FindParams) -> Shared {
    let p = new_shared("Find File");
    let shared = p.clone();
    thread::spawn(move || {
        let pattern = if fp.pattern.trim().is_empty() { "*".to_string() } else { fp.pattern.clone() };
        let needle: Vec<u8> = if fp.case_sensitive { fp.content.clone().into_bytes() } else { fp.content.to_lowercase().into_bytes() };
        let mut stack = vec![fp.start.clone()];
        while let Some(dir) = stack.pop() {
            if shared.lock().unwrap().cancel {
                break;
            }
            shared.lock().unwrap().current = dir.display().to_string();
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            let mut items: Vec<_> = rd.flatten().collect();
            items.sort_by_key(|e| e.file_name());
            let mut subdirs = Vec::new();
            for de in items {
                let name = de.file_name().to_string_lossy().to_string();
                if fp.skip_hidden && name.starts_with('.') {
                    continue;
                }
                let path = de.path();
                let Ok(ft) = de.file_type() else { continue };
                if ft.is_dir() {
                    subdirs.push(path.clone());
                }
                if !fsutil::wildcard_match(&pattern, &name, fp.case_sensitive) {
                    continue;
                }
                if !needle.is_empty() {
                    if !ft.is_file() || !file_contains(&path, &needle, fp.case_sensitive) {
                        continue;
                    }
                }
                let mut g = shared.lock().unwrap();
                g.found.push(path);
                g.files_done += 1;
            }
            subdirs.reverse();
            stack.extend(subdirs);
        }
        shared.lock().unwrap().finished = true;
    });
    p
}

fn file_contains(path: &Path, needle: &[u8], case_sensitive: bool) -> bool {
    let Ok(mut f) = fs::File::open(path) else { return false };
    let mut data = Vec::new();
    if Read::by_ref(&mut f).take(64 << 20).read_to_end(&mut data).is_err() {
        return false;
    }
    if !case_sensitive {
        data.make_ascii_lowercase();
    }
    data.windows(needle.len()).any(|w| w == needle)
}

pub fn start_shell(cmd: String, cwd: PathBuf) -> Shared {
    let p = new_shared(&cmd);
    let shared = p.clone();
    thread::spawn(move || {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let out = std::process::Command::new(&shell)
            .arg("-c")
            .arg(&cmd)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .output();
        let mut g = shared.lock().unwrap();
        match out {
            Ok(o) => {
                let mut s = String::from_utf8_lossy(&o.stdout).to_string();
                let e = String::from_utf8_lossy(&o.stderr);
                if !e.is_empty() {
                    if !s.is_empty() && !s.ends_with('\n') {
                        s.push('\n');
                    }
                    s.push_str(&e);
                }
                g.output = s;
                g.exit_code = o.status.code();
            }
            Err(e) => {
                g.output = format!("Cannot run \"{}\": {}", cmd, e);
                g.exit_code = Some(-1);
            }
        }
        g.finished = true;
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_copy_clones_and_overwrites() {
        let d = std::env::temp_dir().join(format!("mycmd-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("src/sub")).unwrap();
        fs::create_dir_all(d.join("dst")).unwrap();
        fs::write(d.join("src/a.txt"), b"hello").unwrap();
        fs::write(d.join("src/sub/b.bin"), vec![7u8; 100_000]).unwrap();
        fs::write(d.join("dst/src"), b"old").ok();
        fs::remove_file(d.join("dst/src")).unwrap();
        let p = start_file_op_with(OpKind::Copy, vec![d.join("src")], Some(d.join("dst")), false);
        wait(&p);
        {
            let g = p.lock().unwrap();
            assert!(g.errors.is_empty(), "{:?}", g.errors);
            assert_eq!((g.files_total, g.bytes_total), (2, 100_005));
            assert_eq!(g.bytes_done, 100_005);
        }
        assert_eq!(fs::read(d.join("dst/src/a.txt")).unwrap(), b"hello");
        // copy again over the existing files (overwrite path, also via clone)
        fs::write(d.join("src/a.txt"), b"HELLO2").unwrap();
        let p = start_file_op_with(OpKind::Copy, vec![d.join("src")], Some(d.join("dst")), false);
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty());
        assert_eq!(fs::read(d.join("dst/src/a.txt")).unwrap(), b"HELLO2");
        assert_eq!(fs::read(d.join("dst/src/sub/b.bin")).unwrap().len(), 100_000);
        let _ = fs::remove_dir_all(&d);
    }

    fn wait(p: &Shared) {
        for _ in 0..500 {
            if p.lock().unwrap().finished {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("job did not finish");
    }

    #[test]
    fn copy_move_delete() {
        let base = std::env::temp_dir().join(format!("mycmd-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        fs::create_dir_all(a.join("sub")).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("one.txt"), "hello").unwrap();
        fs::write(a.join("sub/two.txt"), "world!").unwrap();

        let p = start_file_op(OpKind::Copy, vec![a.join("one.txt"), a.join("sub")], Some(b.clone()));
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty());
        assert_eq!(fs::read_to_string(b.join("sub/two.txt")).unwrap(), "world!");
        assert_eq!(p.lock().unwrap().bytes_done, 11);

        let p = start_file_op(OpKind::Move, vec![b.join("one.txt")], Some(b.join("renamed.txt")));
        wait(&p);
        assert!(b.join("renamed.txt").exists() && !b.join("one.txt").exists());

        let p = start_file_op(OpKind::Delete, vec![b.join("sub")], None);
        wait(&p);
        assert!(!b.join("sub").exists());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn sizes_and_masks() {
        assert_eq!(fsutil::size_fit(1234567, 7), "1234567");
        assert_eq!(fsutil::size_fit(12345678, 7), "12056K");
        assert!(fsutil::wildcard_match("*.TXT", "a.txt", false));
        assert!(!fsutil::wildcard_match("*.TXT", "a.txt", true));
        assert!(fsutil::wildcard_match("f?le[0-9]*", "file7.rs", true));
        assert_eq!(fsutil::version_cmp("file2", "file10"), std::cmp::Ordering::Less);
    }

    /// Copy / move / delete between local folders and an "sftp://" location (real sftp client
    /// talking to a local sftp-server, so no network or ssh keys are needed).
    #[test]
    fn remote_copy_move_delete() {
        let server = "/usr/libexec/sftp-server";
        if !Path::new(server).exists() {
            return;
        }
        std::env::set_var("MYCMD_SFTP_SERVER", server);
        let base = std::env::temp_dir().join(format!("mycmd-rtest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (local, srv) = (base.join("local"), base.join("srv"));
        fs::create_dir_all(local.join("tree/sub")).unwrap();
        fs::create_dir_all(&srv).unwrap();
        fs::write(local.join("tree/one.txt"), "hello").unwrap();
        fs::write(local.join("tree/sub/two [x].txt"), "world!").unwrap();
        let remote = crate::vfs::build(crate::vfs::Scheme::Sftp, "local", &srv.to_string_lossy());

        // upload a folder tree
        let p = start_file_op(OpKind::Copy, vec![local.join("tree")], Some(remote.clone()));
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty(), "{:?}", p.lock().unwrap().errors);
        assert_eq!(fs::read_to_string(srv.join("tree/sub/two [x].txt")).unwrap(), "world!");

        // server-side move (rename) within the same host
        let rtree = crate::vfs::join(&remote, "tree");
        let moved = crate::vfs::join(&remote, "moved");
        let p = start_file_op(OpKind::Move, vec![rtree], Some(moved.clone()));
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty(), "{:?}", p.lock().unwrap().errors);
        assert!(srv.join("moved/one.txt").exists() && !srv.join("tree").exists());

        // download it again
        let back = base.join("back");
        fs::create_dir_all(&back).unwrap();
        let p = start_file_op(OpKind::Copy, vec![moved.clone()], Some(back.clone()));
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty(), "{:?}", p.lock().unwrap().errors);
        assert_eq!(fs::read_to_string(back.join("moved/one.txt")).unwrap(), "hello");

        // delete on the server
        let p = start_file_op(OpKind::Delete, vec![moved], None);
        wait(&p);
        assert!(p.lock().unwrap().errors.is_empty(), "{:?}", p.lock().unwrap().errors);
        assert!(!srv.join("moved").exists());
        let _ = fs::remove_dir_all(&base);
    }
}
