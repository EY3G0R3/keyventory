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
- scan compiled-style `keys[]` tables in dwm and dwl configs, expanding the config's own macros such as `TAGKEYS` and `CHVT`
- include Kitty's built-in shortcuts and zsh's live `bindkey` keymap
- answer "which shortcuts are free?" from the command line (see below)
- toggle sources, click or hold modifier layers, search actions, and inspect source locations
- distinguish free keys, assigned keys, and possible conflicts with per-source colors

A global source (compositor or hotkey daemon) counts only while its process runs, so an installed but idle Hyprland or dwm does not block keys next to dwl. The app enables running sources by default.

Keyventory currently treats duplicate chords as **possible** conflicts. Scope and input precedence matter: two application-local bindings may coexist, while a compositor binding can intercept both. A later conflict engine will model those relationships explicitly.

Editing is deliberately disabled until every writable adapter has round-trip fixtures, validation, an exact diff preview, and an atomic backup path.

## Command line

The same executable answers questions without opening a window or needing a display:

```sh
keyventory free [--scope global|all] [--mods super,super+shift] [--json]
keyventory check CHORD [--scope global|all] [--json]
```

`free` lists free chords for each modifier set in `--mods` (default `super,super+shift`; use `none` for no modifier). `check` reports whether one chord, such as `super+shift+t`, is taken. Modifiers are `super` (also `win`, `logo`, `meta`), `ctrl`, `alt` and `shift`; key names follow the list below.

- `--scope global` (default): a chord is taken when a running global source binds it. A compositor binding takes the key from every application, so application bindings on that chord are reported as clashes, not blockers.
- `--scope all`: application bindings block too.

`check` exits 0 when the chord is free, 1 when it is taken and 2 on bad usage. `free` exits 0.

### JSON output

`--json` prints one object. The `version` field is `1`; fields may be added, but existing fields keep their meaning within a version.

A binding object:

| Field | Meaning |
| --- | --- |
| `source` | source id: `dwl`, `kitty`, `zsh`, `hyprland`, `dwm`, `sxhkd`, `tmux`, `neovim` |
| `sourceName` | display name |
| `scope` | `global` or `application` |
| `context` | where it applies, such as `keys`, `keys, works while locked`, `kitty defaults` or `main keymap` |
| `chord` | normalized chord, such as `Super+Shift+T` |
| `action` | what the binding does |
| `file` | config file |
| `line` | 1-based line, or 0 when unknown (built-in defaults and live keymaps) |

`check --json`:

```json
{
  "version": 1,
  "chord": "Super+1",
  "modifiers": ["Super"],
  "key": "1",
  "scope": "global",
  "free": false,
  "blockers": [{ "source": "dwl", "sourceName": "dwl", "scope": "global", "context": "keys", "chord": "Super+1", "action": "togglescratch 1", "file": "/home/me/src/dwl/config.h", "line": 386 }],
  "clashes": [],
  "inactive": [],
  "sources": [{ "id": "dwl", "name": "dwl", "scope": "global", "status": "ready", "active": true, "bindings": 144, "warnings": [] }]
}
```

`blockers` make the chord taken. `clashes` are application bindings a global shortcut would shadow. `inactive` are bindings in sources that are not running and are ignored.

`free --json` has `version`, `scope`, `mods` (the modifier sets, as arrays), `sources` (as above) and `free`, a list of `{ "chord", "modifiers", "key", "clashes" }`.

Modifiers are always ordered `Super`, `Ctrl`, `Alt`, `Shift`. Key names are uppercase: letters, digits, `F1`–`F12`, `ESC`, `PRINT`, `GRAVE`, `MINUS`, `EQUAL`, `BACKSPACE`, `TAB`, `BRACKETLEFT`, `BRACKETRIGHT`, `BACKSLASH`, `SEMICOLON`, `APOSTROPHE`, `ENTER`, `COMMA`, `PERIOD`, `SLASH`, `SPACE`, `INSERT`, `DELETE`, `HOME`, `END`, `PAGEUP`, `PAGEDOWN`, `LEFT`, `UP`, `DOWN`, `RIGHT`.

### dwl and dwm notes

dwl compares a binding's keysym with both the unshifted and the shifted symbol of the pressed key, so `MODKEY|WLR_MODIFIER_SHIFT, XKB_KEY_T` and `XKB_KEY_t` are the same chord, Super+Shift+T. A shifted keysym bound without Shift can never fire; Keyventory skips it with a warning. Entries also listed in `lockkeys[]` get the context `keys, works while locked`.

### Not scanned

Firefox shortcuts are compiled in and have no config to read. Quickshell's `GlobalShortcut` works only through a Hyprland protocol, so it binds nothing under dwl.

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

Kitty's defaults come from `kitty +runpy`, which loads Kitty's own option code without opening a window. zsh bindings come from `zsh -ic 'bindkey -L'`; `file:line` points into `.zshrc` when a matching `bindkey` line exists there.

Hyprland's live API is authoritative for Lua configurations, although Lua callback source locations and bodies are not exposed. Neovim is queried in headless mode, so mappings registered only after a lazy plugin event may not appear until a later adapter can introspect an existing editor process. dwm and dwl are discovered from `~/.config/<name>/` or `~/src/<name>/`, preferring `config.h` over `config.def.h`.

## Architecture

- `src-tauri/src/scanner.rs`: source adapters and the normalized binding model
- `src-tauri/src/lib.rs`: narrow Tauri command boundary
- `src-tauri/src/cli.rs`: headless `free` and `check` commands
- `src/App.tsx`: source selection, modifier layers, conflict/free-state projection
- `src/App.css`: responsive keyboard and inspection UI

The Rust backend owns filesystem access. The webview receives normalized binding records and has no general-purpose filesystem permission.

## Planned adapters

i3/Sway, Zellij, Ghostty, and a documented adapter format for additional tools.

## Prior art

Keyventory learns from the layered keyboard view in [BindScope](https://github.com/juliopchile/BindScope), the shortcut visualization in [KeyCombiner](https://keycombiner.com/features/), Linux config parsing in [HotKeyHub](https://github.com/meowrch/HotKeyHub), and the precedence-aware reporting in [HotkeyClash](https://github.com/Wunderlandmedia/HotkeyClash).
