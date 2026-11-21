# Changelog

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
