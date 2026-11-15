# myCommander

A native macOS two-panel file manager that works like [Midnight Commander](https://midnight-commander.org/) – without the terminal.

![myCommander](docs/screenshots/main.png)

## Features

- Two panels, F-key driven copy / move / mkdir / delete, mc's keyboard shortcuts
- Listing formats Full / Brief / Long / User, mc sort modes, marking with size summary
- Quick view shows images as images, renders Markdown and syntax-highlights source, HTML, shell and config files (themes and language overrides configurable); Info and Tree panel modes; Find file and External panelize
- Internal viewer (text/hex) and editor, user menu, directory hotlist, history
- Real macOS menu bar with mc's exact menu structure, light/dark mode, Finder-style list
- Slim toolbar (Find files, Show/Hide Hidden) – everything else is in the menus, keys and function-key bar
- Background jobs – the UI never blocks

See [`docs/SPEC.md`](docs/SPEC.md) for the full specification and known limitations.

## Install

Apple Silicon (M-series) Macs only. With [Homebrew](https://brew.sh):

```sh
brew tap say-kaj/tap
brew install --cask mycommander
```

Update later with `brew upgrade --cask mycommander`; `brew outdated --cask` shows what is outdated.
Remove with `brew uninstall --cask mycommander` (add `--zap` to delete settings too).

Without Homebrew: download `myCommander-<version>-macos-arm64.zip` from the
[Releases](https://github.com/say-kaj/MyCommander/releases) page, unzip, move `myCommander.app` to
`/Applications` and open it with right-click → Open the first time (the app is ad-hoc signed, not notarized).

## Build and run

Requires macOS on Apple Silicon (M-series, arm64) and a recent stable Rust toolchain. Intel Macs are not supported.

```sh
cargo run --release -- [left-dir] [right-dir]
```

Settings are stored in `~/Library/Application Support/myCommander/`.

To build the `.app` bundle and release zip: `scripts/package.sh` (icon: `scripts/make_icon.py`).

## Contributing

Humans and coding agents: read [`docs/SPEC.md`](docs/SPEC.md) and [`AGENTS.md`](AGENTS.md) first.

## License

MIT – see [`LICENSE`](LICENSE).
