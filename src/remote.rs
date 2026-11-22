//! App side of the remote control (SPEC §14): tool handlers that run on the UI thread, between
//! frames, plus the guard rails and the approval dialog for file changes.

use crate::app::{Action, App, Dialog, Job, JobKind, Screen};
use crate::control::{self, arg_bool, arg_str, arg_u64, Reply, Req};
use crate::fsutil::{self, Kind};
use crate::ops::{self, OpKind};
use crate::panel::{Listing, PanelView, SortMode};
use crate::settings::McpLevel;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

type ReplyTx = Sender<Result<Reply, String>>;

/// What an approved file change does.
#[derive(Clone)]
pub enum Plan {
    CopyMove { kind: OpKind, sources: Vec<PathBuf>, dest: PathBuf },
    Delete { paths: Vec<PathBuf> },
    Mkdir { path: PathBuf },
    Rename { from: PathBuf, to: PathBuf },
    Shell { command: String, dir: PathBuf },
}

enum Kind2 {
    /// Key presses are being played; answer when the queue is empty and the UI has settled.
    Keys { settle: u8 },
    /// Frames to wait before answering with the new state.
    Settle { frames: u8 },
    Screenshot { requested: bool, max_width: u32 },
    Approval { id: u64, plan: Plan },
    Job { snapshot: Option<Value> },
}

struct Pending {
    reply: Option<ReplyTx>,
    deadline: Instant,
    kind: Kind2,
}

pub struct RemoteState {
    pub tx: Sender<Req>,
    rx: Receiver<Req>,
    pending: Vec<Pending>,
    keys: VecDeque<KeyPress>,
    next_id: u64,
    decisions: Vec<(u64, bool)>,
    /// Level of the agent whose input is being processed in this frame (guard rails, REM-5).
    pub guard: Option<McpLevel>,
}

impl Default for RemoteState {
    fn default() -> Self {
        let (tx, rx) = channel();
        RemoteState { tx, rx, pending: Vec::new(), keys: VecDeque::new(), next_id: 1, decisions: Vec::new(), guard: None }
    }
}

enum Out {
    Done(Result<Reply, String>),
    Wait(Kind2, u64),
}

fn done(v: Value) -> Out {
    Out::Done(Ok(Reply::json(v)))
}

fn fail(e: impl Into<String>) -> Out {
    Out::Done(Err(e.into()))
}

// ---------------------------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct KeyPress {
    pub key: egui::Key,
    pub mods: egui::Modifiers,
    pub text: Option<String>,
    pub label: String,
}

fn key_by_name(n: &str) -> Option<egui::Key> {
    use egui::Key::*;
    let l = n.to_lowercase();
    let k = match l.as_str() {
        "enter" | "return" => Enter,
        "esc" | "escape" => Escape,
        "tab" => Tab,
        "space" => Space,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "insert" | "ins" => Insert,
        "home" => Home,
        "end" => End,
        "pageup" | "pgup" => PageUp,
        "pagedown" | "pgdn" | "pgdown" => PageDown,
        "up" | "arrowup" => ArrowUp,
        "down" | "arrowdown" => ArrowDown,
        "left" | "arrowleft" => ArrowLeft,
        "right" | "arrowright" => ArrowRight,
        "comma" | "," => Comma,
        "period" | "dot" | "." => Period,
        "slash" | "/" => Slash,
        "backslash" | "\\" => Backslash,
        "minus" | "-" => Minus,
        "plus" | "+" => Plus,
        "equals" | "=" => Equals,
        "questionmark" | "?" => Questionmark,
        "exclamationmark" | "!" => Exclamationmark,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        _ => return egui::Key::from_name(&n.to_uppercase()).or_else(|| egui::Key::from_name(n)),
    };
    Some(k)
}

/// "Down Down Enter", "Cmd+D", "Ctrl+X Q" → key presses.
pub fn parse_keys(spec: &str) -> Result<Vec<KeyPress>, String> {
    let mut out = Vec::new();
    for tok in spec.split(|c: char| c.is_whitespace() || c == ',' && spec.contains(' ')).filter(|t| !t.is_empty()) {
        // A lone "+" or "-" is a key, not a separator.
        let parts: Vec<&str> = if tok == "+" || tok == "-" { vec![tok] } else { tok.split('+').collect() };
        let (key_name, mod_names) = parts.split_last().ok_or("empty key")?;
        let mut mods = egui::Modifiers::NONE;
        for m in mod_names {
            match m.to_lowercase().as_str() {
                "ctrl" | "control" => mods.ctrl = true,
                "alt" | "option" | "opt" => mods.alt = true,
                "shift" => mods.shift = true,
                "cmd" | "command" | "super" | "meta" => {
                    mods.mac_cmd = true;
                    mods.command = true;
                }
                other => return Err(format!("Unknown modifier \"{other}\" in \"{tok}\". Use Ctrl+, Alt+, Shift+, Cmd+.")),
            }
        }
        let key = key_by_name(key_name).ok_or_else(|| format!("Unknown key \"{key_name}\". Examples: Enter, Escape, Tab, Up, F5, a, Cmd+D."))?;
        let plain = !mods.ctrl && !mods.alt && !mods.mac_cmd;
        let text = if plain {
            let name = key.name();
            if key == egui::Key::Space {
                Some(" ".to_string())
            } else if name.chars().count() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                Some(if mods.shift { name.to_uppercase() } else { name.to_lowercase() })
            } else {
                match key {
                    egui::Key::Comma => Some(",".into()),
                    egui::Key::Period => Some(".".into()),
                    egui::Key::Slash => Some("/".into()),
                    egui::Key::Minus => Some("-".into()),
                    egui::Key::Plus => Some("+".into()),
                    egui::Key::Equals => Some("=".into()),
                    _ => None,
                }
            }
        } else {
            None
        };
        out.push(KeyPress { key, mods, text, label: tok.to_string() });
    }
    if out.is_empty() {
        return Err("No keys given.".into());
    }
    if out.len() > 200 {
        return Err("At most 200 key presses per call.".into());
    }
    Ok(out)
}

