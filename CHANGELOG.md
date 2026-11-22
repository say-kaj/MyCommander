# Changelog

## 1.4.1
- Fix: a double-click (or Enter) on a document with an execute bit – typical on network shares – opened nothing or ran it as a command. Only real programs (Mach-O binaries and `#!` scripts) are run; everything else opens with its default app (SPEC PNL-17).
- **Right-click shows a Finder-like context menu** (open, Quick Look, show in Finder, Copy, Move, Rename, Move to Trash, Delete, Compress, Copy Path, Mark, New Folder …) instead of marking. Cmd-click, Space and Insert still mark.
- About shows the GitHub address; new *Help ▸ myCommander on GitHub…*.
- LCARS look: the version is shown once (footer), not twice.

## 1.4.0
- **LCARS look (SPEC §15).** A button right of *Settings* switches the whole app to an LCARS interface built from the author's LCARS design system: elbows, colour segments with a 5 px gutter, the ten function keys as a side column, command line and status bar in the footer, panels with header bands, accent-pill cursor and dot markers, the Antonio typeface, LCARS dialogs. Four palettes (TNG, Nemesis, Picard, Red alert) in Settings ▸ Appearance; **CLASSIC** switches back. The choice is saved. The LCARS look hides the macOS window buttons and draws its own as square segments in the header bar (minimise, maximise, close; LCR-9).

## 1.3.0
- **Remote control for AI agents (MCP, SPEC §14).** Settings ▸ General ▸ Remote control (off by default; levels Read only / Control / Full) lets agents such as Claude Code, Claude Desktop, Cursor, VS Code, Codex or Gemini CLI look at and use the running app through `myCommander --mcp`: 21 tools (state, listings, search, screenshots, navigation, marking, keys, menu items, copy/move/delete/rename/mkdir, shell). File changes show an Allow/Deny question first; below *Full* everything that changes files is refused; agents can never open Settings or answer the question themselves.
- **Help rebuilt:** pages *Welcome*, *Keys* and *Remote control* with a step-by-step setup for Claude Code, Claude Desktop, Cursor, VS Code, Codex, Gemini CLI, Windsurf and others, copy buttons for the command and JSON, and a live status. The same texts live in `docs/HELP.md` and `docs/MCP.md`.
- **About** names the author and links to the GitHub project; the macOS About panel shows the real version.
- **Help ▸ Check for Updates…** checks for a new release on demand and reports the result (SPEC UPD-6).

## 1.2.6
- Mouse wheel direction is now a three-way choice in Settings ▸ Panels: like the system, turn towards me = down (default), turn towards me = up (SPEC PNL-15).

## 1.2.5
- Fix: turning the mouse wheel froze the app (busy cursor) in 1.2.4; the wheel handling no longer locks the UI context.

## 1.2.4
- **Places** (⌘D) replaces the Directory hotlist: one list of all mounted volumes (local disks, SMB/NFS shares wherever they are mounted, with free space and *Eject*), SSH servers from `~/.ssh/config` and your favorites. Time Machine volumes are hidden unless enabled in Settings ▸ Panels (SPEC PNL-16).
- Mouse wheel: on a notched mouse, turning the wheel towards you moves the cursor down regardless of the system "natural scrolling" setting; switchable in Settings ▸ Panels (SPEC PNL-15). Trackpads are unchanged.

## 1.2.3
- Fix: after copy/move/delete the panels showed the new files only after the next mouse move or key press; they now refresh immediately.
- Faster copy/move/delete: the file count before the job uses the fast bulk scan, files on the same APFS volume are cloned instantly (like Finder), and background folder sizes pause while a job runs (SPEC OPS-6a).

## 1.2.2
- Fix: macOS forgot the folder access you had granted after every update. Releases are now signed with a stable identity (SPEC UPD-5). Grants have to be given once more after the first update to this version.
- The update check repeats every 30 minutes, so the Update button also appears without restarting the app.

## 1.2.1
- The *Options* menu is gone: a direct **Settings** button ends the menu bar (⌘, still works); *Save setup* moved to the Command menu.

## 1.2.0
- One **Settings** window (⌘,) with tabs General, Panels, Appearance and Quick View replaces the six separate Options dialogs; changes apply immediately, duplicated options are gone.
- Removed the unusable menu items *Display bits*, *Learn keys*, *Screen list*, *Edit extension file* and *Edit highlighting group file* (terminal-only).
- In-app updates: a **⬆ Update to X.Y.Z** button appears in the toolbar when a newer release exists and installs it inline (SPEC §13); can be switched off in Settings ▸ General.
- Folder sizes are calculated in the background (`getattrlistbulk`, cached, revalidated) and shown in the Size column; configurable in Settings ▸ Panels, default on (SPEC PNL-12a).

## 1.1.1
- Way back from a server: `..` at a server root, a "Local" crumb in the path bar and Left/Right ▸ Local folder lead back to the local file system.
- Fix: SFTP was very slow because every operation opened a new ssh connection. One multiplexed connection per host is now reused; listings are cached for 5 s and Quick view of remote files is debounced.

## 1.1.0
- Slim toolbar: only *Find files* and *Show/Hide Hidden*.
- Quick view shows images (incl. SVG, HEIC, PDF page 1) as images, renders Markdown and syntax-highlights source, HTML, shell and config files; configurable in Options ▸ Quick view.
- SFTP and FTP links (SPEC §5a): browse, view, edit, copy, move, mkdir and delete on servers, using `~/.ssh/config` + keys for SFTP and `~/.netrc` for FTP.
- `myCommander sftp://host/path` start arguments.
- Left/Right ▸ Encoding…: text encoding (automatic, UTF-8, Windows-1252, ISO-8859-15, Mac Roman, Windows-1250/1251, KOI8-R, UTF-16) for viewer, editor and Quick view; the editor saves in the original encoding.
- Fix: the Dock showed eframe's default "E" instead of the app icon; the icon is now set explicitly.
- Removed the unusable menu items *Shell link*, *Active VFS list* and *Virtual FS*.

## 1.0.1
- App icon (macOS squircle, Norton/Midnight Commander blue); `scripts/make_icon.py` regenerates it.
- `scripts/package.sh` builds the `.app` bundle and release zip.
- Install instructions (Homebrew tap) in the README.
- Documented Apple Silicon (arm64) as the only supported platform.

## 1.0.0

First release. Native macOS two-panel file manager modelled on Midnight Commander 4.8.33
mechanics, with a Finder-style presentation. See `docs/SPEC.md`.
