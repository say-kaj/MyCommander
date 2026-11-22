//! Remote control (SPEC §14, REM): the tool table, the local socket server inside the app
//! and the MCP server (`myCommander --mcp`) that forwards agent calls to it.
//!
//! Tools that need the app's state run on the UI thread (`remote.rs`); tools that only look at
//! the file system run right here in the socket thread, so they never wait for a frame.

use crate::settings::McpLevel;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

/// A tool call for the UI thread.
pub struct Req {
    pub tool: String,
    pub args: Value,
    pub reply: Sender<Result<Reply, String>>,
}

/// The answer of a tool: JSON, plus an optional PNG (screenshot).
pub struct Reply {
    pub value: Value,
    pub png: Option<Vec<u8>>,
}

impl Reply {
    pub fn json(value: Value) -> Reply {
        Reply { value, png: None }
    }
}

// ---------------------------------------------------------------------------------------------
// Level and socket
// ---------------------------------------------------------------------------------------------

static LEVEL: AtomicU8 = AtomicU8::new(0);
static SERVER_UP: AtomicBool = AtomicBool::new(false);

pub fn level() -> McpLevel {
    match LEVEL.load(Ordering::Relaxed) {
        1 => McpLevel::Read,
        2 => McpLevel::Control,
        3 => McpLevel::Full,
        _ => McpLevel::Off,
    }
}

pub fn set_level(l: McpLevel) {
    LEVEL.store(l as u8, Ordering::Relaxed);
}

pub fn server_running() -> bool {
    SERVER_UP.load(Ordering::Relaxed)
}

/// `~/Library/Application Support/myCommander/control.sock`. A Unix socket path may have at most
/// 103 bytes; for longer ones a short, per-folder name in /tmp is used instead (same rule for app
/// and `--mcp`). Whoever uses a /tmp socket checks that it belongs to the current user.
pub fn socket_path() -> PathBuf {
    let dir = crate::settings::config_dir();
    let p = dir.join("control.sock");
    if p.as_os_str().len() < 100 {
        return p;
    }
    let mut h: u32 = 0x811c9dc5;
    for b in dir.as_os_str().as_encoded_bytes() {
        h = (h ^ *b as u32).wrapping_mul(16777619);
    }
    PathBuf::from(format!("/tmp/mycmd-{}-{:08x}.sock", unsafe { libc::geteuid() }, h))
}

fn owned_by_me(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(p).map(|m| m.uid() == unsafe { libc::geteuid() }).unwrap_or(true)
}

/// Remove the socket file (at quit).
pub fn cleanup() {
    if server_running() {
        let _ = std::fs::remove_file(socket_path());
    }
}

/// Start listening, once. The thread ends (and removes the socket) when the level goes back to Off.
pub fn start_server(ctx: egui::Context, tx: Sender<Req>) {
    if SERVER_UP.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        serve(ctx, tx);
        SERVER_UP.store(false, Ordering::SeqCst);
    });
}

fn serve(ctx: egui::Context, tx: Sender<Req>) {
    let path = socket_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if !owned_by_me(&path) {
        eprintln!("myCommander: {} belongs to someone else, remote control not started", path.display());
        return;
    }
    if UnixStream::connect(&path).is_ok() {
        eprintln!("myCommander: another instance already serves {}", path.display());
        return;
    }
    let _ = std::fs::remove_file(&path);
    // Create the socket private from the start.
    let old = unsafe { libc::umask(0o077) };
    let bound = UnixListener::bind(&path);
    unsafe { libc::umask(old) };
    let Ok(listener) = bound else {
        eprintln!("myCommander: cannot create {}", path.display());
        return;
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let _ = listener.set_nonblocking(true);
    loop {
        if level() == McpLevel::Off {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let (ctx, tx) = (ctx.clone(), tx.clone());
                std::thread::spawn(move || handle_conn(stream, ctx, tx));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(60)),
        }
    }
    let _ = std::fs::remove_file(&path);
}

fn handle_conn(stream: UnixStream, ctx: egui::Context, tx: Sender<Req>) {
    let Ok(read) = stream.try_clone() else { return };
    let mut write = stream;
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = answer_line(&line, &ctx, &tx);
        if writeln!(write, "{answer}").is_err() || write.flush().is_err() {
            break;
        }
    }
}

