//! Mounted volumes for the Places dialog (SPEC PNL-16).
//!
//! The list comes from the kernel mount table (`getmntinfo(3)` with `MNT_NOWAIT`), which never
//! touches the volumes, so a stale network mount cannot block the UI.

use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Mount {
    pub path: PathBuf,
    pub name: String,
    pub fstype: String,
    /// Device or `//user@host/share`.
    pub source: String,
    pub total: u64,
    pub free: u64,
    pub network: bool,
    pub is_root: bool,
}

impl Mount {
    /// "SMB · 192.168.0.20" for shares, "APFS" for disks.
    pub fn kind(&self) -> String {
        if self.network {
            let host = self.source.rsplit('@').next().unwrap_or(&self.source);
            let host = host.trim_start_matches('/').split('/').next().unwrap_or("");
            let proto = match self.fstype.as_str() {
                "smbfs" => "SMB",
                "nfs" => "NFS",
                "afpfs" => "AFP",
                "webdav" => "WebDAV",
                other => other,
            };
            format!("{proto} · {host}")
        } else {
            self.fstype.to_uppercase()
        }
    }
}

/// Does this volume hold Time Machine backups? Strings only, except for the marker folder on
/// local volumes (never on network mounts).
pub fn is_time_machine(path: &str, source: &str, network: bool) -> bool {
    let p = path.to_lowercase();
    let s = source.to_lowercase();
    if p.starts_with("/volumes/.timemachine") || p.contains("com.apple.timemachine") {
        return true;
    }
    if p.contains("time machine") || p.contains("timemachine") || s.contains("timemachine") {
        return true;
    }
    !network && std::path::Path::new(path).join("Backups.backupdb").exists()
}

#[cfg(target_os = "macos")]
pub fn list(show_time_machine: bool) -> Vec<Mount> {
    use std::ffi::CStr;
    const MNT_DONTBROWSE: u32 = 0x0010_0000;
    const MNT_LOCAL: u32 = 0x0000_1000;
    let mut buf: *mut libc::statfs = std::ptr::null_mut();
    // SAFETY: getmntinfo fills `buf` with a pointer to a static array of `n` entries.
    let n = unsafe { libc::getmntinfo(&mut buf, libc::MNT_NOWAIT) };
    if n <= 0 || buf.is_null() {
        return Vec::new();
    }
    let entries = unsafe { std::slice::from_raw_parts(buf, n as usize) };
    let mut out = Vec::new();
    for e in entries {
        let s = |p: *const libc::c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        let path = s(e.f_mntonname.as_ptr());
        let fstype = s(e.f_fstypename.as_ptr());
        let source = s(e.f_mntfromname.as_ptr());
        if e.f_flags & MNT_DONTBROWSE != 0 && path != "/" {
            continue;
        }
        if path.starts_with("/System/") || path.starts_with("/private/") || matches!(fstype.as_str(), "devfs" | "autofs" | "fdesc" | "volfs") {
            continue;
        }
        let network = e.f_flags & MNT_LOCAL == 0;
        if !show_time_machine && is_time_machine(&path, &source, network) {
            continue;
        }
        let is_root = path == "/";
        let name = if is_root {
            "Macintosh HD".to_string()
        } else {
            std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone())
        };
        let bs = e.f_bsize as u64;
        out.push(Mount { path: PathBuf::from(&path), name, fstype, source, total: e.f_blocks * bs, free: e.f_bavail * bs, network, is_root });
    }
    out.sort_by(|a, b| b.is_root.cmp(&a.is_root).then(a.network.cmp(&b.network)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

#[cfg(not(target_os = "macos"))]
pub fn list(_show_time_machine: bool) -> Vec<Mount> {
    Vec::new()
}

static RESULT: Mutex<Option<String>> = Mutex::new(None);

/// Unmount in the background; the outcome is picked up with [`take_result`].
pub fn eject(path: PathBuf, ctx: egui::Context) {
    std::thread::spawn(move || {
        let out = std::process::Command::new("/usr/sbin/diskutil").arg("unmount").arg(&path).output();
        let msg = match out {
            Ok(o) if o.status.success() => format!("Unmounted {}", path.display()),
            Ok(o) => {
                let e = String::from_utf8_lossy(&o.stderr);
                let e = if e.trim().is_empty() { String::from_utf8_lossy(&o.stdout).into_owned() } else { e.into_owned() };
                format!("Cannot unmount {}: {}", path.display(), e.trim())
            }
            Err(e) => format!("Cannot unmount {}: {e}", path.display()),
        };
        *RESULT.lock().unwrap() = Some(msg);
        ctx.request_repaint();
    });
}

pub fn take_result() -> Option<String> {
    RESULT.lock().unwrap().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_machine_detection() {
        assert!(is_time_machine("/Volumes/.timemachine/abc", "x", true));
        assert!(is_time_machine("/Volumes/Time Machine Backups", "x", true));
        assert!(is_time_machine("/Volumes/X", "com.apple.TimeMachine.2024", false));
        assert!(!is_time_machine("/Users/kay/mnt/fotoarchiv", "//kay@nas/fotoarchiv", true));
    }

    #[test]
    fn lists_root() {
        let l = list(true);
        assert!(l.iter().any(|m| m.is_root));
    }
}
