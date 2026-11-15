//! Remote locations (mc's "virtual file systems"), without any myCommander-specific setup:
//!
//! * `sftp://<host>/<abs path>` – talks to the system `sftp`. `<host>` is anything `ssh` understands:
//!   an alias from `~/.ssh/config` (user, port, key, ProxyJump …) or `user@host`. Keys come from the
//!   ssh agent / keychain / `IdentityFile`; passwords are never asked for (BatchMode).
//! * `ftp://[user@]<host>/<path>` – talks to the system `curl`. Credentials come from `~/.netrc`.
//!
//! A remote location is an ordinary `PathBuf` holding that URL, so panels, history and the hotlist
//! work unchanged; everything that touches the disk goes through the functions here when
//! `is_remote` is true.

use crate::fsutil::{Entry, Kind};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    Sftp,
    Ftp,
}

#[derive(Clone, Debug)]
pub struct Loc {
    pub scheme: Scheme,
    pub host: String,
    /// Absolute, always starts with `/`.
    pub path: String,
}

pub fn is_remote(p: &Path) -> bool {
    let s = p.to_string_lossy();
    s.starts_with("sftp://") || s.starts_with("ftp://")
}

pub fn parse(p: &Path) -> Option<Loc> {
    let s = p.to_string_lossy();
    let (scheme, rest) = if let Some(r) = s.strip_prefix("sftp://") {
        (Scheme::Sftp, r)
    } else if let Some(r) = s.strip_prefix("ftp://") {
        (Scheme::Ftp, r)
    } else {
        return None;
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host.is_empty() {
        return None;
    }
    let mut path = path.to_string();
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    Some(Loc { scheme, host: host.to_string(), path })
}

pub fn build(scheme: Scheme, host: &str, path: &str) -> PathBuf {
    let prefix = if scheme == Scheme::Sftp { "sftp://" } else { "ftp://" };
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    PathBuf::from(format!("{prefix}{host}{path}"))
}

/// Parent folder of a local or remote path (`None` at the root).
pub fn parent(p: &Path) -> Option<PathBuf> {
    match parse(p) {
        None => p.parent().map(|x| x.to_path_buf()),
        Some(l) => {
            if l.path == "/" {
                return None;
            }
            let cut = l.path.rfind('/').unwrap_or(0);
            let up = if cut == 0 { "/" } else { &l.path[..cut] };
            Some(build(l.scheme, &l.host, up))
        }
    }
}

/// Join a child name onto a local or remote folder.
pub fn join(dir: &Path, name: &str) -> PathBuf {
    match parse(dir) {
        None => dir.join(name),
        Some(l) => {
            let base = if l.path == "/" { String::new() } else { l.path };
            build(l.scheme, &l.host, &format!("{base}/{name}"))
        }
    }
}

pub fn base_name(p: &Path) -> String {
    match parse(p) {
        None => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        Some(l) => l.path.rsplit('/').next().unwrap_or("").to_string(),
    }
}

/// Canonical form of a remote path (no trailing slash except the root); local paths unchanged.
pub fn normalize(p: &Path) -> PathBuf {
    match parse(p) {
        Some(l) => build(l.scheme, &l.host, &l.path),
        None => p.to_path_buf(),
    }
}

/// Turn what the person typed in the connect dialog into a location.
/// Accepts `host`, `user@host`, `host/path`, or a full `sftp://…` / `ftp://…` URL.
pub fn from_input(scheme: Scheme, input: &str) -> Result<PathBuf, String> {
    let t = input.trim();
    let t = t.strip_prefix("sftp://").or_else(|| t.strip_prefix("ftp://")).unwrap_or(t);
    let (host, path) = match t.find('/') {
        Some(i) => (&t[..i], Some(&t[i..])),
        None => (t, None),
    };
    if host.is_empty() {
        return Err("Enter a host".into());
    }
    if scheme == Scheme::Sftp {
        check_host(host).map_err(|e| e.to_string())?;
    }
    let path = match path {
        Some(p) => p.to_string(),
        None => home(scheme, host).map_err(|e| e.to_string())?,
    };
    Ok(build(scheme, host, &path))
}

/// The login folder of a host (sftp: asks the server, ftp: the root of the login).
fn home(scheme: Scheme, host: &str) -> io::Result<String> {
    match scheme {
        Scheme::Ftp => Ok("/".into()),
        Scheme::Sftp => {
            let out = sftp(host, "pwd\n")?;
            out.lines().find_map(|l| l.strip_prefix("Remote working directory: ")).map(|s| s.trim().to_string()).ok_or_else(|| err("Server did not report its home folder"))
        }
    }
}

/// Short text for titles: `host:/path`.
pub fn display(p: &Path) -> String {
    match parse(p) {
        None => p.display().to_string(),
        Some(l) => format!("{}:{}", l.host, l.path),
    }
}

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.into())
}

