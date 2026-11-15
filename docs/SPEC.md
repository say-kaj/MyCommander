# myCommander – Specification

Status: **v1.0** · Audience: coding agents and human contributors · Language: English

This document is the single source of truth for what myCommander is and how it
must behave. It consolidates everything learned while analysing Midnight
Commander (mc) 4.8.33 and while building the first working version.

Keywords **MUST**, **SHOULD**, **MAY** are used in the RFC 2119 sense.
Requirement IDs (e.g. `PNL-3`) are stable; reference them in commits, issues and tests.

---

## 1. Purpose and scope

**Goal.** A native macOS application that works like Midnight Commander –
two side-by-side file panels, function-key driven file operations, quick keyboard
navigation – but is a real GUI app, not a terminal program. Name: **myCommander**.

**Stack.** Rust 2021, `eframe`/`egui` 0.31 (glow backend), `muda` for the native
macOS menu bar, `egui_commonmark` (Markdown), `syntect` + `two-face` (highlighting), `image` (decoding), `resvg` (SVG). No other UI toolkit.

**Platform.** macOS on Apple Silicon (arm64) only. Intel Macs, Linux and Windows
are out of scope; release binaries are arm64 only, not universal.

**Non-goals for 1.0**
- No virtual file systems (archives, FTP, SFTP, shell links).
- No embedded subshell / terminal emulator.
- No mc-compatible config files, skins or `mc.ext.ini` rules.
- No pixel-exact imitation of mc's terminal look (see §10, U-1).

**Nature of the mc analysis.** mc was studied by reading strings, function
behaviour and the config files it ships. Nothing is copied code; this spec
describes *behaviour*. Binary addresses are given as provenance only.

---

## 2. Design principles

1. **mc mechanics, Mac presentation.** Behaviour (sorting, marking, key
   bindings, menu structure, operation rules) follows mc. Presentation follows
   macOS (Finder-style list, light/dark following the system, native menu bar,
   unified toolbar).
2. **Keyboard first, mouse friendly.** Every action reachable by a key or menu
   item; clicking, double-clicking and the toolbar also work.
3. **Never block the UI.** Copy, move, delete, directory-size, find and shell
   commands run as background jobs and report through a shared progress object.
4. **Safe by default.** Destructive actions confirm (configurable); `..` can
   never be operated on.
5. **Honest UI.** Features that need unsupported mc subsystems are shown in
   the menus but **disabled**, never silently missing or faked.

---

## 3. Screen layout (LAY)

- **LAY-1** Top to bottom: unified toolbar in the title bar → in-window menu bar
  (`Left File Command Options Right`) → two panels → command line → function-key bar.
- **LAY-1a** The toolbar contains **only** the *Find files* field and the *Show/Hide Hidden* button
  (right-aligned). Back/forward, current-folder title, View/Edit/Copy/Move/New Folder/Delete and the
  List/Compact/Details switches are intentionally absent: they are redundant with the path bar,
  the function-key bar, the menus and `Alt-T`.
- **LAY-2** The window MUST also provide a real macOS menu bar:
  `myCommander · Left · File · Command · Options · Right · Help`. Its items
  come from the same table as the in-window menu (`menus.rs`) so both always match.
- **LAY-3** Default window 1280×800, minimum 760×440.
- **LAY-4** The active panel shows the selection bar; the inactive one a dimmed one.
- **LAY-5** Layout options (Options ▸ Layout): vertical/horizontal split, show
  menu bar, command line, key bar, toolbar. Persisted.