/// Keys that change files, run programs or leave the app (REM-5). `level` below Full refuses them.
fn key_restricted(k: &KeyPress) -> Option<&'static str> {
    use egui::Key::*;
    let m = k.mods;
    let cmd = m.mac_cmd;
    let none = !m.ctrl && !m.alt && !m.shift && !cmd;
    match k.key {
        F2 if none => Some("F2 (user menu) runs commands"),
        F4 => Some("F4 edits files"),
        F5 | F6 | F7 | F8 if none => Some("F5–F8 change files"),
        F10 if none => Some("F10 quits"),
        Backspace if cmd => Some("Cmd+Backspace deletes"),
        X if m.ctrl => Some("Ctrl+X opens commands that change files"),
        N if cmd && m.shift => Some("Cmd+Shift+N creates folders"),
        Q if cmd => Some("Cmd+Q quits"),
        E if cmd => Some("Cmd+E ejects volumes"),
        _ => None,
    }
}

/// Actions that change files, run programs or leave the app.
fn action_restricted(a: Action, level: McpLevel) -> bool {
    if matches!(a, Action::Config) {
        return true; // an agent never opens Settings: it could raise its own level
    }
    if level >= McpLevel::Full {
        return false;
    }
    matches!(
        a,
        Action::Edit
            | Action::EditNew
            | Action::Copy
            | Action::Move
            | Action::Mkdir
            | Action::Delete
            | Action::Chmod
            | Action::Chown
            | Action::AdvChown
            | Action::Link
            | Action::Symlink
            | Action::RelSymlink
            | Action::EditSymlink
            | Action::UserMenu
            | Action::UserCommand(_)
            | Action::ExternalPanelize
            | Action::EditMenuFile
            | Action::Zip
            | Action::Quit
            | Action::SaveSetup
            | Action::OpenTerminal
    )
}