// ---------------------------------------------------------------- ssh config

/// Host aliases from `~/.ssh/config` (no wildcards), for the connect dialog.
pub fn ssh_hosts() -> Vec<String> {
    let mut out = Vec::new();
    let path = crate::fsutil::home_dir().join(".ssh/config");
    collect_hosts(&path, &mut out, 0);
    out.sort();
    out.dedup();
    out
}

fn collect_hosts(path: &Path, out: &mut Vec<String>, depth: usize) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    for line in text.lines() {
        let l = line.trim();
        let mut it = l.splitn(2, char::is_whitespace);
        let (Some(key), Some(rest)) = (it.next(), it.next()) else { continue };
        if key.eq_ignore_ascii_case("host") {
            for h in rest.split_whitespace() {
                if !h.contains(['*', '?', '!']) {
                    out.push(h.to_string());
                }
            }
        } else if key.eq_ignore_ascii_case("include") && depth < 3 {
            for pat in rest.split_whitespace() {
                let p = if let Some(r) = pat.strip_prefix("~/") {
                    crate::fsutil::home_dir().join(r)
                } else if pat.starts_with('/') {
                    PathBuf::from(pat)
                } else {
                    crate::fsutil::home_dir().join(".ssh").join(pat)
                };
                if p.is_file() {
                    collect_hosts(&p, out, depth + 1);
                } else if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
                    // simple "dir/*" globs
                    if name.to_string_lossy() == "*" {
                        if let Ok(rd) = std::fs::read_dir(dir) {
                            for e in rd.flatten() {
                                collect_hosts(&e.path(), out, depth + 1);
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- SFTP

fn control_dir() -> PathBuf {
    let d = PathBuf::from(format!("/tmp/myCommander-ssh-{}", unsafe { libc::getuid() }));
    if std::fs::create_dir_all(&d).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    d
}

/// Quote a path for an sftp batch command (no glob protection, for arguments sftp does not glob).
fn q(s: &str) -> String {
    quote(s)
}

/// For arguments that sftp expands as globs (the path of `ls`, `rm`, remote side of `get`, local
/// side of `put`): backslash-escape everything special instead of using quotes, which is the form
/// sftp's glob code unescapes back to the literal name.
fn qg(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\\\\\"),
            c if c.is_whitespace() || matches!(c, '"' | '\'' | '*' | '?' | '[' | ']') => {
                o.push('\\');
                o.push(c);
            }
            c => o.push(c),
        }
    }
    o
}

fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                o.push('\\');
                o.push(c);
            }
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Split `[user@]host[:port]` into the ssh destination and an optional port.
fn split_port(host: &str) -> (&str, Option<&str>) {
    match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, Some(p)),
        _ => (host, None),
    }
}

fn check_host(host: &str) -> io::Result<()> {
    let (h, _) = split_port(host);
    if h.is_empty() || h.starts_with('-') || h.contains([':', ' ', '\'', '"', '\n']) {
        return Err(err(format!("Invalid host \"{host}\" (use an alias from ~/.ssh/config or user@host)")));
    }
    Ok(())
}

fn friendly_ssh_error(stderr: &str, host: &str) -> String {
    let e = stderr.trim();
    if e.contains("Host key verification failed") || e.contains("REMOTE HOST IDENTIFICATION") {
        format!("{e}\n\nConnect once with `ssh {host}` in Terminal to accept the host key.")
    } else if e.contains("Permission denied") {
        format!("{e}\n\nmyCommander only uses your ssh keys (agent, keychain, IdentityFile). Check that `ssh {host}` works without a password prompt.")
    } else {
        e.to_string()
    }
}

/// Run sftp commands against `host` in batch mode. Returns stdout without the echoed commands.
fn sftp(host: &str, script: &str) -> io::Result<String> {
    check_host(host)?;
    let cp = format!("ControlPath={}/%C", control_dir().display());
    let (dest, port) = split_port(host);
    let mut cmd = Command::new("/usr/bin/sftp");
    cmd.args(["-q", "-b", "-"]);
    // Test aid: MYCMD_SFTP_SERVER=/usr/libexec/sftp-server talks to a local sftp-server, no ssh involved.
    if let Some(server) = std::env::var_os("MYCMD_SFTP_SERVER") {
        cmd.arg("-D").arg(server);
    } else {
        cmd.args(["-o", "BatchMode=yes", "-o", "ControlMaster=auto", "-o", &cp, "-o", "ControlPersist=120", "-o", "ConnectTimeout=15"]);
        if let Some(p) = port {
            cmd.args(["-P", p]);
        }
        cmd.arg(dest);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| err(format!("Cannot run sftp: {e}")))?;
    child.stdin.take().unwrap().write_all(script.as_bytes())?;
    let out = child.wait_with_output()?;
    let stdout: String = String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.starts_with("sftp> ")).collect::<Vec<_>>().join("\n");
    if out.status.success() {
        Ok(stdout)
    } else {
        let mut e = String::from_utf8_lossy(&out.stderr).to_string();
        if e.trim().is_empty() {
            e = stdout.clone();
        }
        Err(err(friendly_ssh_error(&e, host)))
    }
}

