//! Background folder sizes (SPEC §4a, PNL-14): a small worker pool walks folders with
//! `getattrlistbulk(2)` (one syscall returns the name, type and size of many entries),
//! results are cached and revalidated in the background ("stale while revalidate").

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A cached size is trusted this long (and while the folder's mtime is unchanged).
const TTL: Duration = Duration::from_secs(60);
const WORKERS: usize = 2;

struct Item {
    size: u64,
    mtime: i64,
    at: Instant,
}

#[derive(Default)]
struct State {
    cache: HashMap<PathBuf, Item>,
    queue: VecDeque<PathBuf>,
    queued: HashSet<PathBuf>,
    running: HashSet<PathBuf>,
    /// Folders somebody still looks at; others are skipped / aborted.
    wanted: HashMap<usize, HashSet<PathBuf>>,
    /// Results computed before this instant are stale (explicit refresh).
    min_valid: Option<Instant>,
}

static STATE: OnceLock<(Mutex<State>, Condvar)> = OnceLock::new();
static VERSION: AtomicU64 = AtomicU64::new(1);

fn st() -> &'static (Mutex<State>, Condvar) {
    STATE.get_or_init(|| {
        for _ in 0..WORKERS {
            std::thread::spawn(worker);
        }
        (Mutex::new(State::default()), Condvar::new())
    })
}

/// Bumped whenever a result arrives; panels re-read their sizes when it changes.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// True while folders are queued or being measured.
pub fn busy() -> bool {
    let g = st().0.lock().unwrap();
    !g.queue.is_empty() || !g.running.is_empty()
}

/// An explicit refresh (F2 / after file operations): every cached size is revalidated.
pub fn invalidate() {
    st().0.lock().unwrap().min_valid = Some(Instant::now());
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Cached size of `dirs` (path, mtime) as far as known — stale values are returned too —
/// and queue everything missing or stale. `dirs` replaces what `owner` (a panel) wants.
pub fn lookup(owner: usize, dirs: &[(PathBuf, i64)]) -> Vec<Option<u64>> {
    let (m, cv) = st();
    let mut g = m.lock().unwrap();
    g.wanted.insert(owner, dirs.iter().map(|(p, _)| p.clone()).collect());
    let mut out = Vec::with_capacity(dirs.len());
    let mut todo = Vec::new();
    for (p, mt) in dirs {
        let (val, fresh) = match g.cache.get(p) {
            Some(it) => {
                let fresh = it.mtime == *mt && it.at.elapsed() < TTL && g.min_valid.map_or(true, |v| it.at > v);
                (Some(it.size), fresh)
            }
            None => (None, false),
        };
        out.push(val);
        if !fresh && !g.queued.contains(p) && !g.running.contains(p) {
            todo.push(p.clone());
        }
    }
    // The first entry of the listing is measured first; newer batches go before older ones.
    for p in todo.into_iter().rev() {
        g.queued.insert(p.clone());
        g.queue.push_front(p);
    }
    drop(g);
    cv.notify_all();
    out
}

fn worker() {
    loop {
        let path = {
            let (m, cv) = st();
            let mut g = m.lock().unwrap();
            loop {
                match g.queue.pop_front() {
                    Some(p) => {
                        g.queued.remove(&p);
                        if g.wanted.values().any(|w| w.contains(&p)) {
                            g.running.insert(p.clone());
                            break p;
                        }
                    }
                    None => g = cv.wait(g).unwrap(),
                }
            }
        };
        let mtime = std::fs::symlink_metadata(&path)
            .ok()
            .map(|m| std::os::unix::fs::MetadataExt::mtime(&m))
            .unwrap_or(0);
        let result = measure(&path, &|| st().0.lock().unwrap().wanted.values().any(|w| w.contains(&path)));
        let mut g = st().0.lock().unwrap();
        g.running.remove(&path);
        if let Some(size) = result {
            g.cache.insert(path, Item { size, mtime, at: Instant::now() });
        }
        drop(g);
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Total logical size of everything below `root` (symlinks not followed, other volumes
/// skipped). `None` when aborted because nobody wants the result any more.
fn measure(root: &Path, wanted: &dyn Fn() -> bool) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let dev = std::fs::symlink_metadata(root).ok()?.dev() as u32;
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    let mut n = 0u32;
    while let Some(dir) = stack.pop() {
        n += 1;
        if n % 16 == 0 && !wanted() {
            return None;
        }
        let before = stack.len();
        scan(&dir, dev, &mut total, &mut stack);
        // Cloud providers (OneDrive, iCloud, ...) would make network requests: skip them
        // unless the folder asked for is one of them.
        let mut i = before;
        while i < stack.len() {
            if is_cloud_mount(&stack[i]) && !is_cloud_mount(root) {
                stack.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }
    Some(total)
}

fn is_cloud_mount(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.contains("/Library/CloudStorage") || s.contains("/Library/Mobile Documents")
}

#[cfg(target_os = "macos")]
fn scan(dir: &Path, dev: u32, total: &mut u64, subdirs: &mut Vec<PathBuf>) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = CString::new(dir.as_os_str().as_bytes()) else { return };
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };
    if fd < 0 {
        return;
    }
    let mut al: libc::attrlist = unsafe { std::mem::zeroed() };
    al.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    al.commonattr = libc::ATTR_CMN_RETURNED_ATTRS | libc::ATTR_CMN_NAME | libc::ATTR_CMN_DEVID | libc::ATTR_CMN_OBJTYPE;
    al.fileattr = libc::ATTR_FILE_DATALENGTH;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let cnt = unsafe {
            libc::getattrlistbulk(
                fd,
                &mut al as *mut _ as *mut libc::c_void,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
                libc::FSOPT_PACK_INVAL_ATTRS as u64,
            )
        };
        if cnt <= 0 {
            break;
        }
        let mut off = 0usize;
        for _ in 0..cnt {
            let rd32 = |o: usize| unsafe { std::ptr::read_unaligned(buf.as_ptr().add(o) as *const u32) };
            let len = rd32(off) as usize;
            if len == 0 || off + len > buf.len() {
                break;
            }
            // Layout: u32 length, attribute_set_t (5 x u32), then the requested attributes
            // in bit order: name (attrreference), devid (u32), objtype (u32), datalength (i64).
            let mut p = off + 4;
            let common_ret = rd32(p);
            let file_ret = rd32(p + 12);
            p += 20;
            let name_ref = p;
            let mut name_ok = false;
            if common_ret & libc::ATTR_CMN_NAME != 0 {
                name_ok = true;
                p += 8;
            }
            let mut edev = dev;
            if common_ret & libc::ATTR_CMN_DEVID != 0 {
                edev = rd32(p);
                p += 4;
            }
            let mut ty = 0u32;
            if common_ret & libc::ATTR_CMN_OBJTYPE != 0 {
                ty = rd32(p);
                p += 4;
            }
            if ty == 2 {
                // VDIR
                if name_ok && edev == dev {
                    let o = rd32(name_ref) as i32 as isize;
                    let nlen = rd32(name_ref + 4) as usize;
                    let start = (name_ref as isize + o) as usize;
                    if nlen > 1 && start + nlen <= buf.len() {
                        let nm = &buf[start..start + nlen - 1]; // without the NUL
                        subdirs.push(dir.join(std::ffi::OsStr::from_bytes(nm)));
                    }
                }
            } else if ty == 1 && file_ret & libc::ATTR_FILE_DATALENGTH != 0 {
                let sz = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(p) as *const i64) };
                *total += sz.max(0) as u64;
            }
            off += len;
        }
    }
    unsafe { libc::close(fd) };
}