fn answer_line(line: &str, ctx: &egui::Context, tx: &Sender<Req>) -> Value {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return json!({"id": null, "ok": false, "error": "invalid JSON"});
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let tool = v.get("tool").and_then(Value::as_str).unwrap_or("");
    let args = v.get("args").cloned().unwrap_or_else(|| json!({}));
    match run_tool(tool, &args, ctx, tx) {
        Ok(r) => {
            let mut o = json!({"id": id, "ok": true, "result": r.value});
            if let Some(png) = r.png {
                o["png_base64"] = Value::String(base64(&png));
            }
            o
        }
        Err(e) => json!({"id": id, "ok": false, "error": e}),
    }
}

fn run_tool(tool: &str, args: &Value, ctx: &egui::Context, tx: &Sender<Req>) -> Result<Reply, String> {
    let def = tools().into_iter().find(|t| t.name == tool).ok_or_else(|| format!("Unknown tool \"{tool}\". Use tools/list to see the available tools."))?;
    check_level(&def)?;
    if def.local && !(tool == "list_entries" && args.get("path").is_none()) {
        return call_local(tool, args);
    }
    let (rtx, rrx) = channel();
    tx.send(Req { tool: tool.to_string(), args: args.clone(), reply: rtx }).map_err(|_| "myCommander is shutting down".to_string())?;
    ctx.request_repaint();
    match rrx.recv_timeout(Duration::from_secs(50)) {
        Ok(r) => r,
        Err(_) => Err("myCommander did not answer within 50 s. Make sure its window is visible (not minimised or hidden) and no modal question blocks it.".into()),
    }
}