// ---------------------------------------------------------------- FTP (curl)

fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{:02X}", b));
        }
    }
    o
}

fn ftp_url(l: &Loc, dir: bool) -> String {
    let rel = pct(l.path.trim_start_matches('/'));
    let mut u = format!("ftp://{}/{}", l.host, rel);
    if dir && !u.ends_with('/') {
        u.push('/');
    }
    u
}

fn curl(args: &[&str], url: &str) -> io::Result<Vec<u8>> {
    if url.contains(['\n', '\r']) {
        return Err(err("Invalid name"));
    }
    let out = Command::new("/usr/bin/curl")
        .args(["-sS", "--netrc-optional", "--connect-timeout", "15"])
        .args(args)
        .arg("--")
        .arg(url)
        .output()
        .map_err(|e| err(format!("Cannot run curl: {e}")))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let hint = if e.contains("530") || e.contains("Access denied") { "\n\nCredentials come from ~/.netrc (machine <host> login <user> password <pw>)." } else { "" };
        Err(err(format!("{e}{hint}")))
    }
}

fn ftp_cmd(l: &Loc, quote: &[String]) -> io::Result<()> {
    let root = format!("ftp://{}/", l.host);
    let mut args: Vec<&str> = Vec::new();
    for q in quote {
        args.push("-Q");
        args.push(q);
    }
    args.extend(["-o", "/dev/null"]);
    curl(&args, &root).map(|_| ())
}

// ---------------------------------------------------------------- listing parser