#[cfg(not(target_os = "macos"))]
fn scan(dir: &Path, dev: u32, total: &mut u64, subdirs: &mut Vec<PathBuf>) {
    use std::os::unix::fs::MetadataExt;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.path().symlink_metadata() else { continue };
        if md.is_dir() {
            if md.dev() as u32 == dev {
                subdirs.push(e.path());
            }
        } else if md.is_file() {
            *total += md.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_a_tree() {
        let d = std::env::temp_dir().join(format!("mycmd-dirsize-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("x"), vec![0u8; 1000]).unwrap();
        std::fs::write(d.join("a/y"), vec![0u8; 2000]).unwrap();
        std::fs::write(d.join("a/b/z"), vec![0u8; 345]).unwrap();
        std::os::unix::fs::symlink(&d, d.join("a/loop")).unwrap();
        assert_eq!(measure(&d, &|| true), Some(3345));
        // through the worker pool
        let mt = std::fs::metadata(&d).unwrap();
        let mtime = std::os::unix::fs::MetadataExt::mtime(&mt);
        let want = vec![(d.clone(), mtime)];
        assert_eq!(lookup(0, &want)[0], None);
        for _ in 0..100 {
            if let Some(v) = lookup(0, &want)[0] {
                assert_eq!(v, 3345);
                std::fs::remove_dir_all(&d).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("worker pool produced no result");
    }
}

#[cfg(test)]
mod bench {
    /// Manual: MYCMD_DIRSIZE_BENCH=/path cargo test --release bench -- --nocapture
    #[test]
    fn bench() {
        if let Ok(p) = std::env::var("MYCMD_DIRSIZE_BENCH") {
            let t = std::time::Instant::now();
            let n = super::measure(std::path::Path::new(&p), &|| true);
            println!("{:?} bytes in {:?}", n, t.elapsed());
        }
    }
}
