# Welcome to myCommander

**myCommander** is a two-panel file manager for the Mac, modelled on the classic *Midnight Commander*:
two folders side by side, everything on the keyboard, copy and move from one panel to the other.
It looks like a Mac app, but it works like the orthodox file managers you may know from Norton Commander.

## The first five minutes

- **Two panels.** The one with the cursor is *active*. **Tab** switches. **Enter** opens a folder (or a file with its Mac app); **Backspace**/**Cmd-↑** goes up.
- **Mark files** with **Insert** or **Space** (or right-click). Marked files are what the next command works on; without marks it is the file under the cursor.
- **F5 copy, F6 move/rename, F7 new folder, F8 delete.** Copy and move go to the *other* panel's folder – that is the whole idea.
- **F3 view, F4 edit.** F3 shows text, images, PDFs; F4 opens a small editor.
- **Cmd-D (Places)** jumps anywhere: all mounted disks and network shares (with free space and *Eject*), your SSH servers and your favorite folders.
- **Type a command** at the bottom and press Enter: it runs in the active panel's folder. **Ctrl-O** shows its output.
- **F9** opens the menu bar, **Cmd-,** opens Settings, **F1** this help.
- **LCARS look.** The button **LCARS** right of *Settings* switches the whole app to an LCARS interface (elbows, colour segments, the function keys as a side column); **CLASSIC** in the same place switches back. Four colour themes under Settings ▸ Appearance.

## Good to know

- **Folder sizes** appear in the Size column by themselves, calculated in the background (switch off in Settings ▸ Panels).
- **Servers.** *Left/Right ▸ SFTP link…* opens a server from your `~/.ssh/config` like a local folder: browse, view, edit, copy between server and Mac. FTP uses `~/.netrc`.
- **Quick view** (Left/Right ▸ Quick view) shows the file under the cursor in the other panel – images, Markdown, code with colours.
- **Updates.** When a new version exists, a **⬆ Update** button appears in the toolbar and installs it in one click. *Help ▸ Check for Updates…* asks right away.
- **Settings** (Cmd-,) apply immediately. *Command ▸ Save setup* remembers your folders.

## Let an AI agent help

myCommander can be driven by AI agents (Claude, Cursor, Codex, …) through the *Model Context Protocol*.
The agent sees and uses the same panels you do, and you watch it work. It is **off by default**.
Open the **Remote control** page of this help (arrow keys ▸) for a step-by-step guide.

## About

myCommander is free software by **Kay Mühlena** – <https://github.com/say-kaj/MyCommander>.
Bug reports and ideas are welcome there.