- **LAY-6** Function-key bar: `1 Help 2 Menu 3 View 4 Edit 5 Copy 6 Move 7 New Folder 8 Delete 9 Commands 10 Quit`.
  (mc's terse labels `RenMov`, `Mkdir`, `PullDn` are intentionally spelled out.)
- **LAY-7** Appearance: light, dark or follow-system (Options ▸ Appearance);
  font size adjustable.

## 4. Panels (PNL)

### 4.1 Panel modes
- **PNL-1** Each panel shows one of: **File listing**, **Quick view**, **Info**, **Tree**.
  Quick view / Info always show data about the *other* panel's current item.
- **PNL-1a** Quick view content (`preview.rs`), chosen by file type:
  | Content | Rendering |
  |---------|-----------|
  | Image (png, jpg/jpeg/jfif, gif, webp, bmp, tif/tiff, ico) | shown as an image, scaled to fit, caption `W × H px · size`; decoded images larger than 2048 px are downscaled for display; files > 64 MB are not decoded |
  | SVG | rasterised with `resvg` (long side ≤ 2048 px, system fonts), caption `SVG · …` |
  | HEIC/HEIF, PDF | converted by macOS `sips` to a PNG (long side ≤ 2048 px); PDF shows **page 1 only** |
  | Markdown (`.md`, `.markdown`, `.mdown`) | rendered (headings, lists, links, code, tables) in a scroll area |
  | Source / HTML / shell / config | syntax highlighted with a language-matched colour scheme; language name shown top right |
  | Other text | plain monospace |
  | Binary (NUL byte in first 4 KB) | hex dump |
  | Directory | list of its entries |
- **PNL-1b** Language detection order: user override (`ext=Syntax name`) → file extension → well-known names
  (`.zshrc`, `.bashrc`, `Makefile`, `Dockerfile`, `Cargo.lock` …) → shebang / first line. Unknown types fall back to plain text.
- **PNL-1c** Quick view reads at most *N* KB (default 512) and 20 000 lines; truncation is indicated.
  Loaded content is cached per (path, mtime, size, settings) so cursor movement stays cheap.
- **PNL-1d** Quick view is configurable in **Options ▸ Quick view…** (see CFG-2): images on/off, Markdown rendering
  on/off, highlighting on/off, line numbers, light and dark highlighting theme, language overrides, read limit.
- **PNL-2** A panel may also show **panelized** results (Find file results, External panelize).
- **PNL-3** Each panel keeps: folder, entries, cursor, marks, sort order, filter,
  listing format and directory history (back/forward).

### 4.2 Listing formats
| Name  | mc format string | Notes |
|-------|------------------|-------|
| Full  | `half type name \| size \| mtime` | default |
| Brief | `half 2 type name` | two columns |
| Long  | `full perm space nlink space owner space group space size space mtime space name` | |
| User  | `half type name space bsize space perm space` | |

- **PNL-4** `Alt-T` cycles the format; Left/Right ▸ *Listing format…* selects directly.
- **PNL-5** Native presentation: the **List** view is Finder-style (icons, striped
  rows, blue selection bar, breadcrumb path bar, relative dates such as
  "Today, 14:05", human sizes "34 KB"); the **Details** view keeps mc's exact
  date formats and plain byte sizes (§4.4, §4.5).
- **PNL-6** Footer shows "N items, X available" and, when something is marked,
  `<size> in <n> file(s)`.

### 4.3 Sorting
- **PNL-7** Modes: Unsorted, Name, Version, Extension, Size, Modify time,
  Access time, Change time, Inode. Options: reverse, case-sensitive,
  executables-first, mix-all-files.
- **PNL-8** Directories always sort before files (unless *mix all files*);
  `..` always stays first. An optional "executables first" rank sits between
  directories and files.
- **PNL-9** Name comparison honours *case sensitive*. The comparison result is
  multiplied by `reverse` (±1): Reverse flips order **but not the directory grouping**.
- **PNL-10** Size and time sorts break ties by name. mc sorts ascending unless reversed
  (no implicit "largest/newest first").
- **PNL-11** Clicking a column header sorts by it (native addition).

### 4.4 Size column
- **PNL-12** `..` shows `UP--DIR`; devices show `major,minor`; in brief/mini info,
  directories show `SUB-DIR` and symlinks `SYMLINK`.
- **PNL-13** Column width is 7 characters. If the decimal number fits it is shown as is,
  otherwise divided by 1024 (rounded: `(x+512)>>10`) and suffixed `K M G T P`
  until it fits.

### 4.5 Date column
- **PNL-14** Recent format `%b %e %H:%M`; old format `%b %e  %Y` (two spaces).
- **PNL-15** A file is "old" if its mtime is more than **15 552 000 s (180 days)** in
  the past **or more than 3 600 s in the future**.

### 4.6 Marking
- **PNL-16** `..` can never be marked.
- **PNL-17** The panel tracks marked count, marked directory count and total
  bytes of marked files; the footer shows `<size> in <n> file(s)`.
- **PNL-18** `Insert` / `Ctrl-T` mark and move down; `Shift-Up/Down` mark while moving;
  `+` / `Alt-+` select group, `-` / `Alt--` unselect group, `*` invert selection.
  Right mouse button marks (estimated mc behaviour).
- **PNL-19** Operations act on the marked files; if none are marked, on the file under the cursor.

### 4.7 Mini info line
- **PNL-20** Shows the current file name; for a symlink `-> target`; for `..` `UP--DIR`.
- **PNL-21** Free space is shown as ` %s / %s (%d%%) ` (free / total, percent).

### 4.8 Navigation and quick search
- **PNL-22** `Enter` on a directory enters it; on a file opens it with the **macOS default
  app** (mc uses `mc.ext.ini` rules – not replicated).
- **PNL-23** `Ctrl-S` / `Alt-S` start quick search (incremental, in the current panel);
  `Backspace` edits, `Esc` cancels, `Enter` accepts.
- **PNL-24** Directory history: `Alt-Y` back, `Alt-U` forward.
- **PNL-25** Lynx-style motion (Left/Right = parent/enter) is an option, off by default.
- **PNL-26** Hidden files toggle (`Alt-.`), persisted.

## 5. File operations (OPS)

All run as background jobs (`ops.rs`) and show a progress window.

- **OPS-1** Prompt text, single item: `<Op> file "<name>" with source mask:` then `to:`.
  Several: `<Op> <n> files/directories with source mask:`.
- **OPS-2** Copy (F5) / Move (F6) default destination is the **other panel's folder**.
- **OPS-3** Move uses rename; across volumes it falls back to copy + delete.
- **OPS-4** Copy preserves permissions and modification time (mc "preserve attributes" default on).
- **OPS-5** Overwrite question: `Overwrite this file?` with **Yes / No / All / None / Skip**
  (and `Overwrite all files?`). Configurable via Confirmation options.
- **OPS-6** Operating on `..` is refused with `Cannot operate on ".."!`.
- **OPS-7** Mkdir (F7): dialog *Create a new Directory* / *Enter directory name:*.
- **OPS-8** Delete (F8) confirms; directories are removed recursively after confirmation.
  Confirmation is configurable. Delete is permanent (no Trash) in 1.0.
- **OPS-9** Additional operations: Chmod, Chown, Advanced chown, Link, Symlink,
  Relative symlink, Edit symlink, Select group / Unselect group (glob), Compare
  directories, Compare files (diff), Show directory sizes (current / all / marked
  via `Ctrl-Space`).
- **OPS-10** Find file (`Alt-?`, `Cmd-F`): background search; results can be
  panelized into the panel.
- **OPS-11** External panelize: run a shell command, treat each output line as a path.
- **OPS-12** Command line: `sh -c` in the active panel's folder; output shown in a window
  (`Ctrl-O`). No subshell. `Alt-Enter` puts the file name on the command line,
  `Alt-A` / `Alt-P` the path, `Alt-N` the other panel's path.
- **OPS-13** Background jobs window lists running jobs.

## 6. Menu structure (MNU)

Identical in the in-window menu bar, the macOS menu bar and the F9 pull-downs.
Items marked `*` are shown **disabled** (they require mc's virtual file systems or terminal settings).

- **Left / Right:** File listing, Quick view, Info, Tree, Listing format…, Sort order…,
  Filter…, Encoding*, FTP link*, Shell link*, SFTP link*, Panelize, Rescan.
- **File:** View, View file…, Filtered view, Edit, Copy, Chmod, Link, Symlink,
  Relative symlink, Edit symlink, Chown, Advanced chown, Rename/Move, Mkdir, Delete,
  Quick cd, Select group, Unselect group, Invert selection, Exit.
- **Command:** User menu, Directory tree, Find file, Swap panels, Switch panels on/off,
  Compare directories, Compare files, External panelize, Show directory sizes,
  Command history, Viewed/edited files history, Directory hotlist, Active VFS list*,
  Background jobs, Screen list*, Edit extension file*, Edit menu file,
  Edit highlighting group file*.
- **Options:** Configuration, Layout, Panel options, Confirmation, Appearance, Quick view (native addition),
  Display bits*, Learn keys*, Virtual FS*, Save setup.
- **MNU-1** The user menu (F2) reads `menu.txt` from the config directory; entries
  support the substitutions `%f` (current file), `%d` (current dir), `%F` / `%D`
  (same for the other panel).

## 7. Key bindings (KEY)

Taken from mc's `mc.default.keymap` (`[filemanager]`, `[panel]`), plus Mac aliases.

| Key | Action |
|-----|--------|
| Tab | switch active panel |
| F1 / F2 / F3 / F4 | help / user menu / view / edit |
| F5 / F6 / F7 / F8 | copy / move / mkdir / delete |
| F9 / F10 | menu / quit |
| Shift-F4 | edit new file |
| Ctrl-R (Cmd-R) | re-read |
| Ctrl-U | swap panels |
| Alt-. (Cmd-Shift-.) | toggle hidden files |
| Ctrl-Space | directory size |
| Alt-? (Cmd-F) | find file |
| Ctrl-\ (Cmd-D) | directory hotlist |
| Alt-T | cycle listing format |
| Ctrl-S / Alt-S | quick search |
| Insert / Ctrl-T | mark and move down |
| Home / End | top / bottom |
| PgUp / PgDn, Alt-V / Ctrl-V | page up / down |
| Ctrl-PgUp / Ctrl-PgDn | parent / enter |
| Alt-O | other panel → this directory |
| Alt-I | sync other panel to current item |
| Alt-Enter / Alt-A / Alt-P | put name / path on command line |
| Ctrl-O | show command output |
| Alt-Y / Alt-U | directory history back / forward |
| Alt-C | quick cd |
| Ctrl-X i / Ctrl-X q | info panel / quick view |
| Cmd-, | Configuration |
| Cmd-Q | quit |
| Cmd-Backspace | delete |
| Cmd-Shift-N | new folder |

- **KEY-1** Keys MUST NOT be handled while a text field or modal dialog has focus,
  except the dialog's own keys.
- **KEY-2** Viewer: `F2` wrap, `F3`/`F10`/`q`/`Esc` quit, `F4` hex, `F7` search, `n` continue search.
- **KEY-3** Editor: `F2` save, `F10`/`Esc` quit, `F7` search.

## 8. Visual design (VIS)

- **VIS-1** Native look, not terminal colours: light and dark palettes follow the system appearance.
- **VIS-2** mc's file-type groups (from `filehighlight.ini`) colour the **file icons**
  (not the names): directory, executable, stale link, device, archive, document,
  source, media, graphics, database, temp, core.
- **VIS-3** Original mc skin (for reference only, no longer drawn): panel light-gray on
  blue; cursor black on cyan; marked yellow on blue; dialogs black on light-gray;
  errors white on red; menu white on cyan; key bar hotkey white on black, label black on cyan.
  Dialogs follow mc's convention that errors and delete confirmations are red.

## 9. Persistence (CFG)

- **CFG-1** Directory: `~/Library/Application Support/myCommander/`.
- **CFG-2** `settings.json` (serde, every field `#[serde(default)]` so older files
  keep loading): panel folders and sort, listing, hidden flag, hotlist, auto-save,
  font size, command history, file history, layout flags, panel options
  (mini status, lynx motion, mix all files), confirmation flags, theme, and the Quick view options
  (`qv_images`, `qv_markdown`, `qv_highlight`, `qv_line_numbers`, `qv_max_kb`, `qv_theme_light`,
  `qv_theme_dark`, `qv_syntax_map`).
- **CFG-3** `menu.txt` – user menu file (see MNU-1).
- **CFG-4** Auto-save is on by default; *Save setup* saves explicitly.
- **CFG-5** New settings fields MUST have a default so existing user files never break.

## 10. Known unknowns and limitations

| ID | Item | Status |
|----|------|--------|
| U-1 | Exact pixel/column layout of mc's running terminal UI | Unverified. The person declined running mc in a recorded terminal; layout follows format strings in the binary (size width 7, date 12 chars). Low severity – the app deliberately uses a Mac look. |
| U-2 | Runtime behaviour vs. mechanics values | The mechanics sheet is readable and sourced (30 of 32 values with a binary/config source) but no automated comparison against a running mc exists. |
| U-3 | Estimated (not sourced) values | "Copy preserves attributes" and "right click marks". |
| L-1 | No VFS / archives / FTP / SFTP | Menu items disabled. |
| L-2 | No encoding, display-bits, learn-keys | Menu items disabled. |
| L-3 | Delete is permanent | Trash integration is a post-1.0 candidate. |
| L-5 | Quick view: PDF shows only the first page; HEIC/PDF depend on macOS `sips`; images inside Markdown are not loaded; highlighting uses syntect's built-in themes and syntax set (two-face extras) | Post-1.0 candidates. |
| L-4 | Tests | Only `ops.rs` has unit tests; panel rules lack automated tests. |

## 11. Architecture (ARC)

```
src/
  main.rs         CLI (--screenshot, --help, --version, two start folders), window setup
  app.rs          App state, Action enum, global keyboard handling
  ui_main.rs      Toolbar, menu bar, popups, command line, key bar, Info / Quick view
  panel.rs        Panel model: entries, cursor, marks, sort, history  (pure logic)
  panel_view.rs   Finder-style panel drawing
  preview.rs      Quick view: images, Markdown (egui_commonmark), syntax highlighting (syntect + two-face), cache
  fsutil.rs       Directory reading, size/date/permission formatting, highlight classes
  ops.rs          Background jobs (copy, move, delete, dirsize, find, shell) + Progress
  dialogs.rs      Standard dialogs + progress / overwrite window
  dialogs2.rs     Link, chown, filtered view, quick cd, panelize, history, Options pages
  screens.rs      Viewer (F3), editor (F4), command output, --screenshot capture
  menus.rs        The menu table (single source for both menu bars)
  native_menu.rs  macOS menu bar via muda
  settings.rs     settings.json, menu.txt, Options data
  theme.rs        Light/dark palettes, file-type colours
  util.rs         Key helpers, text shortening, fonts, widget style
```

- **ARC-1** Pure logic (sorting, marking, formatting) stays in `panel.rs` / `fsutil.rs`
  with no egui dependency so it can be unit-tested.
- **ARC-2** Anything that touches the file system for longer than a frame goes into `ops.rs`.
- **ARC-3** A new menu item is added in `menus.rs` once; both menu bars pick it up.
  Unimplemented items MUST be registered as disabled, not omitted.
- **ARC-4** Test aids: `myCommander --screenshot out.png` renders one frame headless-ish and exits;
  env `MYCMD_SHOT_MENU=<0..4>` opens menu *n* for the picture; env `MYCMD_SHOT_QV=<file>` shows that file
  in the right panel's Quick view.

## 12. Roadmap candidates (post-1.0)

Move-to-Trash, unit tests for panel rules, Quick Look integration, mc-style
extension rules for *open with*, archive browsing (read-only), configurable key map,
cross-platform support (Linux/Windows) behind the existing platform guards.

## 13. Rules for coding agents

1. Read this spec and `AGENTS.md` before changing code; cite requirement IDs in commits.
2. Behaviour in §4–§7 is normative; do not "improve" it without changing the spec first.
3. Keep `cargo build --release` warning-free and `cargo test` green.
4. Do not add dependencies without a reason noted in the PR.
5. Never run destructive file operations against real user directories in tests – use temp dirs.