/// The error for a tool the current level does not allow (REM-1).
pub fn check_level(def: &Tool) -> Result<(), String> {
    let lvl = level();
    if lvl == McpLevel::Off {
        return Err("Remote control is switched off. In myCommander open Settings (⌘,) ▸ General ▸ Remote control and choose a level.".into());
    }
    if lvl < def.level {
        return Err(format!(
            "The tool \"{}\" needs the remote-control level \"{}\", but myCommander is set to \"{}\". The person can raise it in Settings (⌘,) ▸ General ▸ Remote control.",
            def.name,
            def.level.name(),
            lvl.name()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Tool table
// ---------------------------------------------------------------------------------------------

pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub level: McpLevel,
    pub description: &'static str,
    pub schema: Value,
    pub read_only: bool,
    pub destructive: bool,
    /// Runs in the socket thread (file system only).
    pub local: bool,
}

fn panel_prop() -> Value {
    json!({"type": "string", "enum": ["active", "other", "left", "right"], "description": "Which panel. \"active\" (default) is the one with the cursor, \"other\" the opposite one."})
}

fn names_prop(what: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": format!("File or folder names inside the panel's folder (no paths). {what}")})
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

pub fn tools() -> Vec<Tool> {
    use McpLevel::*;
    let t = |name, title, level, description, schema, read_only, destructive, local| Tool { name, title, level, description, schema, read_only, destructive, local };
    vec![
        t(
            "get_state",
            "Get the app state",
            Read,
            "Snapshot of myCommander: both panels (folder, cursor, marked names, sort order, listing format, number of entries), which panel is active, an open dialog (kind, title, text), a running job with its progress, the current screen (main, viewer, editor), the status line and the remote-control level. Call this first, and again after every change to verify what happened.",
            schema(json!({}), &[]),
            true,
            false,
            false,
        ),
        t(
            "list_entries",
            "List files and folders",
            Read,
            "Lists the entries of a panel as the person sees them (sorted, filtered), or – with \"path\" – of any folder without touching the panels. Each entry has name, type (file, folder, symlink, broken_symlink, device, special), size in bytes, modified time (UTC), mode and whether it is marked. Use offset/limit for big folders.",
            schema(
                json!({
                    "panel": panel_prop(),
                    "path": {"type": "string", "description": "Absolute folder path (\"~\" allowed) or sftp:// / ftp:// URL. If set, \"panel\" is ignored and the panels are not changed."},
                    "offset": {"type": "integer", "minimum": 0, "description": "First entry to return (default 0)."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "How many entries (default 200)."},
                    "show_hidden": {"type": "boolean", "description": "Only for \"path\": include dot files (default false)."}
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        t(
            "read_text_file",
            "Read a text file",
            Read,
            "Returns the beginning of a local text file (UTF-8, or detected legacy encodings). Fails for binary files. Use for looking into configs, logs, code, Markdown.",
            schema(
                json!({
                    "path": {"type": "string", "description": "Absolute file path (\"~\" allowed)."},
                    "max_bytes": {"type": "integer", "minimum": 1, "maximum": 1048576, "description": "How much to read (default 65536)."}
                }),
                &["path"],
            ),
            true,
            false,
            true,
        ),
        t(
            "find_files",
            "Find files",
            Read,
            "Searches below a folder by name pattern (wildcards * and ?, case-insensitive by default) and optionally by text inside the files. Stops after max_results or 10 seconds. Does not follow symlinks.",
            schema(
                json!({
                    "start": {"type": "string", "description": "Folder to search in (absolute, \"~\" allowed). Default: the home folder."},
                    "name": {"type": "string", "description": "Name pattern, e.g. \"*.pdf\" or \"report*\". Default \"*\"."},
                    "content": {"type": "string", "description": "Only files containing this text (files up to 2 MB are searched)."},
                    "case_sensitive": {"type": "boolean"},
                    "include_hidden": {"type": "boolean", "description": "Search dot folders and files too (default false)."},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 200."}
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        t(
            "places",
            "List places",
            Read,
            "Everything a panel can go to: mounted volumes (local disks, SMB/NFS shares with free space), SSH servers from ~/.ssh/config, and the person's favorite folders. Use a volume's path or a favorite with change_dir; a server name with change_dir as \"sftp://name/\".",
            schema(json!({}), &[]),
            true,
            false,
            false,
        ),
        t(
            "screenshot",
            "Take a screenshot",
            Read,
            "A PNG picture of the myCommander window, to see what the person sees (dialogs, colours, layout). Needs a visible, non-minimised window.",
            schema(json!({"max_width": {"type": "integer", "minimum": 400, "maximum": 2560, "description": "Scale the picture down to at most this width (default 1400)."}}), &[]),
            true,
            false,
            false,
        ),
        t(
            "get_help",
            "Read the built-in help",
            Read,
            "The help texts of myCommander (what it is, key list, remote control). Read \"keys\" before using press_keys.",
            schema(json!({"topic": {"type": "string", "enum": ["welcome", "keys", "remote_control", "all"], "description": "Default \"all\"."}}), &[]),
            true,
            false,
            true,
        ),
        t(
            "list_menu_items",
            "List menu items",
            Read,
            "All pull-down menu items (menu, label, shortcut). Use the menu and label with run_menu_item.",
            schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        t(
            "change_dir",
            "Change a panel's folder",
            Control,
            "Makes a panel show another folder. Accepts absolute paths, \"~\", a path relative to the panel's folder, \"..\", and sftp://host/path or ftp://host/path URLs. Returns the new state of the panel.",
            schema(json!({"panel": panel_prop(), "path": {"type": "string"}}), &["path"]),
            false,
            false,
            false,
        ),
        t(
            "set_active_panel",
            "Choose the active panel",
            Control,
            "Moves the cursor focus to the left or right panel (same as Tab).",
            schema(json!({"panel": panel_prop()}), &["panel"]),
            false,
            false,
            false,
        ),
        t(
            "set_cursor",
            "Move the cursor",
            Control,
            "Puts a panel's cursor on a file by name or by position (0 = first entry, which is \"..\" when the folder has a parent).",
            schema(json!({"panel": panel_prop(), "name": {"type": "string"}, "index": {"type": "integer", "minimum": 0}}), &[]),
            false,
            false,
            false,
        ),
        t(
            "select",
            "Mark files",
            Control,
            "Marks (selects) files in a panel – the marked files are what copy_or_move and delete use by default. Modes: \"set\" (exactly these), \"add\", \"remove\", \"toggle\", \"all\", \"none\", \"invert\". Give names and/or a wildcard pattern.",
            schema(
                json!({
                    "panel": panel_prop(),
                    "mode": {"type": "string", "enum": ["set", "add", "remove", "toggle", "all", "none", "invert"], "description": "Default \"set\"."},
                    "names": names_prop(""),
                    "pattern": {"type": "string", "description": "Wildcard such as \"*.jpg\"."},
                    "files_only": {"type": "boolean", "description": "Pattern matches files, not folders (default false)."}
                }),
                &[],
            ),
            false,
            false,
            false,
        ),
        t(
            "configure_panel",
            "Change how a panel lists",
            Control,
            "Sort order, listing format, hidden files and name filter of a panel.",
            schema(
                json!({
                    "panel": panel_prop(),
                    "sort": {"type": "string", "enum": ["name", "extension", "modified", "size", "unsorted"]},
                    "reverse": {"type": "boolean"},
                    "listing": {"type": "string", "enum": ["full", "brief", "long"]},
                    "show_hidden": {"type": "boolean", "description": "Applies to both panels, like Cmd-Shift-."},
                    "filter": {"type": "string", "description": "Wildcard that file names must match; empty string removes the filter."}
                }),
                &[],
            ),
            false,
            false,
            false,
        ),
        t(
            "press_keys",
            "Press keys",
            Control,
            "Presses keys as if the person did, one after the other: \"Down Down Enter\", \"Tab\", \"F9\", \"Cmd+D\", \"Ctrl+X Q\", \"Escape\". Names: letters, digits, F1–F12, Enter, Escape, Tab, Space, Backspace, Delete, Insert, Home, End, PageUp, PageDown, Up, Down, Left, Right, Comma, Period, Slash, Minus, Plus, plus the modifiers Ctrl+ Alt+ Shift+ Cmd+. Read get_help(\"keys\") for the key list. Below the Full level keys that change files are refused. Returns the state afterwards.",
            schema(json!({"keys": {"type": "string", "description": "Space-separated key presses."}}), &["keys"]),
            false,
            false,
            false,
        ),
        t(
            "type_text",
            "Type text",
            Control,
            "Types text into the focused input: a dialog field, the quick search (after Ctrl+S) or the command line. Does not press Enter.",
            schema(json!({"text": {"type": "string"}}), &["text"]),
            false,
            false,
            false,
        ),
        t(
            "run_menu_item",
            "Run a menu item",
            Control,
            "Runs a pull-down menu item, e.g. menu \"Command\", item \"Swap panels\" or menu \"Left\", item \"Sort order\". Get the names from list_menu_items. Dialogs it opens stay open for the person (or press_keys).",
            schema(json!({"menu": {"type": "string", "enum": ["Left", "File", "Command", "Right"]}, "item": {"type": "string"}}), &["menu", "item"]),
            false,
            false,
            false,
        ),
        t(
            "copy_or_move",
            "Copy or move files",
            Full,
            "Copies or moves files to a folder. Source: the given names, or the marked files, or the file under the cursor. Destination: \"to\", or the folder of the other panel. With the app's \"ask\" option on (default) the person approves in a dialog first; the call then waits up to ~40 s and returns status \"waiting_for_user_approval\" if undecided.",
            schema(
                json!({
                    "operation": {"type": "string", "enum": ["copy", "move"]},
                    "panel": panel_prop(),
                    "names": names_prop("Default: the marked files, else the file under the cursor."),
                    "to": {"type": "string", "description": "Destination folder (absolute, \"~\" or sftp:// URL). Default: the other panel's folder."}
                }),
                &["operation"],
            ),
            false,
            true,
            false,
        ),
        t(
            "delete",
            "Delete files",
            Full,
            "Deletes files and folders (with everything inside) for good – there is no trash. Source: the given names, or the marked files, or the file under the cursor. The person approves in a dialog first unless the app's \"ask\" option is off.",
            schema(json!({"panel": panel_prop(), "names": names_prop("Default: the marked files, else the file under the cursor.")}), &[]),
            false,
            true,
            false,
        ),
        t(
            "make_directory",
            "Create a folder",
            Full,
            "Creates a folder (several levels allowed) in a panel's folder.",
            schema(json!({"panel": panel_prop(), "name": {"type": "string"}}), &["name"]),
            false,
            false,
            false,
        ),
        t(
            "rename",
            "Rename a file or folder",
            Full,
            "Renames an entry inside its folder.",
            schema(json!({"panel": panel_prop(), "name": {"type": "string"}, "new_name": {"type": "string"}}), &["name", "new_name"]),
            false,
            true,
            false,
        ),
        t(
            "run_shell_command",
            "Run a shell command",
            Full,
            "Runs a shell command (zsh) in a panel's local folder and returns its output and exit code. Same as typing it on the command line. Needs approval unless the app's \"ask\" option is off.",
            schema(json!({"panel": panel_prop(), "command": {"type": "string"}}), &["command"]),
            false,
            true,
            false,
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// Tools that run in the socket thread
// ---------------------------------------------------------------------------------------------

fn call_local(tool: &str, args: &Value) -> Result<Reply, String> {
    let v = match tool {
        "list_entries" => list_entries_path(args)?,
        "read_text_file" => read_text_file(args)?,
        "find_files" => find_files(args)?,
        "get_help" => get_help(args),
        "list_menu_items" => list_menu_items(),
        _ => return Err(format!("Unknown tool \"{tool}\"")),
    };
    Ok(Reply::json(v))
}

pub fn arg_str<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

pub fn arg_u64(args: &Value, k: &str) -> Option<u64> {
    args.get(k).and_then(Value::as_u64)
}

pub fn arg_bool(args: &Value, k: &str) -> Option<bool> {
    args.get(k).and_then(Value::as_bool)
}

/// "~", "~/x", "/abs" or an sftp://, ftp:// URL as a path. Relative paths are refused.
pub fn resolve_abs(s: &str) -> Result<PathBuf, String> {
    let s = s.trim();
    if s.starts_with("sftp://") || s.starts_with("ftp://") {
        return Ok(crate::vfs::normalize(Path::new(s)));
    }
    if s == "~" {
        return Ok(crate::fsutil::home_dir());
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return Ok(crate::fsutil::home_dir().join(rest));
    }
    if s.starts_with('/') {
        return Ok(PathBuf::from(s));
    }
    Err(format!("\"{s}\" is not an absolute path. Use a path starting with / or ~/."))
}

pub fn kind_name(e: &crate::fsutil::Entry) -> &'static str {
    use crate::fsutil::Kind::*;
    match e.kind {
        UpDir => "parent",
        Dir => "folder",
        File => "file",
        Symlink => {
            if e.link_to_dir {
                "symlink_to_folder"
            } else {
                "symlink"
            }
        }
        StaleLink => "broken_symlink",
        Device => "device",
        Special => "special",
    }
}

pub fn entry_json(e: &crate::fsutil::Entry) -> Value {
    let modified = chrono::DateTime::from_timestamp(e.mtime, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
    let mut o = json!({
        "name": e.name,
        "type": kind_name(e),
        "size": e.size,
        "modified": modified,
        "mode": format!("{:04o}", e.mode & 0o7777),
        "marked": e.marked,
    });
    if let Some(s) = e.dir_size {
        o["folder_total_size"] = json!(s);
    }
    o
}

pub fn entries_page(entries: &[crate::fsutil::Entry], args: &Value) -> Value {
    let offset = arg_u64(args, "offset").unwrap_or(0) as usize;
    let limit = (arg_u64(args, "limit").unwrap_or(200) as usize).clamp(1, 1000);
    let list: Vec<Value> = entries.iter().filter(|e| !e.is_updir()).skip(offset).take(limit).map(entry_json).collect();
    let total = entries.iter().filter(|e| !e.is_updir()).count();
    json!({"total": total, "offset": offset, "returned": list.len(), "entries": list})
}

fn list_entries_path(args: &Value) -> Result<Value, String> {
    let path = resolve_abs(arg_str(args, "path").unwrap_or("~"))?;
    let hidden = arg_bool(args, "show_hidden").unwrap_or(false);
    let mut entries = crate::fsutil::list_dir(&path, hidden).map_err(|e| format!("Cannot read \"{}\": {e}", crate::vfs::display(&path)))?;
    entries.sort_by(|a, b| b.is_dir_like().cmp(&a.is_dir_like()).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let mut v = entries_page(&entries, args);
    v["path"] = json!(crate::vfs::display(&path));
    Ok(v)
}

fn read_text_file(args: &Value) -> Result<Value, String> {
    use std::io::Read;
    let path = resolve_abs(arg_str(args, "path").ok_or("\"path\" is required")?)?;
    if crate::vfs::is_remote(&path) {
        return Err("read_text_file reads local files only.".into());
    }
    let max = (arg_u64(args, "max_bytes").unwrap_or(65536) as usize).clamp(1, 1 << 20);
    let meta = std::fs::metadata(&path).map_err(|e| format!("Cannot read \"{}\": {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("\"{}\" is not a regular file.", path.display()));
    }
    let mut buf = Vec::new();
    std::fs::File::open(&path).and_then(|f| f.take(max as u64).read_to_end(&mut buf)).map_err(|e| format!("Cannot read \"{}\": {e}", path.display()))?;
    if buf.contains(&0) {
        return Err(format!("\"{}\" looks like a binary file.", path.display()));
    }
    let truncated = meta.len() as usize > buf.len();
    // A multi-byte character may be cut at the end of a truncated read.
    if truncated {
        while !buf.is_empty() && std::str::from_utf8(&buf).is_err() && buf.len() + 4 > max {
            buf.pop();
        }
    }
    let d = crate::textenc::decode(&buf, crate::textenc::TextEncoding::Auto);
    Ok(json!({"path": path.display().to_string(), "size": meta.len(), "truncated": truncated, "encoding": d.enc.label(), "text": d.text}))
}

fn find_files(args: &Value) -> Result<Value, String> {
    let start = resolve_abs(arg_str(args, "start").unwrap_or("~"))?;
    if crate::vfs::is_remote(&start) {
        return Err("find_files searches local folders only.".into());
    }
    let pattern = arg_str(args, "name").filter(|s| !s.is_empty()).unwrap_or("*").to_string();
    let content = arg_str(args, "content").filter(|s| !s.is_empty()).map(str::to_string);
    let case = arg_bool(args, "case_sensitive").unwrap_or(false);
    let hidden = arg_bool(args, "include_hidden").unwrap_or(false);
    let max = (arg_u64(args, "max_results").unwrap_or(200) as usize).clamp(1, 1000);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found: Vec<String> = Vec::new();
    let mut dirs = 0usize;
    let mut timed_out = false;
    let mut stack = vec![start.clone()];
    'walk: while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        dirs += 1;
        for de in rd.flatten() {
            if Instant::now() > deadline {
                timed_out = true;
                break 'walk;
            }
            let name = de.file_name().to_string_lossy().to_string();
            if !hidden && name.starts_with('.') {
                continue;
            }
            let Ok(ft) = de.file_type() else { continue };
            let p = de.path();
            if ft.is_dir() {
                // Cloud providers download files on access: leave them alone.
                if name == "CloudStorage" || name == "Mobile Documents" {
                    continue;
                }
                stack.push(p.clone());
            }
            if !crate::fsutil::wildcard_match(&pattern, &name, case) {
                continue;
            }
            if let Some(text) = &content {
                if !ft.is_file() || de.metadata().map(|m| m.len() > 2 << 20).unwrap_or(true) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&p) else { continue };
                let hay = String::from_utf8_lossy(&bytes);
                let hit = if case { hay.contains(text.as_str()) } else { hay.to_lowercase().contains(&text.to_lowercase()) };
                if !hit {
                    continue;
                }
            }
            found.push(p.display().to_string());
            if found.len() >= max {
                break 'walk;
            }
        }
    }
    found.sort();
    Ok(json!({"start": start.display().to_string(), "count": found.len(), "truncated": found.len() >= max, "timed_out": timed_out, "folders_searched": dirs, "matches": found}))
}

const HELP_WELCOME: &str = include_str!("../docs/HELP.md");
const HELP_MCP: &str = include_str!("../docs/MCP.md");

pub fn help_welcome() -> &'static str {
    HELP_WELCOME
}

/// The key table as Markdown.
pub fn help_keys() -> String {
    let mut s = String::from("# Keys\n\n");
    for (k, v) in crate::util::HELP {
        if k.is_empty() {
            s.push_str(&format!("\n## {v}\n\n"));
        } else {
            s.push_str(&format!("- `{k}` – {v}\n"));
        }
    }
    s
}

fn get_help(args: &Value) -> Value {
    let topic = arg_str(args, "topic").unwrap_or("all");
    let bin = binary_path();
    let mcp = HELP_MCP.replace("{{BIN}}", &bin);
    let text = match topic {
        "welcome" => HELP_WELCOME.to_string(),
        "keys" => help_keys(),
        "remote_control" => mcp,
        _ => format!("{HELP_WELCOME}\n\n{}\n\n{mcp}", help_keys()),
    };
    json!({"topic": topic, "text": text})
}

fn list_menu_items() -> Value {
    let mut out = Vec::new();
    for (i, title) in crate::menus::TITLES.iter().enumerate() {
        for it in crate::menus::items(i) {
            if it.action.is_some() {
                out.push(json!({"menu": title, "item": it.label, "shortcut": it.key}));
            }
        }
    }
    json!({"items": out})
}

/// Absolute path of the running binary (for copy-ready configuration snippets).
pub fn binary_path() -> String {
    std::env::current_exe().ok().map(|p| p.display().to_string()).unwrap_or_else(|| "/Applications/myCommander.app/Contents/MacOS/myCommander".into())
}

/// The Markdown of the Remote control help page, with the real binary path filled in.
pub fn mcp_help_text() -> String {
    HELP_MCP.replace("{{BIN}}", &binary_path())
}

/// `claude mcp add …` for Claude Code.
pub fn claude_code_command() -> String {
    format!("claude mcp add mycommander -- \"{}\" --mcp", binary_path())
}

/// The JSON block most MCP hosts understand (`mcpServers`).
pub fn json_config() -> String {
    let v = json!({"mcpServers": {"mycommander": {"command": binary_path(), "args": ["--mcp"]}}});
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// MCP server on stdio (`myCommander --mcp`)
// ---------------------------------------------------------------------------------------------

const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "myCommander is a two-panel file manager for macOS (like Midnight Commander). \
You work in the person's real, visible window. Start with get_state to see both panels, then use list_entries, \
change_dir, select and so on. File names are plain names inside a panel's folder. \
Read get_help(\"keys\") before pressing keys. Tools may be refused when the person chose a lower remote-control \
level in Settings; the error says how to raise it. Changes to files (Full level) may wait for the person's approval \
in the app: tell the person to look at the myCommander window. Deleted files are gone for good (no trash).";

/// Stdio loop: one JSON-RPC message per line in, one per line out.
pub fn run_mcp() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let e = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}});
                let _ = writeln!(out, "{e}");
                let _ = out.flush();
                continue;
            }
        };
        if let Some(resp) = mcp_dispatch(&msg, &call_app) {
            let _ = writeln!(out, "{resp}");
            let _ = out.flush();
        }
    }
}

/// Handle one JSON-RPC message; `None` for notifications. `call` performs a tool call in the app.
pub fn mcp_dispatch(msg: &Value, call: &dyn Fn(&str, &Value) -> Result<Reply, String>) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str)?;
    let id = msg.get("id").cloned();
    // Notifications (no id) never get an answer.
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let ok = |result: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    let err = |code: i64, message: String| Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    match method {
        "initialize" => {
            let want = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = PROTOCOLS.iter().find(|p| **p == want).copied().unwrap_or(PROTOCOLS[0]);
            ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "myCommander", "title": "myCommander file manager", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => {
            let list: Vec<Value> = tools()
                .into_iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "title": t.title,
                        "description": format!("{} [needs remote-control level: {}]", t.description, t.level.name()),
                        "inputSchema": t.schema,
                        "annotations": {"title": t.title, "readOnlyHint": t.read_only, "destructiveHint": t.destructive, "idempotentHint": t.read_only, "openWorldHint": false}
                    })
                })
                .collect();
            ok(json!({"tools": list}))
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return err(-32602, "Missing tool name".into());
            };
            if !tools().iter().any(|t| t.name == name) {
                return err(-32602, format!("Unknown tool: {name}"));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call(name, &args) {
                Ok(r) => {
                    let text = serde_json::to_string_pretty(&r.value).unwrap_or_default();
                    let mut content = vec![json!({"type": "text", "text": text})];
                    if let Some(png) = r.png {
                        content.push(json!({"type": "image", "data": base64(&png), "mimeType": "image/png"}));
                    }
                    ok(json!({"content": content, "isError": false}))
                }
                Err(e) => ok(json!({"content": [{"type": "text", "text": e}], "isError": true})),
            }
        }
        "resources/list" => ok(json!({"resources": []})),
        "prompts/list" => ok(json!({"prompts": []})),
        _ => err(-32601, format!("Method not found: {method}")),
    }
}

/// Forward a tool call to the running app over the socket.
fn call_app(tool: &str, args: &Value) -> Result<Reply, String> {
    let path = socket_path();
    let not_running = || {
        format!(
            "myCommander is not running, or remote control is switched off. Start myCommander, then open Settings (⌘,) ▸ General ▸ Remote control and choose a level. (Looked for {})",
            path.display()
        )
    };
    if !owned_by_me(&path) {
        return Err(format!("{} does not belong to you; refusing to talk to it.", path.display()));
    }
    let mut s = UnixStream::connect(&path).map_err(|_| not_running())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(70)));
    let req = json!({"id": 1, "tool": tool, "args": args});
    writeln!(s, "{req}").map_err(|e| format!("Cannot talk to myCommander: {e}"))?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| format!("myCommander did not answer: {e}"))?;
    let v: Value = serde_json::from_str(&line).map_err(|_| "myCommander sent an unreadable answer".to_string())?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        let png = v.get("png_base64").and_then(Value::as_str).and_then(unbase64);
        Ok(Reply { value: v.get("result").cloned().unwrap_or(Value::Null), png })
    } else {
        Err(v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string())
    }
}

