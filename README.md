# Keyventory

See every keybinding, conflict, and open chord across your tools.

Keyventory is a Linux-first desktop app that inventories shortcut configuration, normalizes it into one model, and projects selected sources onto a virtual keyboard. The long-term goal is safe, previewable remapping back to each tool's native config.

## Current alpha

The first working slice can:

- scan Hyprland configs, including variables and sourced files
- scan Kitty configs, including `include` and `globinclude`
- scan simple sxhkd bindings and tmux binding tables
- toggle sources, combine modifier layers, search actions, and inspect source locations
- distinguish free keys, assigned keys, and possible conflicts

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

## Architecture

- `src-tauri/src/scanner.rs`: source adapters and the normalized binding model
- `src-tauri/src/lib.rs`: narrow Tauri command boundary
- `src/App.tsx`: source selection, modifier layers, conflict/free-state projection
- `src/App.css`: responsive keyboard and inspection UI

The Rust backend owns filesystem access. The webview receives normalized binding records and has no general-purpose filesystem permission.

## Planned adapters

Neovim, dwm, dwl, i3/Sway, Zellij, Ghostty, and a documented adapter format for additional tools.

## Prior art

Keyventory learns from the layered keyboard view in [BindScope](https://github.com/juliopchile/BindScope), the shortcut visualization in [KeyCombiner](https://keycombiner.com/features/), Linux config parsing in [HotKeyHub](https://github.com/meowrch/HotKeyHub), and the precedence-aware reporting in [HotkeyClash](https://github.com/Wunderlandmedia/HotkeyClash).
