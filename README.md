# Keyventory

See every keybinding, conflict, and open chord across your tools.

Keyventory is a Linux-first desktop app that inventories shortcut configuration, normalizes it into one model, and projects selected sources onto a virtual keyboard. The long-term goal is safe, previewable remapping back to each tool's native config.

## Current alpha

The first working slice can:

- scan live Hyprland bindings, including evaluated Omarchy defaults and personal Lua config
- fall back to parsing classic Hyprland `.conf` variables and sourced files
- scan Kitty configs, including `include` and `globinclude`
- scan simple sxhkd bindings and tmux binding tables
- scan mappings loaded by Neovim at startup
- scan compiled-style `keys[]` tables and `TAGKEYS` macros in dwm and dwl configs
- toggle sources, click or hold modifier layers, search actions, and inspect source locations
- distinguish free keys, assigned keys, and possible conflicts with per-source colors

Keyventory currently treats duplicate chords as **possible** conflicts. Scope and input precedence matter: two application-local bindings may coexist, while a compositor binding can intercept both. A later conflict engine will model those relationships explicitly.

Editing is deliberately disabled until every writable adapter has round-trip fixtures, validation, an exact diff preview, and an atomic backup path.

## Development

Requirements: Node.js, npm, Rust, GTK 4, WebKitGTK 4.1, and the usual Tauri Linux prerequisites.

```sh
npm install
npm run tauri dev
```

Useful checks:

```sh
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Running `npm run dev` opens a browser preview with sample data. The Tauri app reads the real config files.

## Desktop install

Build the desktop executable and install it for the current user:

```sh
make install
```

This installs the executable to `~/.local/bin/keyventory` and registers Keyventory
with XDG-compatible application launchers, including Omarchy and Ubuntu. Launch it
from the application menu or run `keyventory` in a terminal.

Override the destination with `make install PREFIX=/another/prefix`. The selected
prefix's `bin` directory must be on the desktop session's `PATH`. The executable
contains the frontend assets and does not need the repository after installation.

Hyprland's live API is authoritative for Lua configurations, although Lua callback source locations and bodies are not exposed. Neovim is queried in headless mode, so mappings registered only after a lazy plugin event may not appear until a later adapter can introspect an existing editor process. dwm and dwl are discovered from `~/.config/<name>/` or `~/src/<name>/`, preferring `config.h` over `config.def.h`.

## Architecture

- `src-tauri/src/scanner.rs`: source adapters and the normalized binding model
- `src-tauri/src/lib.rs`: narrow Tauri command boundary
- `src/App.tsx`: source selection, modifier layers, conflict/free-state projection
- `src/App.css`: responsive keyboard and inspection UI

The Rust backend owns filesystem access. The webview receives normalized binding records and has no general-purpose filesystem permission.

## Planned adapters

i3/Sway, Zellij, Ghostty, and a documented adapter format for additional tools.

## Prior art

Keyventory learns from the layered keyboard view in [BindScope](https://github.com/juliopchile/BindScope), the shortcut visualization in [KeyCombiner](https://keycombiner.com/features/), Linux config parsing in [HotKeyHub](https://github.com/meowrch/HotKeyHub), and the precedence-aware reporting in [HotkeyClash](https://github.com/Wunderlandmedia/HotkeyClash).
