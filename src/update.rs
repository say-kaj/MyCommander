//! In-app updates (SPEC §13): look for a newer GitHub release in the background and,
//! on request, download it, replace the running .app bundle and relaunch.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

const REPO: &str = "say-kaj/MyCommander";
const ASSET_SUFFIX: &str = "macos-arm64.zip";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, url: String },
    Installing(String),
    /// New bundle is in place and the relauncher is waiting: quit now.
    Restart,
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);

pub fn state() -> State {
    STATE.lock().unwrap().clone()
}

fn set(s: State) {
    *STATE.lock().unwrap() = s;
}

pub fn busy() -> bool {
    matches!(state(), State::Checking | State::Installing(_))
}

/// "1.2.0" > "1.1.1" (numeric, dotted; a leading "v" is ignored).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let p = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split('.').map(|x| x.parse().unwrap_or(0)).collect() };
    p(candidate) > p(current)
}

/// The running .app bundle, if we run from one.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let app = exe.parent()?.parent()?.parent()?.to_path_buf();
    (app.extension().map_or(false, |e| e == "app")).then_some(app)
}

pub fn check_async() {
    if busy() {
        return;
    }
    set(State::Checking);
    std::thread::spawn(|| match check() {
        Ok(s) => set(s),
        Err(_) => set(State::Idle), // offline etc.: stay quiet
    });
}

fn check() -> Result<State, String> {
    // Test aid: MYCMD_UPDATE_TEST="<version>|<zip url>" pretends this release exists;
    // MYCMD_UPDATE_AUTO=1 installs it right away.
    if let Ok(t) = std::env::var("MYCMD_UPDATE_TEST") {
        if let Some((v, u)) = t.split_once('|') {
            if std::env::var("MYCMD_UPDATE_AUTO").is_ok() {
                install_async(v.to_string(), u.to_string());
            }
            return Ok(State::Available { version: v.to_string(), url: u.to_string() });
        }
    }
    let out = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "15", "-H", "User-Agent: myCommander-updater", "-H", "Accept: application/vnd.github+json"])
        .arg(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("request failed".into());
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no tag")?.trim_start_matches('v').to_string();
    if !is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Ok(State::UpToDate);
    }
    let url = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str().map_or(false, |n| n.ends_with(ASSET_SUFFIX))))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or("no asset")?
        .to_string();
    Ok(State::Available { version: tag, url })
}

/// Download, verify, swap the bundle and relaunch (after this process exits).
pub fn install_async(version: String, url: String) {
    set(State::Installing("Downloading…".into()));
    std::thread::spawn(move || {
        if let Err(e) = install(&version, &url) {
            set(State::Failed(e));
        }
    });
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let st = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err(format!("{:?} failed", cmd.get_program())) }
}

fn install(version: &str, url: &str) -> Result<(), String> {
    let bundle = bundle_path().ok_or("not running from an .app bundle")?;
    let parent = bundle.parent().ok_or("bad path")?;
    let c = std::ffi::CString::new(parent.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::access(c.as_ptr(), libc::W_OK) } != 0 {
        return Err(format!("no write permission in {}", parent.display()));
    }
    let tmp = std::env::temp_dir().join(format!("myCommander-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip = tmp.join("update.zip");
    run(Command::new("/usr/bin/curl").args(["-fsSL", "--max-time", "300", "-o"]).arg(&zip).arg(url)).map_err(|_| "download failed".to_string())?;
    set(State::Installing("Unpacking…".into()));
    let ex = tmp.join("x");
    run(Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&zip).arg(&ex)).map_err(|_| "unpack failed".to_string())?;
    let new_app = ex.join("myCommander.app");
    run(Command::new("/usr/bin/codesign").args(["--verify", "--deep"]).arg(&new_app)).map_err(|_| "signature check failed".to_string())?;
    let out = Command::new(new_app.join("Contents/MacOS/myCommander")).arg("--version").output().map_err(|e| e.to_string())?;
    if !String::from_utf8_lossy(&out.stdout).contains(version) {
        return Err("downloaded app has an unexpected version".into());
    }
    // Move it next to the old one first, so the final swap is two quick renames.
    let staged = parent.join(".myCommander-new.app");
    let _ = std::fs::remove_dir_all(&staged);
    run(Command::new("/bin/mv").arg(&new_app).arg(&staged)).map_err(|_| "could not stage the new app".to_string())?;
    let script = r#"while kill -0 "$3" 2>/dev/null; do sleep 0.2; done
rm -rf "$1" && mv "$2" "$1" && open "$1"
rm -rf "$4""#;
    Command::new("/bin/sh")
        .args(["-c", script, "sh"])
        .arg(&bundle)
        .arg(&staged)
        .arg(std::process::id().to_string())
        .arg(&tmp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    set(State::Restart);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("1.2.0", "1.1.1"));
        assert!(is_newer("v1.10.0", "1.9.9"));
        assert!(!is_newer("1.1.1", "1.1.1"));
        assert!(!is_newer("1.0.9", "1.1.0"));
    }
}