impl App {
    /// May an agent type into / act on the current screen? Err = the explanation.
    fn remote_screen_ok(&self, level: McpLevel, keys: &[KeyPress]) -> Result<(), String> {
        if let Some(Dialog::RemoteAsk { .. }) = self.dialog {
            return Err("A permission question is open. Only the person can answer it (Return = Allow, Escape = Deny). Tell them to look at the myCommander window.".into());
        }
        if keys.iter().any(|k| k.key == egui::Key::Comma && k.mods.mac_cmd) {
            return Err("Cmd+, would open Settings. Agents cannot change settings.".into());
        }
        let only_esc = keys.iter().all(|k| k.key == egui::Key::Escape);
        if matches!(self.dialog, Some(Dialog::Options { .. })) && !only_esc {
            return Err("The Settings window is open. Agents cannot change settings; press Escape to close it.".into());
        }
        if level >= McpLevel::Full {
            return Ok(());
        }
        if matches!(self.screen, Screen::Editor(_)) {
            return Err("The editor is open. Below the Full level agents cannot type into it.".into());
        }
        let safe = matches!(
            self.dialog,
            None | Some(Dialog::Message { .. }) | Some(Dialog::Help { .. }) | Some(Dialog::About) | Some(Dialog::Hotlist { .. }) | Some(Dialog::History { .. }) | Some(Dialog::FindResults { .. })
                | Some(Dialog::Filter { .. }) | Some(Dialog::Sort { .. }) | Some(Dialog::Listing { .. }) | Some(Dialog::SelectGroup { .. }) | Some(Dialog::Encoding { .. })
                | Some(Dialog::Remote { .. })
        );
        if !safe && !only_esc {
            return Err("A confirmation or input dialog that changes files is open. Below the Full level only Escape works; the person decides.".into());
        }
        for k in keys {
            if let Some(why) = key_restricted(k) {
                return Err(format!("\"{}\" is refused at the \"{}\" level ({why}). The person can raise the level in Settings ▸ General ▸ Remote control.", k.label, level.name()));
            }
        }
        // Enter would run a typed command or a program.
        if keys.iter().any(|k| k.key == egui::Key::Enter && k.mods == egui::Modifiers::NONE) {
            if self.dialog.is_none() && !self.cmdline.trim().is_empty() {
                return Err("Enter would run the text on the command line, which needs the Full level.".into());
            }
            if self.dialog.is_none() && self.panels[self.active].current().map(|e| e.is_executable() && e.kind == Kind::File).unwrap_or(false) {
                return Err("Enter would run a program, which needs the Full level.".into());
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------------
    // Per-frame work
    // -----------------------------------------------------------------------------------------

    pub fn remote_poll(&mut self, ctx: &egui::Context) {
        if self.screenshot.is_some() {
            return;
        }
        self.remote.guard = None;
        control::set_level(self.settings.mcp_level);
        if self.settings.mcp_level != McpLevel::Off && !control::server_running() {
            control::start_server(ctx.clone(), self.remote.tx.clone());
        }
        // New calls.
        while let Ok(req) = self.remote.rx.try_recv() {
            let Req { tool, args, reply } = req;
            match self.remote_exec(ctx, &tool, &args) {
                Out::Done(r) => {
                    let _ = reply.send(r);
                }
                Out::Wait(kind, secs) => self.remote.pending.push(Pending { reply: Some(reply), deadline: Instant::now() + Duration::from_secs(secs), kind }),
            }
            ctx.request_repaint();
        }
        // Play one key per frame (several in one frame would be lost by the key handlers).
        if let Some(k) = self.remote.keys.pop_front() {
            self.remote.guard = Some(control::level());
            ctx.input_mut(|i| {
                i.events.push(egui::Event::Key { key: k.key, physical_key: Some(k.key), pressed: true, repeat: false, modifiers: k.mods });
                if let Some(t) = &k.text {
                    i.events.push(egui::Event::Text(t.clone()));
                }
                i.events.push(egui::Event::Key { key: k.key, physical_key: Some(k.key), pressed: false, repeat: false, modifiers: k.mods });
            });
            ctx.request_repaint();
        }
        self.remote_advance(ctx);
    }

    fn remote_advance(&mut self, ctx: &egui::Context) {
        let mut pending = std::mem::take(&mut self.remote.pending);
        let decisions = std::mem::take(&mut self.remote.decisions);
        let now = Instant::now();
        let keys_left = !self.remote.keys.is_empty();
        let mut keep: Vec<Pending> = Vec::new();
        for mut p in pending.drain(..) {
            let mut answer: Option<Result<Reply, String>> = None;
            let mut next: Option<Out> = None;
            match &mut p.kind {
                Kind2::Keys { settle } => {
                    if keys_left {
                        *settle = 2;
                    } else if *settle > 0 {
                        *settle -= 1;
                    } else {
                        answer = Some(Ok(Reply::json(self.state_json())));
                    }
                }
                Kind2::Settle { frames } => {
                    if *frames > 0 {
                        *frames -= 1;
                    } else {
                        answer = Some(Ok(Reply::json(self.state_json())));
                    }
                }
                Kind2::Screenshot { requested, max_width } => {
                    if !*requested {
                        *requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                    } else {
                        let shot = ctx.input(|i| i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
                        if let Some(img) = shot {
                            answer = Some(encode_screenshot(&img, *max_width));
                        }
                    }
                    ctx.request_repaint();
                }
                Kind2::Approval { id, plan } => {
                    if let Some((_, allow)) = decisions.iter().find(|(d, _)| d == id) {
                        if *allow {
                            next = Some(self.remote_execute(ctx, plan.clone()));
                        } else {
                            answer = Some(Err("The person denied this change in the app. Nothing was done.".into()));
                        }
                    } else if !matches!(self.dialog, Some(Dialog::RemoteAsk { id: d, .. }) if d == *id) {
                        // The dialog is gone without a decision (e.g. replaced): treat as denied.
                        answer = Some(Err("The permission question was closed without an answer. Nothing was done.".into()));
                    }
                }
                Kind2::Job { snapshot } => {
                    if snapshot.is_none() {
                        if let Some(j) = &self.job {
                            let g = j.shared.lock().unwrap();
                            if g.finished {
                                *snapshot = Some(job_json(&g, "finished"));
                            }
                        }
                    }
                    if self.job.is_none() {
                        let v = snapshot.take().unwrap_or_else(|| json!({"status": "finished"}));
                        answer = Some(Ok(Reply::json(v)));
                    }
                }
            }
            if let Some(Out::Wait(kind, secs)) = next {
                p.kind = kind;
                p.deadline = now + Duration::from_secs(secs);
                keep.push(p);
                continue;
            }
            if let Some(Out::Done(r)) = next {
                answer = Some(r);
            }
            if let Some(r) = answer {
                if let Some(tx) = p.reply.take() {
                    let _ = tx.send(r);
                }
                continue;
            }
            if now > p.deadline {
                // Answer in time (agent hosts time out): say what is going on and keep working.
                let msg = match &p.kind {
                    Kind2::Approval { .. } => Some(Ok(Reply::json(json!({
                        "status": "waiting_for_user_approval",
                        "note": "The person has not decided yet. A question is open in the myCommander window. Ask them to allow or deny it, then check get_state."
                    })))),
                    Kind2::Job { .. } => {
                        let v = self.job.as_ref().map(|j| job_json(&j.shared.lock().unwrap(), "running")).unwrap_or_else(|| json!({"status": "running"}));
                        Some(Ok(Reply::json(v)))
                    }
                    Kind2::Screenshot { .. } => Some(Err("No screenshot arrived. Is the myCommander window visible and not minimised?".into())),
                    Kind2::Keys { .. } | Kind2::Settle { .. } => Some(Ok(Reply::json(self.state_json()))),
                };
                if let (Some(r), Some(tx)) = (msg, p.reply.take()) {
                    let _ = tx.send(r);
                }
                // An approval stays alive (without a listener) so that a late "Allow" still works.
                if matches!(p.kind, Kind2::Approval { .. }) && now < p.deadline + Duration::from_secs(600) {
                    keep.push(p);
                }
                continue;
            }
            keep.push(p);
        }
        // Approvals that lost their listener: drop them after a long while.
        keep.retain(|p| !(matches!(p.kind, Kind2::Approval { .. }) && p.reply.is_none() && now > p.deadline + Duration::from_secs(600)));
        self.remote.pending = keep;
        if !self.remote.pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// The person answered a permission dialog.
    pub fn remote_decide(&mut self, ctx: &egui::Context, id: u64, allow: bool) {
        self.remote.decisions.push((id, allow));
        ctx.request_repaint();
    }

    // -----------------------------------------------------------------------------------------
    // Tools
    // -----------------------------------------------------------------------------------------

    fn panel_of(&self, args: &Value) -> Result<usize, String> {
        match arg_str(args, "panel").unwrap_or("active") {
            "active" => Ok(self.active),
            "other" => Ok(1 - self.active),
            "left" => Ok(0),
            "right" => Ok(1),
            x => Err(format!("Unknown panel \"{x}\". Use active, other, left or right.")),
        }
    }

    /// A path typed for a panel: absolute, "~", URL, or relative to the panel's folder.
    fn path_in_panel(&self, i: usize, s: &str) -> PathBuf {
        let s = s.trim();
        let dir = &self.panels[i].dir;
        if s.starts_with("sftp://") || s.starts_with("ftp://") {
            return crate::vfs::normalize(Path::new(s));
        }
        if crate::vfs::is_remote(dir) && !s.starts_with('/') && s != "~" && !s.starts_with("~/") {
            return if s == ".." { crate::vfs::parent(dir).unwrap_or_else(|| dir.clone()) } else { crate::vfs::join(dir, s) };
        }
        if s == "~" {
            return fsutil::home_dir();
        }
        if let Some(r) = s.strip_prefix("~/") {
            return fsutil::home_dir().join(r);
        }
        dir.join(s)
    }

    fn entries_by_name(&self, i: usize, names: &[String]) -> Result<Vec<crate::fsutil::Entry>, String> {
        let p = &self.panels[i];
        let mut out = Vec::new();
        for n in names {
            if n.contains('/') {
                return Err(format!("\"{n}\" is a path. Give plain names from the panel's folder ({}).", crate::vfs::display(&p.dir)));
            }
            match p.entries.iter().find(|e| &e.name == n && !e.is_updir()) {
                Some(e) => out.push(e.clone()),
                None => return Err(format!("\"{n}\" is not in {} (a filter or hidden files may hide it).", crate::vfs::display(&p.dir))),
            }
        }
        Ok(out)
    }

    fn names_arg(args: &Value) -> Option<Vec<String>> {
        args.get("names").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
    }

    /// Sources of an operation: names, else the marked files, else the file under the cursor.
    fn sources_for(&self, i: usize, args: &Value) -> Result<Vec<crate::fsutil::Entry>, String> {
        let v = match Self::names_arg(args) {
            Some(n) if !n.is_empty() => self.entries_by_name(i, &n)?,
            _ => self.panels[i].selection(),
        };
        if v.is_empty() {
            return Err("Nothing to work on: give \"names\", mark files, or put the cursor on a file.".into());
        }
        Ok(v)
    }

    fn remote_exec(&mut self, ctx: &egui::Context, tool: &str, args: &Value) -> Out {
        let Some(def) = control::tools().into_iter().find(|t| t.name == tool) else { return fail(format!("Unknown tool \"{tool}\"")) };
        if let Err(e) = control::check_level(&def) {
            return fail(e);
        }
        let level = control::level();
        match tool {
            "get_state" => done(self.state_json()),
            "list_entries" => match self.panel_of(args) {
                Ok(i) => {
                    let mut v = control::entries_page(&self.panels[i].entries, args);
                    v["path"] = json!(crate::vfs::display(&self.panels[i].dir));
                    v["panel"] = json!(if i == 0 { "left" } else { "right" });
                    done(v)
                }
                Err(e) => fail(e),
            },
            "places" => done(self.places_json()),
            "screenshot" => {
                let w = arg_u64(args, "max_width").unwrap_or(1400).clamp(400, 2560) as u32;
                Out::Wait(Kind2::Screenshot { requested: false, max_width: w }, 10)
            }
            "change_dir" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let Some(path) = arg_str(args, "path") else { return fail("\"path\" is required") };
                let target = self.path_in_panel(i, path);
                match self.panels[i].chdir(&target, true) {
                    Ok(()) => {
                        self.panels[i].view = PanelView::Listing;
                        Out::Wait(Kind2::Settle { frames: 1 }, 5)
                    }
                    Err(e) => fail(e),
                }
            }
            "set_active_panel" => match self.panel_of(args) {
                Ok(i) => {
                    self.active = i;
                    Out::Wait(Kind2::Settle { frames: 1 }, 5)
                }
                Err(e) => fail(e),
            },
            "set_cursor" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let p = &mut self.panels[i];
                if let Some(n) = arg_str(args, "name") {
                    match p.entries.iter().position(|e| e.name == n) {
                        Some(ix) => p.cursor = ix,
                        None => return fail(format!("\"{n}\" is not in this panel.")),
                    }
                } else if let Some(ix) = arg_u64(args, "index") {
                    if ix as usize >= p.entries.len() {
                        return fail(format!("The panel has only {} entries (0..{}).", p.entries.len(), p.entries.len().saturating_sub(1)));
                    }
                    p.cursor = ix as usize;
                } else {
                    return fail("Give \"name\" or \"index\".");
                }
                Out::Wait(Kind2::Settle { frames: 1 }, 5)
            }
            "select" => self.tool_select(args),
            "configure_panel" => self.tool_configure(args),
            "press_keys" => {
                let Some(spec) = arg_str(args, "keys") else { return fail("\"keys\" is required") };
                let keys = match parse_keys(spec) {
                    Ok(k) => k,
                    Err(e) => return fail(e),
                };
                if let Err(e) = self.remote_screen_ok(level, &keys) {
                    return fail(e);
                }
                self.remote.keys.extend(keys);
                Out::Wait(Kind2::Keys { settle: 2 }, 25)
            }
            "type_text" => {
                let Some(text) = arg_str(args, "text") else { return fail("\"text\" is required") };
                let probe = [KeyPress { key: egui::Key::A, mods: egui::Modifiers::NONE, text: None, label: "text".into() }];
                if let Err(e) = self.remote_screen_ok(level, &probe) {
                    return fail(e);
                }
                self.remote.guard = Some(level);
                let t = text.to_string();
                if self.dialog.is_none() && self.menu.is_none() && matches!(self.screen, Screen::Main) {
                    // Main screen: the text goes to the quick search or the command line, whether or
                    // not the window has the keyboard focus.
                    if let Some(q) = self.panels[self.active].quick_search.clone() {
                        let mut q = q;
                        for ch in t.chars() {
                            q.push(ch);
                            self.panels[self.active].quick_search = Some(q.clone());
                            if !self.panels[self.active].quick_search_jump(false) {
                                q.pop();
                                self.panels[self.active].quick_search = Some(q.clone());
                            }
                        }
                    } else {
                        self.cmdline.push_str(&t);
                    }
                } else {
                    ctx.input_mut(|i| i.events.push(egui::Event::Text(t)));
                }
                Out::Wait(Kind2::Settle { frames: 2 }, 5)
            }
            "run_menu_item" => {
                let (Some(menu), Some(item)) = (arg_str(args, "menu"), arg_str(args, "item")) else { return fail("\"menu\" and \"item\" are required") };
                let Some(mi) = crate::menus::TITLES.iter().position(|t| t.eq_ignore_ascii_case(menu)) else { return fail(format!("Unknown menu \"{menu}\".")) };
                let norm = |s: &str| s.trim().trim_end_matches('…').trim_end_matches("...").trim().to_lowercase();
                let Some(a) = crate::menus::items(mi).into_iter().find(|m| m.action.is_some() && norm(m.label) == norm(item)).and_then(|m| m.action) else {
                    return fail(format!("No item \"{item}\" in the {menu} menu. Use list_menu_items."));
                };
                if action_restricted(a, level) {
                    return fail(format!("\"{menu} ▸ {item}\" is refused at the \"{}\" level (it changes files, runs programs or leaves the app).", level.name()));
                }
                if let Err(e) = self.remote_screen_ok(level, &[]) {
                    return fail(e);
                }
                self.remote.guard = Some(level);
                self.do_action(ctx, a);
                Out::Wait(Kind2::Settle { frames: 2 }, 5)
            }
            "copy_or_move" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let kind = match arg_str(args, "operation") {
                    Some("copy") => OpKind::Copy,
                    Some("move") => OpKind::Move,
                    _ => return fail("\"operation\" must be \"copy\" or \"move\"."),
                };
                let sources = match self.sources_for(i, args) {
                    Ok(s) => s,
                    Err(e) => return fail(e),
                };
                let dest = match arg_str(args, "to") {
                    Some(t) => match control::resolve_abs(t) {
                        Ok(p) => p,
                        Err(_) => self.path_in_panel(i, t),
                    },
                    None => self.panels[1 - i].dir.clone(),
                };
                let dest_ok = if crate::vfs::is_remote(&dest) { crate::vfs::is_dir(&dest) } else { dest.is_dir() };
                if !dest_ok {
                    return fail(format!("The destination folder \"{}\" does not exist. Create it with make_directory first.", crate::vfs::display(&dest)));
                }
                self.ask_or_run(ctx, Plan::CopyMove { kind, sources: sources.iter().map(|e| e.path.clone()).collect(), dest })
            }
            "delete" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                match self.sources_for(i, args) {
                    Ok(s) => self.ask_or_run(ctx, Plan::Delete { paths: s.iter().map(|e| e.path.clone()).collect() }),
                    Err(e) => fail(e),
                }
            }
            "make_directory" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let Some(name) = arg_str(args, "name").map(str::trim).filter(|s| !s.is_empty()) else { return fail("\"name\" is required") };
                let path = self.path_in_panel(i, name);
                self.ask_or_run(ctx, Plan::Mkdir { path })
            }
            "rename" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let (Some(name), Some(new)) = (arg_str(args, "name"), arg_str(args, "new_name")) else { return fail("\"name\" and \"new_name\" are required") };
                if new.contains('/') || new.trim().is_empty() {
                    return fail("\"new_name\" must be a plain name without \"/\". Use copy_or_move to move between folders.");
                }
                let e = match self.entries_by_name(i, &[name.to_string()]) {
                    Ok(mut v) => v.remove(0),
                    Err(e) => return fail(e),
                };
                let to = if crate::vfs::is_remote(&e.path) { crate::vfs::join(&self.panels[i].dir, new) } else { self.panels[i].dir.join(new) };
                self.ask_or_run(ctx, Plan::Rename { from: e.path, to })
            }
            "run_shell_command" => {
                let i = match self.panel_of(args) {
                    Ok(i) => i,
                    Err(e) => return fail(e),
                };
                let Some(cmd) = arg_str(args, "command").map(str::trim).filter(|s| !s.is_empty()) else { return fail("\"command\" is required") };
                let dir = self.panels[i].dir.clone();
                if crate::vfs::is_remote(&dir) {
                    return fail("The shell works in local folders only.");
                }
                self.ask_or_run(ctx, Plan::Shell { command: cmd.to_string(), dir })
            }
            // Handled in the socket thread; should not get here.
            _ => fail(format!("Tool \"{tool}\" is not available here.")),
        }
    }

    fn tool_select(&mut self, args: &Value) -> Out {
        let i = match self.panel_of(args) {
            Ok(i) => i,
            Err(e) => return fail(e),
        };
        let mode = arg_str(args, "mode").unwrap_or("set");
        let names = Self::names_arg(args).unwrap_or_default();
        let pattern = arg_str(args, "pattern").filter(|s| !s.is_empty());
        let files_only = arg_bool(args, "files_only").unwrap_or(false);
        let named = match self.entries_by_name(i, &names) {
            Ok(v) => v.into_iter().map(|e| e.name).collect::<Vec<_>>(),
            Err(e) => return fail(e),
        };
        let p = &mut self.panels[i];
        match mode {
            "none" => p.unmark_all(),
            "invert" => p.invert_selection(),
            "all" => {
                for e in p.entries.iter_mut().filter(|e| !e.is_updir()) {
                    e.marked = true;
                }
            }
            "set" | "add" | "remove" | "toggle" => {
                if mode == "set" {
                    p.unmark_all();
                }
                let on = mode != "remove";
                for e in p.entries.iter_mut() {
                    if named.contains(&e.name) && !e.is_updir() {
                        e.marked = if mode == "toggle" { !e.marked } else { on };
                    }
                }
                if let Some(pat) = pattern {
                    p.select_group(pat, on, files_only, false);
                }
            }
            x => return fail(format!("Unknown mode \"{x}\".")),
        }
        Out::Wait(Kind2::Settle { frames: 1 }, 5)
    }

    fn tool_configure(&mut self, args: &Value) -> Out {
        let i = match self.panel_of(args) {
            Ok(i) => i,
            Err(e) => return fail(e),
        };
        if let Some(s) = arg_str(args, "sort") {
            let mode = match s {
                "name" => SortMode::Name,
                "extension" => SortMode::Extension,
                "modified" => SortMode::MTime,
                "size" => SortMode::Size,
                "unsorted" => SortMode::Unsorted,
                x => return fail(format!("Unknown sort \"{x}\".")),
            };
            self.panels[i].sort.mode = mode;
        }
        if let Some(r) = arg_bool(args, "reverse") {
            self.panels[i].sort.reverse = r;
        }
        if let Some(l) = arg_str(args, "listing") {
            self.panels[i].listing = match l {
                "full" => Listing::Full,
                "brief" => Listing::Brief,
                "long" => Listing::Long,
                x => return fail(format!("Unknown listing \"{x}\".")),
            };
        }
        if let Some(f) = arg_str(args, "filter") {
            self.panels[i].filter = if f.is_empty() || f == "*" { None } else { Some(f.to_string()) };
        }
        if let Some(h) = arg_bool(args, "show_hidden") {
            for p in self.panels.iter_mut() {
                p.show_hidden = h;
            }
            self.settings.show_hidden = h;
        }
        self.panels[i].reload();
        Out::Wait(Kind2::Settle { frames: 1 }, 5)
    }

    /// Ask the person (default) or run at once, for a change of files (REM-5).
    fn ask_or_run(&mut self, ctx: &egui::Context, plan: Plan) -> Out {
        if !self.settings.mcp_ask {
            return self.remote_execute(ctx, plan);
        }
        if self.dialog.is_some() {
            return fail("A dialog is open in myCommander, so the person cannot be asked now. Close it first (press_keys Escape) or ask the person.");
        }
        let id = self.remote.next_id;
        self.remote.next_id += 1;
        self.dialog = Some(Dialog::RemoteAsk { id, title: plan_title(&plan), lines: plan_lines(&plan) });
        ctx.request_repaint();
        Out::Wait(Kind2::Approval { id, plan }, 40)
    }

    fn remote_execute(&mut self, ctx: &egui::Context, plan: Plan) -> Out {
        match plan {
            Plan::CopyMove { kind, sources, dest } => {
                if self.job.is_some() {
                    return fail("Another operation is still running.");
                }
                self.start_op(kind, sources, Some(dest));
                Out::Wait(Kind2::Job { snapshot: None }, 25)
            }
            Plan::Delete { paths } => {
                if self.job.is_some() {
                    return fail("Another operation is still running.");
                }
                self.start_op(OpKind::Delete, paths, None);
                Out::Wait(Kind2::Job { snapshot: None }, 25)
            }
            Plan::Mkdir { path } => {
                let r = if crate::vfs::is_remote(&path) { crate::vfs::mkdir(&path) } else { std::fs::create_dir_all(&path) };
                self.reload_both();
                match r {
                    Ok(()) => done(json!({"status": "done", "created": crate::vfs::display(&path)})),
                    Err(e) => fail(format!("Cannot create \"{}\": {e}", crate::vfs::display(&path))),
                }
            }
            Plan::Rename { from, to } => {
                let exists = if crate::vfs::is_remote(&to) { crate::vfs::exists(&to) } else { to.symlink_metadata().is_ok() };
                if exists {
                    return fail(format!("\"{}\" already exists. Nothing was renamed.", crate::vfs::base_name(&to)));
                }
                let r = if crate::vfs::is_remote(&from) { crate::vfs::rename(&from, &to) } else { std::fs::rename(&from, &to) };
                self.reload_both();
                match r {
                    Ok(()) => done(json!({"status": "done", "renamed_to": crate::vfs::base_name(&to)})),
                    Err(e) => fail(format!("Cannot rename: {e}")),
                }
            }
            Plan::Shell { command, dir } => {
                if self.job.is_some() {
                    return fail("Another operation is still running.");
                }
                self.last_output = format!("{}$ {}\n", fsutil::tilde(&dir), command);
                self.job = Some(Job { kind: JobKind::Shell, shared: ops::start_shell(command, dir) });
                ctx.request_repaint();
                Out::Wait(Kind2::Job { snapshot: None }, 25)
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // JSON views
    // -----------------------------------------------------------------------------------------

    pub fn state_json(&self) -> Value {
        let panels: Vec<Value> = (0..2)
            .map(|i| {
                let p = &self.panels[i];
                let cur = p.current().map(|e| json!({"index": p.cursor, "name": e.name, "type": control::kind_name(e)}));
                let marked: Vec<&str> = p.entries.iter().filter(|e| e.marked).map(|e| e.name.as_str()).take(200).collect();
                json!({
                    "panel": if i == 0 { "left" } else { "right" },
                    "active": i == self.active,
                    "folder": crate::vfs::display(&p.dir),
                    "remote": crate::vfs::is_remote(&p.dir),
                    "showing": match p.view { PanelView::Listing => "file list", PanelView::Info => "info", PanelView::QuickView => "quick view", PanelView::Tree => "tree" },
                    "listing": match p.listing { Listing::Full => "full", Listing::Brief => "brief", Listing::Long => "long" },
                    "sort": {"by": p.sort.mode.label(), "reverse": p.sort.reverse},
                    "show_hidden": p.show_hidden,
                    "filter": p.filter,
                    "entries": p.entries.iter().filter(|e| !e.is_updir()).count(),
                    "cursor": cur,
                    "marked_count": p.entries.iter().filter(|e| e.marked).count(),
                    "marked": marked,
                    "error": p.error,
                })
            })
            .collect();
        let dialog = self.dialog.as_ref().map(|d| match d {
            Dialog::Message { title, text, error } => json!({"kind": "message", "title": title, "text": text, "error": error}),
            Dialog::RemoteAsk { title, lines, .. } => json!({"kind": "permission_question", "title": title, "text": lines.join("\n"), "note": "Only the person can answer this."}),
            Dialog::Delete { text, .. } => json!({"kind": "confirm_delete", "text": text}),
            Dialog::CopyMove { kind, dest, .. } => json!({"kind": "copy_move", "operation": kind.label(), "destination": dest}),
            Dialog::Mkdir { name } => json!({"kind": "new_folder", "text": name}),
            Dialog::Options { .. } => json!({"kind": "settings"}),
            Dialog::Help { .. } => json!({"kind": "help"}),
            Dialog::Hotlist { .. } => json!({"kind": "places"}),
            Dialog::Find { .. } => json!({"kind": "find_files_form"}),
            Dialog::FindResults { .. } => json!({"kind": "find_files_results"}),
            _ => json!({"kind": "other"}),
        });
        let job = self.job.as_ref().map(|j| job_json(&j.shared.lock().unwrap(), "running"));
        json!({
            "app": {"name": "myCommander", "version": env!("CARGO_PKG_VERSION")},
            "remote_control": {"level": control::level().name(), "ask_before_file_changes": self.settings.mcp_ask},
            "active_panel": if self.active == 0 { "left" } else { "right" },
            "panels": panels,
            "screen": match self.screen { Screen::Main => "main", Screen::Viewer(_) => "viewer", Screen::Editor(_) => "editor", Screen::Output => "command output" },
            "dialog": dialog,
            "menu_open": self.menu.is_some(),
            "job": job,
            "status_line": self.status.as_ref().map(|(t, _)| t.clone()),
            "command_line": self.cmdline,
        })
    }

    fn places_json(&self) -> Value {
        let mounts: Vec<Value> = crate::mounts::list(self.settings.show_timemachine)
            .into_iter()
            .map(|m| json!({"name": m.name, "path": m.path.display().to_string(), "kind": m.kind(), "network": m.network, "total_bytes": m.total, "free_bytes": m.free}))
            .collect();
        let favorites: Vec<String> = self.settings.hotlist.iter().map(|p| crate::vfs::display(p)).collect();
        json!({"volumes": mounts, "servers": crate::vfs::ssh_hosts(), "favorites": favorites})
    }
}

fn plan_title(p: &Plan) -> String {
    match p {
        Plan::CopyMove { kind, .. } => format!("An agent wants to {}", kind.label().to_lowercase()),
        Plan::Delete { .. } => "An agent wants to DELETE".into(),
        Plan::Mkdir { .. } => "An agent wants to create a folder".into(),
        Plan::Rename { .. } => "An agent wants to rename".into(),
        Plan::Shell { .. } => "An agent wants to run a command".into(),
    }
}

fn plan_lines(p: &Plan) -> Vec<String> {
    // File names only; the folder is said once.
    let names = |v: &Vec<PathBuf>| -> Vec<String> {
        let mut l: Vec<String> = v.iter().take(10).map(|p| format!("  • {}", crate::vfs::base_name(p))).collect();
        if v.len() > 10 {
            l.push(format!("  … and {} more", v.len() - 10));
        }
        l
    };
    let from = |v: &Vec<PathBuf>| v.first().and_then(|p| crate::vfs::parent(p)).map(|d| fsutil::tilde(&d)).unwrap_or_default();
    match p {
        Plan::CopyMove { kind, sources, dest } => {
            let mut l = vec![format!("{} {} item(s)", kind.label(), sources.len()), format!("from  {}", from(sources)), format!("to    {}", fsutil::tilde(dest)), String::new()];
            l.extend(names(sources));
            l
        }
        Plan::Delete { paths } => {
            let mut l = vec![format!("Delete {} item(s) for good – there is no trash.", paths.len()), format!("in  {}", from(paths)), "Folders go with everything inside.".to_string(), String::new()];
            l.extend(names(paths));
            l
        }
        Plan::Mkdir { path } => vec!["Create the folder".to_string(), format!("  {}", fsutil::tilde(path))],
        Plan::Rename { from, to } => vec!["Rename".to_string(), format!("  {}", crate::vfs::base_name(from)), "to".to_string(), format!("  {}", crate::vfs::base_name(to))],
        Plan::Shell { command, dir } => vec![format!("Run in {}:", fsutil::tilde(dir)), format!("  {command}")],
    }
}

fn job_json(g: &ops::Progress, status: &str) -> Value {
    let out: String = if g.output.chars().count() > 8000 { g.output.chars().take(8000).collect::<String>() + "\n… (output cut)" } else { g.output.clone() };
    let mut v = json!({
        "status": status,
        "title": g.title,
        "current": g.current,
        "files_done": g.files_done,
        "files_total": g.files_total,
        "bytes_done": g.bytes_done,
        "bytes_total": g.bytes_total,
        "errors": g.errors,
    });
    if !g.output.is_empty() {
        v["output"] = json!(out);
    }
    if let Some(c) = g.exit_code {
        v["exit_code"] = json!(c);
    }
    if let Some(q) = &g.question {
        v["waiting_for_person"] = json!({"question": q.text, "source": q.src_info, "target": q.dst_info, "note": "An overwrite question is open in the myCommander window."});
    }
    v
}

fn encode_screenshot(img: &egui::ColorImage, max_width: u32) -> Result<Reply, String> {
    let [w, h] = img.size;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in &img.pixels {
        rgba.extend_from_slice(&p.to_array());
    }
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("bad screenshot")?;
    let mut dynimg = image::DynamicImage::ImageRgba8(buf);
    if dynimg.width() > max_width {
        let nh = (dynimg.height() as f32 * max_width as f32 / dynimg.width() as f32).round().max(1.0) as u32;
        dynimg = dynimg.resize(max_width, nh, image::imageops::FilterType::Triangle);
    }
    let (ow, oh) = (dynimg.width(), dynimg.height());
    let mut png = Vec::new();
    dynimg.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(Reply { value: json!({"width": ow, "height": oh, "bytes": png.len()}), png: Some(png) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_parse() {
        let k = parse_keys("Down Down Enter").unwrap();
        assert_eq!(k.len(), 3);
        assert_eq!(k[2].key, egui::Key::Enter);
        let k = parse_keys("Cmd+D Ctrl+X Q").unwrap();
        assert!(k[0].mods.mac_cmd && k[0].text.is_none());
        assert!(k[1].mods.ctrl && k[1].key == egui::Key::X);
        assert_eq!(k[2].text.as_deref(), Some("q"));
        let k = parse_keys("Shift+a").unwrap();
        assert_eq!(k[0].text.as_deref(), Some("A"));
        assert_eq!(parse_keys("F5").unwrap()[0].key, egui::Key::F5);
        assert_eq!(parse_keys("+").unwrap()[0].key, egui::Key::Plus);
        assert!(parse_keys("Hyper+x").is_err());
        assert!(parse_keys("Flurb").is_err());
        assert!(parse_keys("   ").is_err());
    }

    #[test]
    fn restricted_keys_and_actions() {
        for s in ["F2", "F4", "Shift+F4", "F5", "F6", "F7", "F8", "F10", "Cmd+Backspace", "Ctrl+X", "Cmd+Shift+N", "Cmd+Q", "Cmd+E"] {
            assert!(key_restricted(&parse_keys(s).unwrap()[0]).is_some(), "{s} must be restricted");
        }
        for s in ["Down", "Enter", "Tab", "F3", "F9", "Cmd+D", "Escape", "Insert", "Ctrl+S", "a"] {
            assert!(key_restricted(&parse_keys(s).unwrap()[0]).is_none(), "{s} must be allowed");
        }
        assert!(action_restricted(Action::Delete, McpLevel::Control));
        assert!(!action_restricted(Action::Delete, McpLevel::Full));
        assert!(action_restricted(Action::Config, McpLevel::Full), "agents never open Settings");
        assert!(!action_restricted(Action::Swap, McpLevel::Read));
    }
}

impl App {
    /// Guard rails (REM-5): may this action run while an agent's input is processed?
    pub fn remote_allows(&self, a: Action) -> bool {
        match self.remote.guard {
            Some(level) => !action_restricted(a, level),
            None => true,
        }
    }
}
