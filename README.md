# myCommander

A native macOS two-panel file manager that works like [Midnight Commander](https://midnight-commander.org/) – without the terminal.

![myCommander](docs/screenshots/main.png)

## Features

- Two panels, F-key driven copy / move / mkdir / delete, mc's keyboard shortcuts
- Listing formats Full / Brief / Long / User, mc sort modes, marking with size summary
- Quick view, Info and Tree panel modes; Find file and External panelize
- Internal viewer (text/hex) and editor, user menu, directory hotlist, history
- Real macOS menu bar with mc's exact menu structure, light/dark mode, Finder-style list
- Background jobs – the UI never blocks

See [`docs/SPEC.md`](docs/SPEC.md) for the full specification and known limitations.

## Build and run

Requires macOS on Apple Silicon (M-series, arm64) and a recent stable Rust toolchain. Intel Macs are not supported.

```sh
cargo run --release -- [left-dir] [right-dir]
```

Settings are stored in `~/Library/Application Support/myCommander/`.

## Contributing

Humans and coding agents: read [`docs/SPEC.md`](docs/SPEC.md) and [`AGENTS.md`](AGENTS.md) first.

## License

MIT – see [`LICENSE`](LICENSE).