fn unbase64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for c in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, b) in c.iter().enumerate() {
            n |= val(*b)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if c.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if c.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 7, 9][..]] {
            assert_eq!(unbase64(&base64(data)).unwrap(), data);
        }
        assert_eq!(base64(b"Man"), "TWFu");
    }

    #[test]
    fn tool_table_is_sane() {
        let t = tools();
        let mut names: Vec<_> = t.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), t.len(), "duplicate tool names");
        for tool in &t {
            assert!(tool.description.len() > 30, "{} needs a real description", tool.name);
            assert_eq!(tool.schema["type"], "object");
            // destructive tools are Full only
            if tool.destructive {
                assert_eq!(tool.level, McpLevel::Full, "{}", tool.name);
            }
            // read-only tools need no more than Read
            if tool.read_only {
                assert_eq!(tool.level, McpLevel::Read, "{}", tool.name);
            }
        }
    }

    fn fake(_: &str, _: &Value) -> Result<Reply, String> {
        Ok(Reply::json(json!({"hello": "world"})))
    }

    #[test]
    fn mcp_handshake_and_calls() {
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}});
        let r = mcp_dispatch(&init, &fake).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "myCommander");
        // unknown version -> our latest
        let init = json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}});
        assert_eq!(mcp_dispatch(&init, &fake).unwrap()["result"]["protocolVersion"], "2025-06-18");
        // notification: no answer
        assert!(mcp_dispatch(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), &fake).is_none());
        let list = mcp_dispatch(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}), &fake).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 15);
        let call = mcp_dispatch(&json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "get_state", "arguments": {}}}), &fake).unwrap();
        assert_eq!(call["result"]["isError"], false);
        assert!(call["result"]["content"][0]["text"].as_str().unwrap().contains("world"));
        let bad = mcp_dispatch(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "nope"}}), &fake).unwrap();
        assert!(bad.get("error").is_some());
        let unknown = mcp_dispatch(&json!({"jsonrpc": "2.0", "id": 6, "method": "foo/bar"}), &fake).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn tool_errors_are_tool_results() {
        let failing = |_: &str, _: &Value| -> Result<Reply, String> { Err("no way".into()) };
        let r = mcp_dispatch(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "get_state"}}), &failing).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "no way");
    }

    #[test]
    fn paths() {
        assert!(resolve_abs("relative/x").is_err());
        assert_eq!(resolve_abs("/tmp").unwrap(), PathBuf::from("/tmp"));
        assert!(resolve_abs("~/x").unwrap().ends_with("x"));
    }

    #[test]
    fn find_and_read() {
        let d = std::env::temp_dir().join(format!("mycmd-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), "hello needle world").unwrap();
        std::fs::write(d.join("sub/b.txt"), "nothing").unwrap();
        std::fs::write(d.join("sub/c.md"), "needle").unwrap();
        let r = find_files(&json!({"start": d.display().to_string(), "name": "*.txt", "content": "NEEDLE"})).unwrap();
        assert_eq!(r["count"], 1);
        let r = find_files(&json!({"start": d.display().to_string()})).unwrap();
        assert_eq!(r["count"], 4); // a.txt, sub, sub/b.txt, sub/c.md (the folder `sub` matches "*" too)
        let t = read_text_file(&json!({"path": d.join("a.txt").display().to_string(), "max_bytes": 5})).unwrap();
        assert_eq!(t["text"], "hello");
        assert_eq!(t["truncated"], true);
        let l = list_entries_path(&json!({"path": d.display().to_string()})).unwrap();
        assert_eq!(l["total"], 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
