# Changelog

## 1.1.0
- Slim toolbar: only *Find files* and *Show/Hide Hidden*.
- Quick view shows images (incl. SVG, HEIC, PDF page 1) as images, renders Markdown and syntax-highlights source, HTML, shell and config files; configurable in Options ▸ Quick view.
- SFTP and FTP links (SPEC §5a): browse, view, edit, copy, move, mkdir and delete on servers, using `~/.ssh/config` + keys for SFTP and `~/.netrc` for FTP.
- `myCommander sftp://host/path` start arguments.
- Left/Right ▸ Encoding…: text encoding (automatic, UTF-8, Windows-1252, ISO-8859-15, Mac Roman, Windows-1250/1251, KOI8-R, UTF-16) for viewer, editor and Quick view; the editor saves in the original encoding.
- Removed the unusable menu items *Shell link*, *Active VFS list* and *Virtual FS*.

## 1.0.1
- App icon (macOS squircle, Norton/Midnight Commander blue); `scripts/make_icon.py` regenerates it.
- `scripts/package.sh` builds the `.app` bundle and release zip.
- Install instructions (Homebrew tap) in the README.
- Documented Apple Silicon (arm64) as the only supported platform.

## 1.0.0

First release. Native macOS two-panel file manager modelled on Midnight Commander 4.8.33
mechanics, with a Finder-style presentation. See `docs/SPEC.md`.