fn parse_ls_line(parent: &Path, line: &str) -> Option<Entry> {
    // perms nlink owner group size month day time|year name…
    let mut rest = line.trim_start();
    let mut tok: Vec<&str> = Vec::new();
    for _ in 0..8 {
        let end = rest.find(char::is_whitespace)?;
        tok.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    let perms = tok[0];
    if perms.len() < 10 {
        return None;
    }
    let mut name = rest.trim_end().to_string();
    let t = perms.as_bytes()[0] as char;
    if t == 'l' {
        if let Some(i) = name.find(" -> ") {
            name.truncate(i);
        }
    }
    // Some sftp builds print the full path of each entry; names never contain '/'.
    if let Some(i) = name.rfind('/') {
        name = name[i + 1..].to_string();
    }
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let kind = match t {
        'd' => Kind::Dir,
        'l' => Kind::Symlink,
        '-' => Kind::File,
        _ => Kind::Special,
    };
    let b = perms.as_bytes();
    let bit = |i: usize, on: &[u8]| on.contains(&b[i]) as u32;
    let mut mode = match t {
        'd' => 0o040000,
        'l' => 0o120000,
        '-' => 0o100000,
        _ => 0,
    };
    mode |= bit(1, b"r") << 8 | bit(2, b"w") << 7 | bit(3, b"xsS") << 6;
    mode |= bit(4, b"r") << 5 | bit(5, b"w") << 4 | bit(6, b"xsS") << 3;
    mode |= bit(7, b"r") << 2 | bit(8, b"w") << 1 | bit(9, b"xtT");
    let size: u64 = tok[4].parse().unwrap_or(0);
    let mtime = parse_ls_time(tok[5], tok[6], tok[7]).unwrap_or(0);
    Some(Entry {
        path: join(parent, &name),
        name,
        kind,
        link_to_dir: false,
        size,
        mode,
        nlink: tok[1].parse().unwrap_or(1),
        uid: tok[2].parse().unwrap_or(0),
        gid: tok[3].parse().unwrap_or(0),
        ino: 0,
        rdev: 0,
        mtime,
        atime: mtime,
        ctime: mtime,
        marked: false,
        dir_size: None,
    })
}

fn parse_ls_time(mon: &str, day: &str, ty: &str) -> Option<i64> {
    use chrono::{Datelike, NaiveDate};
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let m = M.iter().position(|x| x.eq_ignore_ascii_case(mon))? as u32 + 1;
    let d: u32 = day.parse().ok()?;
    let now = chrono::Utc::now();
    let (y, h, mi) = if let Some((h, mi)) = ty.split_once(':') {
        let mut y = now.year();
        let cand = NaiveDate::from_ymd_opt(y, m, d)?;
        if cand > (now + chrono::Duration::days(1)).date_naive() {
            y -= 1;
        }
        (y, h.parse().ok()?, mi.parse().ok()?)
    } else {
        (ty.parse().ok()?, 0, 0)
    };
    // ls prints the server's local wall-clock time; show those numbers as they are.
    use chrono::TimeZone;
    chrono::Local.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d)?.and_hms_opt(h, mi, 0)?).earliest().map(|t| t.timestamp())
}

// ---------------------------------------------------------------- public operations

/// Children of a remote folder (without `.` and `..`).
pub fn list(p: &Path) -> io::Result<Vec<Entry>> {
    let l = parse(p).ok_or_else(|| err("not a remote path"))?;
    let text = match l.scheme {
        Scheme::Sftp => sftp(&l.host, &format!("ls -lan {}\n", qg(&l.path)))?,
        Scheme::Ftp => String::from_utf8_lossy(&curl(&[], &ftp_url(&l, true))?).to_string(),
    };
    let mut out: Vec<Entry> = text.lines().filter_map(|ln| parse_ls_line(p, ln)).collect();
    if out.is_empty() && !text.trim().is_empty() && !text.trim_start().starts_with("total") {
        // Something came back but nothing parsed: not a Unix-style listing.
        if !text.lines().all(|l| l.trim().is_empty() || l.starts_with("total")) {
            return Err(err("Unrecognised directory listing format (only Unix-style listings are supported)"));
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Entry for one remote path (found by listing its parent).
pub fn entry_of(p: &Path) -> Option<Entry> {
    let name = base_name(p);
    match parent(p) {
        Some(par) => list(&par).ok()?.into_iter().find(|e| e.name == name),
        None => {
            let mut e = root_entry(p);
            e.name = String::new();
            Some(e)
        }
    }
}

pub fn root_entry(p: &Path) -> Entry {
    Entry {
        name: "..".into(),
        path: p.to_path_buf(),
        kind: Kind::Dir,
        link_to_dir: false,
        size: 0,
        mode: 0o040755,
        nlink: 1,
        uid: 0,
        gid: 0,
        ino: 0,
        rdev: 0,
        mtime: 0,
        atime: 0,
        ctime: 0,
        marked: false,
        dir_size: None,
    }
}

pub fn is_dir(p: &Path) -> bool {
    if !is_remote(p) {
        return p.is_dir();
    }
    entry_of(p).map(|e| e.is_dir_like() || e.kind == Kind::Symlink && list(p).is_ok()).unwrap_or(false)
}

pub fn exists(p: &Path) -> bool {
    if is_remote(p) {
        entry_of(p).is_some()
    } else {
        std::fs::symlink_metadata(p).is_ok()
    }
}

pub fn mkdir(p: &Path) -> io::Result<()> {
    let l = parse(p).ok_or_else(|| err("not a remote path"))?;
    match l.scheme {
        Scheme::Sftp => sftp(&l.host, &format!("mkdir {}\n", q(&l.path))).map(|_| ()),
        Scheme::Ftp => ftp_cmd(&l, &[format!("MKD {}", l.path.trim_start_matches('/'))]),
    }
}

pub fn remove(p: &Path, is_dir: bool) -> io::Result<()> {
    let l = parse(p).ok_or_else(|| err("not a remote path"))?;
    match l.scheme {
        Scheme::Sftp => sftp(&l.host, &format!("{} {}\n", if is_dir { "rmdir" } else { "rm" }, if is_dir { q(&l.path) } else { qg(&l.path) })).map(|_| ()),
        Scheme::Ftp => ftp_cmd(&l, &[format!("{} {}", if is_dir { "RMD" } else { "DELE" }, l.path.trim_start_matches('/'))]),
    }
}

/// Rename within one remote host.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let (a, b) = (parse(from).ok_or_else(|| err("not a remote path"))?, parse(to).ok_or_else(|| err("not a remote path"))?);
    if a.scheme != b.scheme || a.host != b.host {
        return Err(err("Cannot rename between different hosts"));
    }
    match a.scheme {
        Scheme::Sftp => sftp(&a.host, &format!("rename {} {}\n", q(&a.path), q(&b.path))).map(|_| ()),
        Scheme::Ftp => ftp_cmd(&a, &[format!("RNFR {}", a.path.trim_start_matches('/')), format!("RNTO {}", b.path.trim_start_matches('/'))]),
    }
}

pub fn same_host(a: &Path, b: &Path) -> bool {
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) => x.scheme == y.scheme && x.host == y.host,
        _ => false,
    }
}

pub fn download(p: &Path, local: &Path) -> io::Result<()> {
    let l = parse(p).ok_or_else(|| err("not a remote path"))?;
    let lp = local.to_string_lossy();
    match l.scheme {
        Scheme::Sftp => sftp(&l.host, &format!("get -p {} {}\n", qg(&l.path), q(&lp))).map(|_| ()),
        Scheme::Ftp => curl(&["-o", &lp], &ftp_url(&l, false)).map(|_| ()),
    }
}

pub fn upload(local: &Path, p: &Path) -> io::Result<()> {
    let l = parse(p).ok_or_else(|| err("not a remote path"))?;
    let lp = local.to_string_lossy();
    match l.scheme {
        Scheme::Sftp => sftp(&l.host, &format!("put -p {} {}\n", qg(&lp), q(&l.path))).map(|_| ()),
        Scheme::Ftp => curl(&["-T", &lp], &ftp_url(&l, false)).map(|_| ()),
    }
}

/// Download a remote file into the cache folder and return the local copy
/// (for View, Edit, Quick view and "open"). Refuses files above `max_bytes`.
pub fn fetch(p: &Path, size: u64, max_bytes: u64) -> io::Result<PathBuf> {
    if size > max_bytes {
        return Err(err(format!("File too large to download for viewing ({} MB, limit {} MB)", size / 1024 / 1024, max_bytes / 1024 / 1024)));
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    let dir = cache_dir().join(format!("{:016x}", h.finish()));
    std::fs::create_dir_all(&dir)?;
    let local = dir.join(base_name(p));
    download(p, &local)?;
    Ok(local)
}

pub fn cache_dir() -> PathBuf {
    crate::fsutil::home_dir().join("Library/Caches/myCommander/remote")
}

/// Remove downloaded copies (called on quit).
pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        let p = PathBuf::from("sftp://me@box/a/b");
        assert!(is_remote(&p));
        assert_eq!(parent(&p).unwrap(), PathBuf::from("sftp://me@box/a"));
        assert_eq!(parent(&PathBuf::from("sftp://me@box/a")).unwrap(), PathBuf::from("sftp://me@box/"));
        assert!(parent(&PathBuf::from("sftp://me@box/")).is_none());
        assert_eq!(join(&PathBuf::from("sftp://box/"), "x"), PathBuf::from("sftp://box/x"));
        assert_eq!(join(&p, "c d"), PathBuf::from("sftp://me@box/a/b/c d"));
        assert_eq!(base_name(&p), "b");
    }

    #[test]
    fn ls_parser() {
        let dir = PathBuf::from("ftp://h/d");
        let e = parse_ls_line(&dir, "drwxr-xr-x    5 1000     1000         4096 Nov 14 10:00 my dir").unwrap();
        assert_eq!(e.name, "my dir");
        assert!(e.is_dir_like());
        assert_eq!(e.path, PathBuf::from("ftp://h/d/my dir"));
        let f = parse_ls_line(&dir, "-rw-r--r--    1 user group 1234 Jan  3  2020 a.txt").unwrap();
        assert_eq!((f.size, f.mode & 0o777), (1234, 0o644));
        let l = parse_ls_line(&dir, "lrwxrwxrwx 1 u g 5 Jan  3  2020 ln -> a.txt").unwrap();
        assert_eq!((l.name.as_str(), l.kind), ("ln", Kind::Symlink));
        assert!(parse_ls_line(&dir, "total 12").is_none());
        assert!(parse_ls_line(&dir, "drwxr-xr-x 2 u g 64 Jan  3  2020 .").is_none());
    }

    #[test]
    fn quoting() {
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q("a\"b"), "\"a\\\"b\"");
    }

    /// Real `sftp` client against a local `sftp-server` (no network, no ssh keys).
    #[test]
    fn sftp_roundtrip() {
        let server = "/usr/libexec/sftp-server";
        if !Path::new(server).exists() {
            return;
        }
        std::env::set_var("MYCMD_SFTP_SERVER", server);
        let tmp = std::env::temp_dir().join(format!("mycmd-vfs-test-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("sub dir")).unwrap();
        std::fs::write(tmp.join("a b.txt"), "hello").unwrap();
        let root = build(Scheme::Sftp, "local", &tmp.to_string_lossy());

        let names: Vec<String> = list(&root).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["a b.txt", "sub dir"]);
        let f = entry_of(&join(&root, "a b.txt")).unwrap();
        assert_eq!(f.size, 5);
        assert!(entry_of(&join(&root, "sub dir")).unwrap().is_dir_like());
        assert!(is_dir(&join(&root, "sub dir")));

        // download / upload with odd names
        let local = tmp.join("down.txt");
        download(&join(&root, "a b.txt"), &local).unwrap();
        assert_eq!(std::fs::read_to_string(&local).unwrap(), "hello");
        upload(&local, &join(&root, "sub dir/up [1].txt")).unwrap();
        assert_eq!(std::fs::read_to_string(tmp.join("sub dir/up [1].txt")).unwrap(), "hello");

        // mkdir, rename, remove
        mkdir(&join(&root, "new")).unwrap();
        rename(&join(&root, "new"), &join(&root, "renamed")).unwrap();
        assert!(tmp.join("renamed").is_dir());
        remove(&join(&root, "renamed"), true).unwrap();
        remove(&join(&root, "sub dir/up [1].txt"), false).unwrap();
        assert!(!tmp.join("sub dir/up [1].txt").exists());
        assert!(list(&join(&root, "missing")).is_err());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// Needs an FTP server: MYCMD_FTP_TEST=localhost:2121 (login folder = test root). Skipped otherwise.
    #[test]
    fn ftp_roundtrip() {
        let Ok(host) = std::env::var("MYCMD_FTP_TEST") else { return };
        let root = build(Scheme::Ftp, &host, "/");
        let names: Vec<String> = list(&root).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"hello world.txt".to_string()) && names.contains(&"dir one".to_string()), "{names:?}");
        let f = entry_of(&join(&root, "hello world.txt")).unwrap();
        assert_eq!(f.size, 7);
        assert!(is_dir(&join(&root, "dir one")));
        assert_eq!(list(&join(&root, "dir one")).unwrap()[0].name, "in.txt");

        let tmp = std::env::temp_dir().join(format!("mycmd-ftp-{}.txt", std::process::id()));
        download(&join(&root, "hello world.txt"), &tmp).unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "remote\n");
        upload(&tmp, &join(&root, "dir one/up #1.txt")).unwrap();
        assert!(entry_of(&join(&root, "dir one/up #1.txt")).is_some());
        mkdir(&join(&root, "made")).unwrap();
        rename(&join(&root, "made"), &join(&root, "made two")).unwrap();
        assert!(is_dir(&join(&root, "made two")));
        remove(&join(&root, "made two"), true).unwrap();
        remove(&join(&root, "dir one/up #1.txt"), false).unwrap();
        assert!(!exists(&join(&root, "dir one/up #1.txt")));
        assert!(!exists(&join(&root, "made two")));
        let _ = std::fs::remove_file(&tmp);
    }
}
