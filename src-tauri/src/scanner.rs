use glob::glob;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub id: String,
    pub source_id: String,
    pub source_name: String,
    pub scope: String,
    pub context: String,
    pub modifiers: Vec<String>,
    pub key: String,
    pub action: String,
    pub source_file: String,
    pub line: usize,
    pub editable: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScan {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scope: String,
    pub status: String,
    /// Whether the program is in use here: a global source's process is
    /// running, or an application source's config was found.
    pub active: bool,
    pub config_paths: Vec<String>,
    pub bindings: Vec<Binding>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub sources: Vec<SourceScan>,
}

pub fn scan_all() -> ScanResult {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return ScanResult { sources: vec![] };
    };

    let running = running_processes();
    let mut sources = vec![
        scan_hyprland(&home),
        scan_kitty(&home),
        scan_zsh(&home),
        scan_sxhkd(&home),
        scan_tmux(&home),
        scan_neovim(&home),
        scan_dwm(&home),
        scan_dwl(&home),
    ];
    for source in &mut sources {
        source.active = is_active(source, &running);
    }
    ScanResult { sources }
}

/// A global source only intercepts keys while its process runs, so an
/// installed but idle compositor (Hyprland next to dwl) must not count.
fn is_active(source: &SourceScan, running: &HashSet<String>) -> bool {
    if source.status == "missing" {
        return false;
    }
    if source.scope != "global" {
        return true;
    }
    let process = match source.id.as_str() {
        "hyprland" => "Hyprland",
        other => other,
    };
    running.contains(process)
}

fn running_processes() -> HashSet<String> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return HashSet::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path().join("comm")).ok())
        .map(|comm| comm.trim().to_string())
        .collect()
}

fn missing_source(id: &str, name: &str, kind: &str, scope: &str, path: &Path) -> SourceScan {
    SourceScan {
        active: false,
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        scope: scope.into(),
        status: "missing".into(),
        config_paths: vec![path.display().to_string()],
        bindings: vec![],
        warnings: vec![],
    }
}

fn scan_hyprland(home: &Path) -> SourceScan {
    let lua_path = home.join(".config/hypr/hyprland.lua");
    let conf_path = home.join(".config/hypr/hyprland.conf");
    let path = if lua_path.is_file() {
        lua_path
    } else if conf_path.is_file() {
        conf_path
    } else {
        return missing_source("hyprland", "Hyprland", "compositor", "global", &conf_path);
    };

    if env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        if let Ok(output) = Command::new("hyprctl").args(["binds", "-j"]).output() {
            if output.status.success() {
                match parse_hyprctl_bindings(&output.stdout, &path) {
                    Ok(bindings) => {
                        return SourceScan {
                            active: false,
                            id: "hyprland".into(),
                            name: "Hyprland".into(),
                            kind: "compositor".into(),
                            scope: "global".into(),
                            status: status_for(&bindings, &[]),
                            config_paths: vec![path.display().to_string()],
                            bindings,
                            warnings: vec![],
                        };
                    }
                    Err(error) => {
                        return SourceScan {
                            active: false,
                            id: "hyprland".into(),
                            name: "Hyprland".into(),
                            kind: "compositor".into(),
                            scope: "global".into(),
                            status: "warning".into(),
                            config_paths: vec![path.display().to_string()],
                            bindings: vec![],
                            warnings: vec![format!(
                                "Could not decode live Hyprland bindings: {error}"
                            )],
                        };
                    }
                }
            }
        }
    }

    if path.extension().is_some_and(|extension| extension == "lua") {
        return SourceScan {
            active: false,
            id: "hyprland".into(),
            name: "Hyprland".into(),
            kind: "compositor".into(),
            scope: "global".into(),
            status: "warning".into(),
            config_paths: vec![path.display().to_string()],
            bindings: vec![],
            warnings: vec![
                "Lua config found; launch Keyventory inside Hyprland to read evaluated bindings"
                    .into(),
            ],
        };
    }

    let mut bindings = vec![];
    let mut warnings = vec![];
    let mut files = vec![];
    let mut variables = HashMap::new();
    let mut visited = HashSet::new();
    parse_hyprland_file(
        &path,
        home,
        &mut variables,
        &mut visited,
        &mut files,
        &mut bindings,
        &mut warnings,
    );

    SourceScan {
        active: false,
        id: "hyprland".into(),
        name: "Hyprland".into(),
        kind: "compositor".into(),
        scope: "global".into(),
        status: status_for(&bindings, &warnings),
        config_paths: files,
        bindings,
        warnings,
    }
}

#[derive(Debug, Deserialize)]
struct HyprctlBinding {
    #[serde(default)]
    modmask: u32,
    #[serde(default)]
    key: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    submap: String,
}

fn parse_hyprctl_bindings(bytes: &[u8], source_path: &Path) -> serde_json::Result<Vec<Binding>> {
    let raw: Vec<HyprctlBinding> = serde_json::from_slice(bytes)?;
    Ok(raw
        .into_iter()
        .enumerate()
        .filter_map(|(index, raw)| {
            let key = canonical_key(&raw.key);
            if key.is_empty() {
                return None;
            }
            let mut binding = binding(
                ("hyprland", "Hyprland"),
                (
                    "global",
                    if raw.submap.is_empty() {
                        "default"
                    } else {
                        &raw.submap
                    },
                ),
                (hyprland_modifiers(raw.modmask), key),
                if raw.description.is_empty() {
                    "Hyprland binding".into()
                } else {
                    raw.description
                },
                (source_path, 0),
                false,
            );
            binding.id = format!("hyprland:live:{index}");
            Some(binding)
        })
        .collect())
}

fn hyprland_modifiers(mask: u32) -> Vec<String> {
    let mut modifiers = vec![];
    if mask & 64 != 0 {
        modifiers.push("Super".into());
    }
    if mask & 4 != 0 {
        modifiers.push("Ctrl".into());
    }
    if mask & 8 != 0 {
        modifiers.push("Alt".into());
    }
    if mask & 1 != 0 {
        modifiers.push("Shift".into());
    }
    modifiers
}

fn parse_hyprland_file(
    path: &Path,
    home: &Path,
    variables: &mut HashMap<String, String>,
    visited: &mut HashSet<PathBuf>,
    files: &mut Vec<String>,
    bindings: &mut Vec<Binding>,
    warnings: &mut Vec<String>,
) {
    let path = path.to_path_buf();
    if !visited.insert(path.clone()) {
        return;
    }
    let Ok(text) = fs::read_to_string(&path) else {
        warnings.push(format!("Could not read {}", path.display()));
        return;
    };
    files.push(path.display().to_string());

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('$') {
            if let Some((name, value)) = line.split_once('=') {
                variables.insert(name.trim().into(), value.trim().into());
            }
            continue;
        }

        let Some((directive, value)) = line.split_once('=') else {
            continue;
        };
        let directive = directive.trim();
        let value = value.trim();

        if directive == "source" {
            let expanded = expand_variables(value, variables);
            for included in resolve_include(&path, home, &expanded, warnings) {
                parse_hyprland_file(
                    &included, home, variables, visited, files, bindings, warnings,
                );
            }
            continue;
        }

        if !directive.starts_with("bind") || directive.starts_with("unbind") {
            continue;
        }

        let parts: Vec<_> = value.split(',').map(str::trim).collect();
        if parts.len() < 3 {
            warnings.push(format!(
                "Skipped malformed bind at {}:{line_number}",
                path.display()
            ));
            continue;
        }

        let modifier_text = expand_variables(parts[0], variables);
        let modifiers = parse_modifiers(&modifier_text.replace('+', " "));
        let key = canonical_key(parts[1]);
        if key.is_empty() {
            continue;
        }
        let has_description = directive
            .strip_prefix("bind")
            .is_some_and(|flags| flags.contains('d'));
        let action = if has_description && parts.len() >= 4 {
            parts[2].to_string()
        } else if parts.len() >= 4 && !parts[3].is_empty() {
            format!("{} {}", parts[2], parts[3..].join(", "))
        } else {
            parts[2].to_string()
        };
        bindings.push(binding(
            ("hyprland", "Hyprland"),
            ("global", directive),
            (modifiers, key),
            action,
            (&path, line_number),
            true,
        ));
    }
}

fn scan_kitty(home: &Path) -> SourceScan {
    let path = home.join(".config/kitty/kitty.conf");
    if !path.is_file() {
        return missing_source("kitty", "Kitty", "terminal", "application", &path);
    }

    let mut bindings = vec![];
    let mut warnings = vec![];
    let mut files = vec![];
    let mut visited = HashSet::new();
    let mut kitty_mod = vec!["Ctrl".into(), "Shift".into()];
    parse_kitty_file(
        &path,
        home,
        &mut kitty_mod,
        &mut visited,
        &mut files,
        &mut bindings,
        &mut warnings,
    );

    // A user map, including `no_op`, replaces the default on the same chord.
    let user_chords: HashSet<_> = bindings
        .iter()
        .map(|item| (item.modifiers.clone(), item.key.clone()))
        .collect();
    bindings.retain(|item| item.action != "no_op");
    let cleared = files.iter().any(|file| {
        fs::read_to_string(file).is_ok_and(|text| {
            text.lines().any(|line| {
                strip_comment(line)
                    .split_whitespace()
                    .eq(["clear_all_shortcuts", "yes"])
            })
        })
    });
    if !cleared {
        match kitty_defaults(&kitty_mod) {
            Some(defaults) => {
                for (chord, action) in defaults {
                    if user_chords.contains(&chord) || chord.1.is_empty() {
                        continue;
                    }
                    let mut item = binding(
                        ("kitty", "Kitty"),
                        ("application", "kitty defaults"),
                        chord,
                        action,
                        (&path, 0),
                        false,
                    );
                    item.id = format!("kitty:default:{}", bindings.len());
                    bindings.push(item);
                }
            }
            None => warnings
                .push("Built-in Kitty defaults are missing: `kitty +runpy` did not answer".into()),
        }
    }

    SourceScan {
        active: false,
        id: "kitty".into(),
        name: "Kitty".into(),
        kind: "terminal".into(),
        scope: "application".into(),
        status: status_for(&bindings, &warnings),
        config_paths: files,
        bindings,
        warnings,
    }
}

fn parse_kitty_file(
    path: &Path,
    home: &Path,
    kitty_mod: &mut Vec<String>,
    visited: &mut HashSet<PathBuf>,
    files: &mut Vec<String>,
    bindings: &mut Vec<Binding>,
    warnings: &mut Vec<String>,
) {
    let path = path.to_path_buf();
    if !visited.insert(path.clone()) {
        return;
    }
    let Ok(text) = fs::read_to_string(&path) else {
        warnings.push(format!("Could not read {}", path.display()));
        return;
    };
    files.push(path.display().to_string());

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(original).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(include) = line
            .strip_prefix("include ")
            .or_else(|| line.strip_prefix("globinclude "))
        {
            for included in resolve_include(&path, home, include.trim(), warnings) {
                parse_kitty_file(
                    &included, home, kitty_mod, visited, files, bindings, warnings,
                );
            }
            continue;
        }

        if let Some(value) = line.strip_prefix("kitty_mod ") {
            *kitty_mod = parse_modifiers(&value.replace('+', " "));
            continue;
        }

        let Some(rest) = line.strip_prefix("map ") else {
            continue;
        };
        let fields = rest.split_whitespace().collect::<Vec<_>>();
        let Some(chord_index) = fields.iter().position(|field| !field.starts_with("--")) else {
            continue;
        };
        let chord = fields[chord_index];
        let action = fields[chord_index + 1..].join(" ");
        if action.is_empty() {
            continue;
        }
        let first_stroke = chord.split('>').next().unwrap_or(chord);
        let expanded_chord = first_stroke.replace(
            "kitty_mod",
            &kitty_mod
                .iter()
                .map(|modifier| modifier.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("+"),
        );
        let (modifiers, key) = parse_chord(&expanded_chord);
        bindings.push(binding(
            ("kitty", "Kitty"),
            ("application", "kitty"),
            (modifiers, key),
            if chord.contains('>') {
                format!("{action} · sequence {chord}")
            } else {
                action
            },
            (&path, line_number),
            true,
        ));
    }
}

/// Kitty's built-in shortcuts, asked from kitty itself. `+runpy` runs
/// kitty's Python without opening a window.
type KeyChord = (Vec<String>, String);

fn kitty_defaults(kitty_mod: &[String]) -> Option<Vec<(KeyChord, String)>> {
    let script = "import json\n\
        from kitty.config import load_config\n\
        from kitty.types import human_repr_of_single_key as r\n\
        o = load_config()\n\
        print(json.dumps([[r(k, 0), d.definition] for m in o.keyboard_modes.values() \
        for k, ds in m.keymap.items() for d in ds \
        if d.definition and not d.options.when_focus_on]))";
    let output = Command::new("kitty")
        .args(["+runpy", script])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw: Vec<(String, String)> = serde_json::from_slice(&output.stdout).ok()?;
    let user_mod = kitty_mod
        .iter()
        .map(|modifier| modifier.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("+");
    Some(
        raw.into_iter()
            .map(|(chord, action)| {
                let chord = match chord.strip_prefix("kitty_mod+ctrl+shift+") {
                    Some(rest) => format!("{user_mod}+{rest}"),
                    None => chord.trim_start_matches("kitty_mod+").to_string(),
                };
                (parse_chord(&chord), action)
            })
            .collect(),
    )
}

fn scan_zsh(home: &Path) -> SourceScan {
    let config_dir = env::var_os("ZDOTDIR").map_or_else(|| home.to_path_buf(), PathBuf::from);
    let path = config_dir.join(".zshrc");
    if !path.is_file() {
        return missing_source("zsh", "zsh", "shell", "application", &path);
    }
    // The live keymap includes zsh defaults and plugin bindings that a
    // config parser would miss. Without a terminal, zle is off but bindkey
    // still reports the main keymap.
    let output = Command::new("timeout")
        .args(["10", "zsh", "-ic", "bindkey -L"])
        .stdin(std::process::Stdio::null())
        .output();
    let Some(listing) = output
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    else {
        return source_warning(
            "zsh",
            "zsh",
            "shell",
            "application",
            &path,
            "zsh config found, but `zsh -ic 'bindkey -L'` failed",
        );
    };
    let config = fs::read_to_string(&path).unwrap_or_default();
    let bindings = parse_zsh_bindkeys(&listing, &path, &config);

    SourceScan {
        id: "zsh".into(),
        name: "zsh".into(),
        kind: "shell".into(),
        scope: "application".into(),
        status: status_for(&bindings, &[]),
        active: false,
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings: vec![],
    }
}

/// Bindings from `bindkey -L`. Multi-key sequences are grouped under their
/// first key, which is the chord the terminal would have to pass through.
fn parse_zsh_bindkeys(listing: &str, path: &Path, config: &str) -> Vec<Binding> {
    let mut bindings: Vec<Binding> = vec![];
    let mut prefixes: HashMap<(Vec<String>, String), usize> = HashMap::new();
    for line in listing.lines() {
        let Some(rest) = line.strip_prefix("bindkey ") else {
            continue;
        };
        let (string_macro, rest) = match rest.strip_prefix("-s ") {
            Some(rest) => (true, rest),
            None if rest.starts_with('-') => continue,
            None => (false, rest),
        };
        let Some((sequence, widget)) = unquote_zsh(rest) else {
            continue;
        };
        let widget = widget.trim();
        if matches!(widget, "self-insert" | "undefined-key" | "beep") {
            continue;
        }
        let bytes = zsh_caret_bytes(&sequence);
        let Some((modifiers, key, used)) = zsh_first_stroke(&bytes) else {
            continue;
        };
        let action = if string_macro {
            format!("send-string {widget}")
        } else {
            widget.to_string()
        };
        let chord = (modifiers, key);
        let line_number = config
            .lines()
            .position(|config_line| {
                let config_line = config_line.to_ascii_lowercase();
                config_line.trim_start().starts_with("bindkey")
                    && (config_line.contains(&format!("'{}'", sequence.to_ascii_lowercase()))
                        || config_line.contains(&format!("\"{}\"", sequence.to_ascii_lowercase())))
            })
            .map_or(0, |index| index + 1);
        if used < bytes.len() {
            if let Some(&index) = prefixes.get(&chord) {
                let count = bindings[index].action.matches(';').count() + 2;
                if count <= 3 {
                    bindings[index]
                        .action
                        .push_str(&format!("; {sequence} {action}"));
                } else if !bindings[index].action.ends_with('…') {
                    bindings[index].action.push_str("; …");
                }
                continue;
            }
            prefixes.insert(chord.clone(), bindings.len());
        }
        let action = if used < bytes.len() {
            format!("prefix · {sequence} {action}")
        } else {
            action
        };
        let mut item = binding(
            ("zsh", "zsh"),
            ("application", "main keymap"),
            chord,
            action,
            (path, line_number),
            false,
        );
        item.id = format!("zsh:{}", bindings.len());
        bindings.push(item);
    }
    bindings
}

/// The first double-quoted word of a `bindkey -L` line and the remainder.
fn unquote_zsh(text: &str) -> Option<(String, String)> {
    let mut characters = text.strip_prefix('"')?.chars();
    let mut word = String::new();
    while let Some(character) = characters.next() {
        match character {
            '\\' => word.push(characters.next()?),
            '"' => return Some((word, characters.collect())),
            _ => word.push(character),
        }
    }
    None
}

fn zsh_caret_bytes(sequence: &str) -> Vec<u8> {
    let mut bytes = vec![];
    let mut characters = sequence.bytes();
    while let Some(byte) = characters.next() {
        if byte != b'^' {
            bytes.push(byte);
            continue;
        }
        match characters.next() {
            Some(b'?') => bytes.push(0x7f),
            Some(next) => bytes.push(next.to_ascii_uppercase() & 0x1f),
            None => bytes.push(b'^'),
        }
    }
    bytes
}

/// Decodes the first keystroke of a terminal byte sequence into a chord and
/// the number of bytes it used.
fn zsh_first_stroke(bytes: &[u8]) -> Option<(Vec<String>, String, usize)> {
    let (mut modifiers, key, used) = match bytes {
        [] => return None,
        [0x1b] => (vec![], "ESC".to_string(), 1),
        [0x1b, b'[' | b'O', rest @ ..] if !rest.is_empty() => {
            let (modifiers, key) = decode_csi(bytes[1], rest)?;
            (modifiers, key, bytes.len())
        }
        [0x1b, next, ..] => {
            let (mut modifiers, key) = decode_terminal_byte(*next)?;
            modifiers.push("Alt".into());
            (modifiers, key, 2)
        }
        [first, ..] => {
            let (modifiers, key) = decode_terminal_byte(*first)?;
            (modifiers, key, 1)
        }
    };
    // A bare printable key is typing, not a shortcut.
    if modifiers.is_empty() && (key.len() == 1 || key == "SPACE") {
        return None;
    }
    sort_modifiers(&mut modifiers);
    Some((modifiers, key, used))
}

fn decode_terminal_byte(byte: u8) -> Option<(Vec<String>, String)> {
    match byte {
        0x00 => Some((vec!["Ctrl".into()], "SPACE".into())),
        0x09 => Some((vec![], "TAB".into())),
        0x0d => Some((vec![], "ENTER".into())),
        0x1b => Some((vec![], "ESC".into())),
        0x7f => Some((vec![], "BACKSPACE".into())),
        0x01..=0x1a => Some((vec!["Ctrl".into()], ((b'A' + byte - 1) as char).to_string())),
        0x20..=0x7e => {
            let character = byte as char;
            if character == ' ' {
                Some((vec![], "SPACE".into()))
            } else {
                Some(neovim_literal_key(character))
            }
        }
        _ => None,
    }
}

fn decode_csi(introducer: u8, rest: &[u8]) -> Option<(Vec<String>, String)> {
    let text = std::str::from_utf8(rest).ok()?;
    let final_char = text.chars().last()?;
    let params: Vec<u32> = text[..text.len() - 1]
        .split(';')
        .filter(|param| !param.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let key = match (introducer, final_char) {
        (b'O', 'P') => "F1",
        (b'O', 'Q') => "F2",
        (b'O', 'R') => "F3",
        (b'O', 'S') => "F4",
        (_, 'A') => "UP",
        (_, 'B') => "DOWN",
        (_, 'C') => "RIGHT",
        (_, 'D') => "LEFT",
        (_, 'H') => "HOME",
        (_, 'F') => "END",
        (b'[', 'Z') => return Some((vec!["Shift".into()], "TAB".into())),
        (b'[', '~') => match params.first()? {
            1 | 7 => "HOME",
            2 => "INSERT",
            3 => "DELETE",
            4 | 8 => "END",
            5 => "PAGEUP",
            6 => "PAGEDOWN",
            15 => "F5",
            17 => "F6",
            18 => "F7",
            19 => "F8",
            20 => "F9",
            21 => "F10",
            23 => "F11",
            24 => "F12",
            _ => return None,
        },
        _ => return None,
    };
    let mut modifiers = vec![];
    if let Some(mask) = params.get(1).map(|value| value.saturating_sub(1)) {
        if mask & 4 != 0 {
            modifiers.push("Ctrl".into());
        }
        if mask & 2 != 0 {
            modifiers.push("Alt".into());
        }
        if mask & 1 != 0 {
            modifiers.push("Shift".into());
        }
    }
    Some((modifiers, key.into()))
}

fn scan_sxhkd(home: &Path) -> SourceScan {
    let candidates = [
        home.join(".config/sxhkd/sxhkdrc"),
        home.join(".config/bspwm/sxhkdrc"),
    ];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return missing_source("sxhkd", "sxhkd", "hotkey daemon", "global", &candidates[0]);
    };
    let mut bindings = vec![];
    let mut warnings = vec![];
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            warnings.push(format!("Could not read {}", path.display()));
            String::new()
        }
    };

    let mut pending: Option<(usize, String)> = None;
    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        if original.trim().is_empty() || original.trim_start().starts_with('#') {
            continue;
        }
        if original.starts_with(char::is_whitespace) {
            if let Some((bind_line, chord)) = pending.take() {
                let (modifiers, key) = parse_chord(&chord.replace(' ', ""));
                bindings.push(binding(
                    ("sxhkd", "sxhkd"),
                    ("global", "sxhkd"),
                    (modifiers, key),
                    original.trim().to_string(),
                    (path, bind_line),
                    !chord.contains('{'),
                ));
            }
        } else {
            pending = Some((line_number, strip_comment(original).trim().to_string()));
        }
    }

    SourceScan {
        active: false,
        id: "sxhkd".into(),
        name: "sxhkd".into(),
        kind: "hotkey daemon".into(),
        scope: "global".into(),
        status: status_for(&bindings, &warnings),
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings,
    }
}

fn scan_tmux(home: &Path) -> SourceScan {
    let candidates = [home.join(".tmux.conf"), home.join(".config/tmux/tmux.conf")];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return missing_source(
            "tmux",
            "tmux",
            "terminal multiplexer",
            "application",
            &candidates[0],
        );
    };
    let mut bindings = vec![];
    let mut warnings = vec![];
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => {
            warnings.push(format!("Could not read {}", path.display()));
            String::new()
        }
    };

    for (index, original) in text.lines().enumerate() {
        let line = strip_comment(original).trim();
        let Some(mut rest) = line
            .strip_prefix("bind-key ")
            .or_else(|| line.strip_prefix("bind "))
        else {
            continue;
        };
        let mut context = "prefix".to_string();
        let mut no_prefix = false;
        loop {
            if let Some(next) = rest.strip_prefix("-n ") {
                no_prefix = true;
                rest = next.trim_start();
            } else if let Some(next) = rest.strip_prefix("-r ") {
                rest = next.trim_start();
            } else if let Some(next) = rest.strip_prefix("-T ") {
                let Some((table, tail)) = next.split_once(char::is_whitespace) else {
                    break;
                };
                context = table.into();
                rest = tail.trim_start();
            } else {
                break;
            }
        }
        if no_prefix {
            context = "global".into();
        }
        let Some((chord, action)) = rest.split_once(char::is_whitespace) else {
            continue;
        };
        let (modifiers, key) = parse_tmux_chord(chord);
        bindings.push(binding(
            ("tmux", "tmux"),
            ("application", &context),
            (modifiers, key),
            action.trim().to_string(),
            (path, index + 1),
            true,
        ));
    }

    SourceScan {
        active: false,
        id: "tmux".into(),
        name: "tmux".into(),
        kind: "terminal multiplexer".into(),
        scope: "application".into(),
        status: status_for(&bindings, &warnings),
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings,
    }
}

#[derive(Debug, Deserialize)]
struct NeovimBinding {
    mode: String,
    lhs: String,
    #[serde(default)]
    rhs: String,
    #[serde(default)]
    desc: String,
}

fn scan_neovim(home: &Path) -> SourceScan {
    let candidates = [
        home.join(".config/nvim/init.lua"),
        home.join(".config/nvim/init.vim"),
        home.join(".vimrc"),
    ];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return missing_source("neovim", "Neovim", "editor", "application", &candidates[0]);
    };

    let lua = r#"local out={}; for _,mode in ipairs({'n','i','v','x','s','o','t','c'}) do for _,m in ipairs(vim.api.nvim_get_keymap(mode)) do table.insert(out,{mode=mode,lhs=m.lhs,rhs=m.rhs or '',desc=m.desc or ''}) end end; io.stdout:write('KEYVENTORY_JSON:'..vim.json.encode(out))"#;
    let output = Command::new("nvim")
        .args([
            "--headless",
            "-i",
            "NONE",
            "--cmd",
            "set shortmess+=I",
            "-c",
            &format!("lua {lua}"),
            "-c",
            "qa!",
        ])
        .output();

    let Ok(output) = output else {
        return source_warning(
            "neovim",
            "Neovim",
            "editor",
            "application",
            path,
            "Neovim config found, but the nvim executable could not be started",
        );
    };
    if !output.status.success() {
        return source_warning(
            "neovim",
            "Neovim",
            "editor",
            "application",
            path,
            "Neovim config did not finish loading in headless mode",
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(json) = stdout.split("KEYVENTORY_JSON:").nth(1) else {
        return source_warning(
            "neovim",
            "Neovim",
            "editor",
            "application",
            path,
            "Neovim returned no keymap data",
        );
    };
    let Ok(raw_bindings) = serde_json::from_str::<Vec<NeovimBinding>>(json.trim()) else {
        return source_warning(
            "neovim",
            "Neovim",
            "editor",
            "application",
            path,
            "Neovim returned keymap data Keyventory could not decode",
        );
    };

    let bindings = raw_bindings
        .into_iter()
        .enumerate()
        .filter_map(|(index, raw)| {
            let (modifiers, key, sequence) = parse_neovim_chord(&raw.lhs)?;
            let mut action = if !raw.desc.is_empty() {
                raw.desc
            } else if !raw.rhs.is_empty() {
                raw.rhs
            } else {
                "Lua callback".into()
            };
            if sequence {
                action.push_str(&format!(" · sequence {}", raw.lhs));
            }
            let mut binding = binding(
                ("neovim", "Neovim"),
                ("application", neovim_mode_name(&raw.mode)),
                (modifiers, key),
                action,
                (path, 0),
                false,
            );
            binding.id = format!("neovim:runtime:{index}");
            Some(binding)
        })
        .collect::<Vec<_>>();

    SourceScan {
        active: false,
        id: "neovim".into(),
        name: "Neovim".into(),
        kind: "editor".into(),
        scope: "application".into(),
        status: status_for(&bindings, &[]),
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings: vec![],
    }
}

fn source_warning(
    id: &str,
    name: &str,
    kind: &str,
    scope: &str,
    path: &Path,
    warning: &str,
) -> SourceScan {
    SourceScan {
        active: false,
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        scope: scope.into(),
        status: "warning".into(),
        config_paths: vec![path.display().to_string()],
        bindings: vec![],
        warnings: vec![warning.into()],
    }
}

fn neovim_mode_name(mode: &str) -> &str {
    match mode {
        "n" => "normal",
        "i" => "insert",
        "v" | "x" => "visual",
        "s" => "select",
        "o" => "operator-pending",
        "t" => "terminal",
        "c" => "command-line",
        _ => mode,
    }
}

fn parse_neovim_chord(lhs: &str) -> Option<(Vec<String>, String, bool)> {
    let lhs = lhs.trim_matches('\0');
    if lhs.is_empty() {
        return None;
    }

    if let Some(rest) = lhs.strip_prefix('<') {
        let end = rest.find('>')?;
        let mut token = &rest[..end];
        let sequence = end + 2 < lhs.len();
        let mut modifiers = vec![];
        loop {
            let upper = token.to_ascii_uppercase();
            let (modifier, tail) = if upper.starts_with("C-") {
                (Some("Ctrl"), &token[2..])
            } else if upper.starts_with("A-") || upper.starts_with("M-") {
                (Some("Alt"), &token[2..])
            } else if upper.starts_with("S-") {
                (Some("Shift"), &token[2..])
            } else if upper.starts_with("D-") {
                (Some("Super"), &token[2..])
            } else {
                (None, token)
            };
            let Some(modifier) = modifier else {
                break;
            };
            modifiers.push(modifier.into());
            token = tail;
        }
        let key = match token.to_ascii_uppercase().as_str() {
            "LEADER" | "LOCALLEADER" | "SPACE" => "SPACE".into(),
            "CR" => "ENTER".into(),
            "LT" => "COMMA".into(),
            _ => canonical_key(token),
        };
        if key.is_empty() {
            return None;
        }
        sort_modifiers(&mut modifiers);
        return Some((modifiers, key, sequence));
    }

    let first = lhs.chars().next()?;
    let sequence = first.len_utf8() < lhs.len();
    let (mut modifiers, key) = neovim_literal_key(first);
    sort_modifiers(&mut modifiers);
    Some((modifiers, key, sequence))
}

fn neovim_literal_key(key: char) -> (Vec<String>, String) {
    let shifted = match key {
        '~' => Some("GRAVE"),
        '!' => Some("1"),
        '@' => Some("2"),
        '#' => Some("3"),
        '$' => Some("4"),
        '%' => Some("5"),
        '^' => Some("6"),
        '&' => Some("7"),
        '*' => Some("8"),
        '(' => Some("9"),
        ')' => Some("0"),
        '_' => Some("MINUS"),
        '+' => Some("EQUAL"),
        '{' => Some("BRACKETLEFT"),
        '}' => Some("BRACKETRIGHT"),
        '|' => Some("BACKSLASH"),
        ':' => Some("SEMICOLON"),
        '"' => Some("APOSTROPHE"),
        '<' => Some("COMMA"),
        '>' => Some("PERIOD"),
        '?' => Some("SLASH"),
        _ => None,
    };
    if let Some(key) = shifted {
        return (vec!["Shift".into()], key.into());
    }
    if key.is_ascii_uppercase() {
        return (vec!["Shift".into()], key.to_string());
    }
    (vec![], canonical_key(&key.to_string()))
}

fn scan_dwm(home: &Path) -> SourceScan {
    scan_c_window_manager(home, "dwm", "dwm", "X11 window manager", "XK_")
}

fn scan_dwl(home: &Path) -> SourceScan {
    scan_c_window_manager(home, "dwl", "dwl", "Wayland compositor", "XKB_KEY_")
}

fn scan_c_window_manager(
    home: &Path,
    id: &str,
    name: &str,
    kind: &str,
    key_prefix: &str,
) -> SourceScan {
    let candidates = [
        home.join(format!(".config/{id}/config.h")),
        home.join(format!(".config/{id}/config.def.h")),
        home.join(format!("src/{id}/config.h")),
        home.join(format!("src/{id}/config.def.h")),
    ];
    let Some(path) = candidates.iter().find(|path| path.is_file()) else {
        return missing_source(id, name, kind, "global", &candidates[0]);
    };
    let Ok(text) = fs::read_to_string(path) else {
        return source_warning(
            id,
            name,
            kind,
            "global",
            path,
            "Config file could not be read",
        );
    };
    let modkey = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("#define MODKEY "))
        .unwrap_or("Mod4Mask")
        .trim()
        .to_string();
    let macros = c_macros(&text);
    let mut warnings = vec![];
    let lock_chords: HashSet<_> = c_array_rows(&text, " lockkeys[]", &macros, &mut warnings)
        .into_iter()
        .filter(|(_, fields)| fields.len() >= 2)
        .filter_map(|(_, fields)| c_chord(&fields[0], &fields[1], &modkey, key_prefix))
        .collect();

    let mut bindings: Vec<Binding> = vec![];
    for (line_number, fields) in c_array_rows(&text, " keys[]", &macros, &mut warnings) {
        if fields.len() < 3 {
            continue;
        }
        let Some(chord) = c_chord(&fields[0], &fields[1], &modkey, key_prefix) else {
            warnings.push(format!(
                "Skipped {} at {}:{line_number}: a shifted keysym without Shift never matches",
                fields[1],
                path.display()
            ));
            continue;
        };
        let context = if lock_chords.contains(&chord) {
            "keys, works while locked"
        } else {
            "keys"
        };
        let mut item = binding(
            (id, name),
            ("global", context),
            chord,
            c_action(&fields[2], fields.get(3).map(String::as_str)),
            (path, line_number),
            true,
        );
        item.id.push_str(&format!(":{}", bindings.len()));
        bindings.push(item);
    }

    SourceScan {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        scope: "global".into(),
        status: status_for(&bindings, &warnings),
        active: false,
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings,
    }
}

struct CMacro {
    params: Vec<String>,
    body: String,
}

/// Function-like `#define`s, joined across backslash continuations, so key
/// tables can use the config's own TAGKEYS, CHVT and similar helpers.
fn c_macros(text: &str) -> HashMap<String, CMacro> {
    let mut macros = HashMap::new();
    let mut logical = String::new();
    for line in text.lines() {
        if let Some(head) = line.trim_end().strip_suffix('\\') {
            logical.push_str(head);
            logical.push(' ');
            continue;
        }
        logical.push_str(line);
        let definition = std::mem::take(&mut logical);
        let Some(rest) = definition.trim().strip_prefix("#define ") else {
            continue;
        };
        let Some(open) = rest.find('(') else {
            continue;
        };
        let name = &rest[..open];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let Some(close) = rest[open..].find(')') else {
            continue;
        };
        let params = rest[open + 1..open + close]
            .split(',')
            .map(|param| param.trim().to_string())
            .collect();
        macros.insert(
            name.to_string(),
            CMacro {
                params,
                body: rest[open + close + 1..].trim().to_string(),
            },
        );
    }
    macros
}

/// Rows of a C array initializer such as `keys[]`, with macro calls expanded.
/// Each row is its line number and its top-level fields.
fn c_array_rows(
    text: &str,
    marker: &str,
    macros: &HashMap<String, CMacro>,
    warnings: &mut Vec<String>,
) -> Vec<(usize, Vec<String>)> {
    let mut rows = vec![];
    let mut inside = false;
    let mut in_block_comment = false;
    let mut in_define = false;
    for (index, original) in text.lines().enumerate() {
        let line = strip_c_comments(original, &mut in_block_comment);
        let line = line.trim();
        if !inside {
            inside = line.contains(marker) && line.contains('{');
            continue;
        }
        if in_define || line.starts_with('#') {
            in_define = original.trim_end().ends_with('\\');
            continue;
        }
        if line.starts_with("};") {
            break;
        }
        for item in split_c_list(line) {
            expand_c_item(&item, index + 1, macros, 0, &mut rows, warnings);
        }
    }
    rows
}

fn expand_c_item(
    item: &str,
    line_number: usize,
    macros: &HashMap<String, CMacro>,
    depth: usize,
    rows: &mut Vec<(usize, Vec<String>)>,
    warnings: &mut Vec<String>,
) {
    let item = item.trim();
    if let Some(row) = item.strip_prefix('{') {
        let row = row.strip_suffix('}').unwrap_or(row);
        rows.push((line_number, split_c_list(row)));
        return;
    }
    let Some(open) = item.find('(') else {
        return;
    };
    let name = item[..open].trim();
    let Some(definition) = macros.get(name).filter(|_| depth < 4) else {
        warnings.push(format!("Could not expand {name} on line {line_number}"));
        return;
    };
    let arguments = item[open + 1..].trim_end();
    let arguments = split_c_list(arguments.strip_suffix(')').unwrap_or(arguments));
    let expanded = substitute_c_params(&definition.body, &definition.params, &arguments);
    for inner in split_c_list(&expanded) {
        expand_c_item(&inner, line_number, macros, depth + 1, rows, warnings);
    }
}

fn substitute_c_params(body: &str, params: &[String], arguments: &[String]) -> String {
    let mut output = String::new();
    let mut identifier = String::new();
    let flush = |identifier: &mut String, output: &mut String| {
        let replacement = params
            .iter()
            .position(|param| param == identifier)
            .and_then(|index| arguments.get(index));
        output.push_str(replacement.map_or(identifier.as_str(), String::as_str));
        identifier.clear();
    };
    for character in body.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            identifier.push(character);
        } else {
            flush(&mut identifier, &mut output);
            output.push(character);
        }
    }
    flush(&mut identifier, &mut output);
    output.replace(" ## ", "").replace("##", "")
}

/// A config's chord as dwl matches it. dwl compares a binding's keysym with
/// both the unshifted and the shifted symbol of the pressed key, so
/// `MODKEY|ShiftMask, XKB_KEY_T` and `XKB_KEY_t` are the same chord.
/// A shifted keysym bound without Shift can never match and yields None.
fn c_chord(
    raw_modifiers: &str,
    raw_key: &str,
    modkey: &str,
    prefix: &str,
) -> Option<(Vec<String>, String)> {
    let modifiers = parse_c_modifiers(raw_modifiers, modkey);
    let (key, shifted) = canonical_c_key(raw_key, prefix);
    if key.is_empty() || (shifted && !modifiers.iter().any(|modifier| modifier == "Shift")) {
        return None;
    }
    Some((modifiers, key))
}

fn c_action(function: &str, argument: Option<&str>) -> String {
    let function = function.trim();
    let Some(argument) = argument
        .map(str::trim)
        .filter(|argument| !argument.is_empty())
    else {
        return function.to_string();
    };
    let argument = argument
        .strip_prefix("SHCMD(")
        .and_then(|command| command.strip_suffix(')'))
        .map(|command| command.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| {
            let inner = argument
                .trim_start_matches('{')
                .trim_end_matches('}')
                .trim();
            let value = inner
                .split_once('=')
                .map_or(inner, |(_, value)| value)
                .trim();
            match value
                .strip_prefix("1 <<")
                .map(str::trim)
                .map(str::parse::<u32>)
            {
                Some(Ok(tag)) => format!("tag {}", tag + 1),
                _ => value.to_string(),
            }
        });
    if argument.is_empty() || argument == "0" {
        function.to_string()
    } else {
        format!("{function} {argument}")
    }
}

/// Strips `//` and `/* */` comments, leaving string literals intact.
fn strip_c_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut output = String::new();
    let mut quote = None;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if *in_block_comment {
            if character == '*' && characters.peek() == Some(&'/') {
                characters.next();
                *in_block_comment = false;
            }
            continue;
        }
        if let Some(active) = quote {
            output.push(character);
            if character == '\\' {
                if let Some(escaped) = characters.next() {
                    output.push(escaped);
                }
            } else if character == active {
                quote = None;
            }
            continue;
        }
        match (character, characters.peek()) {
            ('/', Some('/')) => break,
            ('/', Some('*')) => {
                characters.next();
                *in_block_comment = true;
            }
            ('"' | '\'', _) => {
                quote = Some(character);
                output.push(character);
            }
            _ => output.push(character),
        }
    }
    output
}

/// Splits on commas outside brackets and string literals.
fn split_c_list(row: &str) -> Vec<String> {
    let mut fields = vec![];
    let mut current = String::new();
    let mut depth = 0_i32;
    let mut quote = None;
    for character in row.chars() {
        if let Some(active) = quote {
            if character == active {
                quote = None;
            }
            current.push(character);
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth <= 0 => {
                fields.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    if !current.trim().is_empty() {
        fields.push(current.trim().to_string());
    }
    fields
}

fn parse_c_modifiers(raw: &str, modkey: &str) -> Vec<String> {
    let mut modifiers = vec![];
    let expanded = raw.replace("MODKEY", modkey);
    if expanded.contains("Mod4Mask") || expanded.contains("WLR_MODIFIER_LOGO") {
        modifiers.push("Super".into());
    }
    if expanded.contains("ControlMask") || expanded.contains("WLR_MODIFIER_CTRL") {
        modifiers.push("Ctrl".into());
    }
    if expanded.contains("Mod1Mask") || expanded.contains("WLR_MODIFIER_ALT") {
        modifiers.push("Alt".into());
    }
    if expanded.contains("ShiftMask") || expanded.contains("WLR_MODIFIER_SHIFT") {
        modifiers.push("Shift".into());
    }
    sort_modifiers(&mut modifiers);
    modifiers
}

/// The physical key of a keysym and whether the keysym needs Shift.
fn canonical_c_key(raw: &str, prefix: &str) -> (String, bool) {
    let name = raw.trim().trim_start_matches(prefix);
    let shifted_base = match name {
        "exclam" => Some("1"),
        "at" => Some("2"),
        "numbersign" => Some("3"),
        "dollar" => Some("4"),
        "percent" => Some("5"),
        "asciicircum" => Some("6"),
        "ampersand" => Some("7"),
        "asterisk" => Some("8"),
        "parenleft" => Some("9"),
        "parenright" => Some("0"),
        "underscore" => Some("MINUS"),
        "plus" => Some("EQUAL"),
        "braceleft" => Some("BRACKETLEFT"),
        "braceright" => Some("BRACKETRIGHT"),
        "bar" => Some("BACKSLASH"),
        "colon" => Some("SEMICOLON"),
        "quotedbl" => Some("APOSTROPHE"),
        "less" => Some("COMMA"),
        "greater" => Some("PERIOD"),
        "question" => Some("SLASH"),
        "asciitilde" => Some("GRAVE"),
        _ => None,
    };
    if let Some(base) = shifted_base {
        return (base.into(), true);
    }
    let shifted = name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase());
    (canonical_key(name), shifted)
}

fn binding(
    source: (&str, &str),
    scope_context: (&str, &str),
    chord: (Vec<String>, String),
    action: String,
    location: (&Path, usize),
    editable: bool,
) -> Binding {
    let (source_id, source_name) = source;
    let (scope, context) = scope_context;
    let (modifiers, key) = chord;
    let (path, line) = location;
    Binding {
        id: format!("{}:{}:{}", source_id, path.display(), line),
        source_id: source_id.into(),
        source_name: source_name.into(),
        scope: scope.into(),
        context: context.into(),
        modifiers,
        key,
        action,
        source_file: path.display().to_string(),
        line,
        editable,
    }
}

fn status_for(bindings: &[Binding], warnings: &[String]) -> String {
    if !warnings.is_empty() {
        "warning".into()
    } else if bindings.is_empty() {
        "empty".into()
    } else {
        "ready".into()
    }
}

fn strip_comment(line: &str) -> &str {
    line.split_once('#').map_or(line, |(content, _)| content)
}

fn expand_variables(value: &str, variables: &HashMap<String, String>) -> String {
    let mut expanded = value.to_string();
    for (name, replacement) in variables {
        expanded = expanded.replace(name, replacement);
    }
    expanded
}

fn resolve_include(
    parent: &Path,
    home: &Path,
    raw: &str,
    warnings: &mut Vec<String>,
) -> Vec<PathBuf> {
    let expanded = raw
        .replace("$HOME", &home.display().to_string())
        .replace("${HOME}", &home.display().to_string());
    let candidate = if let Some(relative) = expanded.strip_prefix("~/") {
        home.join(relative)
    } else {
        let path = PathBuf::from(expanded);
        if path.is_absolute() {
            path
        } else {
            parent.parent().unwrap_or(home).join(path)
        }
    };
    let pattern = candidate.display().to_string();
    if pattern.contains(['*', '?', '[']) {
        match glob(&pattern) {
            Ok(paths) => paths
                .filter_map(Result::ok)
                .filter(|path| path.is_file())
                .collect(),
            Err(_) => {
                warnings.push(format!("Invalid include pattern {pattern}"));
                vec![]
            }
        }
    } else if candidate.is_file() {
        vec![candidate]
    } else {
        warnings.push(format!("Included file not found: {}", candidate.display()));
        vec![]
    }
}

fn parse_chord(raw: &str) -> (Vec<String>, String) {
    let mut parts: Vec<_> = raw.split('+').filter(|part| !part.is_empty()).collect();
    let key = canonical_key(parts.pop().unwrap_or_default());
    (parse_modifiers(&parts.join(" ")), key)
}

fn parse_tmux_chord(raw: &str) -> (Vec<String>, String) {
    let mut modifiers = vec![];
    let mut key = raw;
    while let Some((prefix, tail)) = key.split_once('-') {
        match prefix.to_ascii_uppercase().as_str() {
            "C" => modifiers.push("Ctrl".into()),
            "M" => modifiers.push("Alt".into()),
            "S" => modifiers.push("Shift".into()),
            _ => break,
        }
        key = tail;
    }
    sort_modifiers(&mut modifiers);
    (modifiers, canonical_key(key))
}

pub(crate) fn parse_modifiers(raw: &str) -> Vec<String> {
    let mut modifiers = raw
        .split_whitespace()
        .filter_map(|part| match part.trim().to_ascii_uppercase().as_str() {
            "SUPER" | "META" | "MOD4" | "CMD" | "COMMAND" | "WIN" | "LOGO" => {
                Some("Super".to_string())
            }
            "CTRL" | "CONTROL" => Some("Ctrl".to_string()),
            "ALT" | "MOD1" | "OPT" | "OPTION" => Some("Alt".to_string()),
            "SHIFT" => Some("Shift".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    sort_modifiers(&mut modifiers);
    modifiers
}

fn sort_modifiers(modifiers: &mut Vec<String>) {
    modifiers.sort_by_key(|modifier| match modifier.as_str() {
        "Super" => 0,
        "Ctrl" => 1,
        "Alt" => 2,
        "Shift" => 3,
        _ => 4,
    });
    modifiers.dedup();
}

pub(crate) fn canonical_key(raw: &str) -> String {
    let key = raw.trim().trim_matches(['\'', '"']).to_ascii_uppercase();
    match key.as_str() {
        "RETURN" | "KP_ENTER" => "ENTER".into(),
        "ESCAPE" => "ESC".into(),
        "SPACEBAR" => "SPACE".into(),
        "PGUP" | "PRIOR" => "PAGEUP".into(),
        "PGDN" | "NEXT" => "PAGEDOWN".into(),
        "DEL" => "DELETE".into(),
        "BS" => "BACKSPACE".into(),
        "PAGE_UP" => "PAGEUP".into(),
        "PAGE_DOWN" => "PAGEDOWN".into(),
        "PRINT_SCREEN" => "PRINT".into(),
        "`" | "GRAVE_ACCENT" => "GRAVE".into(),
        "-" => "MINUS".into(),
        "=" => "EQUAL".into(),
        "[" => "BRACKETLEFT".into(),
        "]" => "BRACKETRIGHT".into(),
        "\\" => "BACKSLASH".into(),
        ";" => "SEMICOLON".into(),
        "'" => "APOSTROPHE".into(),
        "," => "COMMA".into(),
        "." => "PERIOD".into(),
        "/" => "SLASH".into(),
        _ => key.strip_prefix("KEY_").unwrap_or(&key).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalizes_common_chords() {
        assert_eq!(
            parse_chord("ctrl+shift+return"),
            (
                vec!["Ctrl".to_string(), "Shift".to_string()],
                "ENTER".to_string()
            )
        );
        assert_eq!(
            parse_tmux_chord("C-M-x"),
            (vec!["Ctrl".to_string(), "Alt".to_string()], "X".to_string())
        );
    }

    #[test]
    fn expands_hyprland_modifiers() {
        let variables = HashMap::from([("$mainMod".to_string(), "SUPER".to_string())]);
        let expanded = expand_variables("$mainMod SHIFT", &variables);
        assert_eq!(parse_modifiers(&expanded), vec!["Super", "Shift"]);
    }

    #[test]
    fn follows_hyprland_sources_and_uses_descriptions() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("keyventory-test-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).expect("fixture directory should be created");
        let main = root.join("hyprland.conf");
        fs::write(
            &main,
            "$mainMod = SUPER\nbind = $mainMod SHIFT, Q, exec, kitty\nsource = extra.conf\n",
        )
        .expect("main fixture should be written");
        fs::write(
            root.join("extra.conf"),
            "bindd = SUPER, Return, Open terminal, exec, kitty\n",
        )
        .expect("included fixture should be written");

        let mut variables = HashMap::new();
        let mut visited = HashSet::new();
        let mut files = vec![];
        let mut bindings = vec![];
        let mut warnings = vec![];
        parse_hyprland_file(
            &main,
            &root,
            &mut variables,
            &mut visited,
            &mut files,
            &mut bindings,
            &mut warnings,
        );

        assert_eq!(files.len(), 2);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].modifiers, vec!["Super", "Shift"]);
        assert_eq!(bindings[1].action, "Open terminal");
        assert!(warnings.is_empty());
        fs::remove_dir_all(&root).expect("fixture directory should be removed");
    }

    #[test]
    fn decodes_live_hyprland_lua_bindings() {
        let json = br#"[{"modmask":65,"key":"Return","description":"Open terminal","submap":""}]"#;
        let bindings = parse_hyprctl_bindings(json, Path::new("/tmp/hyprland.lua"))
            .expect("hyprctl fixture should decode");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].modifiers, vec!["Super", "Shift"]);
        assert_eq!(bindings[0].key, "ENTER");
        assert_eq!(bindings[0].action, "Open terminal");
        assert!(!bindings[0].editable);
    }

    #[test]
    fn parses_neovim_keys_and_sequences() {
        assert_eq!(
            parse_neovim_chord("<C-S-Left>"),
            Some((vec!["Ctrl".into(), "Shift".into()], "LEFT".into(), false))
        );
        assert_eq!(
            parse_neovim_chord("<Space>p"),
            Some((vec![], "SPACE".into(), true))
        );
        assert_eq!(
            parse_neovim_chord(":"),
            Some((vec!["Shift".into()], "SEMICOLON".into(), false))
        );
    }

    #[test]
    fn parses_dwm_rows_and_tag_macros() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("keyventory-dwm-{}-{nonce}", std::process::id()));
        let config_dir = root.join("src/dwm");
        fs::create_dir_all(&config_dir).expect("fixture directory should be created");
        fs::write(
            config_dir.join("config.def.h"),
            "#define MODKEY Mod4Mask\n\
             #define TAGKEYS(KEY,TAG) \\\n\
             { MODKEY, KEY, view, {.ui = 1 << TAG} }, \\\n\
             { MODKEY|ControlMask, KEY, toggleview, {.ui = 1 << TAG} }, \\\n\
             { MODKEY|ShiftMask, KEY, tag, {.ui = 1 << TAG} }, \\\n\
             { MODKEY|ControlMask|ShiftMask, KEY, toggletag, {.ui = 1 << TAG} }\n\
             static Key keys[] = {\n\
             { MODKEY|ShiftMask, XK_Return, spawn, {0} },\n\
             TAGKEYS(XK_q, 0)\n};\n",
        )
        .expect("dwm fixture should be written");

        let source = scan_dwm(&root);
        assert_eq!(source.bindings.len(), 5);
        assert_eq!(source.bindings[0].modifiers, vec!["Super", "Shift"]);
        assert_eq!(source.bindings[0].key, "ENTER");
        assert_eq!(source.bindings[1].action, "view tag 1");
        assert!(source.warnings.is_empty());
        fs::remove_dir_all(&root).expect("fixture directory should be removed");
    }

    #[test]
    fn expands_dwl_macros_shifted_keysyms_and_lockkeys() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should follow the Unix epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!("keyventory-dwl-{}-{nonce}", std::process::id()));
        let config_dir = root.join("src/dwl");
        fs::create_dir_all(&config_dir).expect("fixture directory should be created");
        fs::write(
            config_dir.join("config.h"),
            r#"#define MODKEY WLR_MODIFIER_LOGO
#define TAGKEYS(KEY,TAG) \
	{ MODKEY,                    KEY, vieworlaunch, {.ui = 1 << TAG} }, \
	{ MODKEY|WLR_MODIFIER_SHIFT, KEY, tag,          {.ui = 1 << TAG} }
#define NOMOD 0
static const Key keys[] = {
	{ MODKEY, XKB_KEY_g, spawn, SHCMD("search 'https://google.com/?q='") }, // comment
	{ MODKEY|WLR_MODIFIER_SHIFT, XKB_KEY_N, spawn, SHCMD("browser --incognito") },
	{ MODKEY, XKB_KEY_T, spawn, SHCMD("never") },
	{ MODKEY|WLR_MODIFIER_SHIFT, XKB_KEY_less, tagmon, {.i = -1} },
	{ MODKEY, XKB_KEY_1, togglescratch, {.ui = 1} },
	TAGKEYS(XKB_KEY_q, 0),
	{ WLR_MODIFIER_CTRL|WLR_MODIFIER_ALT,XKB_KEY_s, spawn, SHCMD("systemctl suspend") },
#define CHVT(n) { WLR_MODIFIER_CTRL|WLR_MODIFIER_ALT,XKB_KEY_F##n, chvt, {.ui = (n)} }
	CHVT(1), CHVT(2),
};
static const LockKey lockkeys[] = {
	{ WLR_MODIFIER_CTRL|WLR_MODIFIER_ALT, XKB_KEY_s },
};
"#,
        )
        .expect("dwl fixture should be written");

        let source = scan_dwl(&root);
        let find = |modifiers: &[&str], key: &str| {
            source
                .bindings
                .iter()
                .find(|item| item.modifiers == modifiers && item.key == key)
        };
        let search = find(&["Super"], "G").expect("Super+G should be bound");
        assert_eq!(search.action, "spawn search 'https://google.com/?q='");
        assert_eq!(search.line, 7);
        assert!(find(&["Super", "Shift"], "N").is_some());
        assert!(find(&["Super"], "T").is_none());
        assert_eq!(source.warnings.len(), 1);
        assert_eq!(
            find(&["Super", "Shift"], "COMMA").map(|item| item.action.as_str()),
            Some("tagmon -1")
        );
        assert_eq!(
            find(&["Super"], "1").map(|item| item.action.as_str()),
            Some("togglescratch 1")
        );
        assert_eq!(
            find(&["Super"], "Q").map(|item| item.action.as_str()),
            Some("vieworlaunch tag 1")
        );
        assert!(find(&["Super", "Shift"], "Q").is_some());
        let suspend = find(&["Ctrl", "Alt"], "S").expect("Ctrl+Alt+S should be bound");
        assert_eq!(suspend.context, "keys, works while locked");
        assert!(find(&["Ctrl", "Alt"], "F2").is_some());
        assert_eq!(source.bindings.len(), 9);
        fs::remove_dir_all(&root).expect("fixture directory should be removed");
    }

    #[test]
    fn decodes_zsh_bindkey_listing() {
        let listing = r#"bindkey "^A" beginning-of-line
bindkey "^E" edit-command-line
bindkey "^X^E" edit-command-line
bindkey "^Xu" undo
bindkey "^[[1;5C" forward-word
bindkey "^[[Z" autosuggest-accept
bindkey "^[." insert-last-word
bindkey "^@" autosuggest-accept
bindkey "^?" backward-delete-char
bindkey "a" self-insert
bindkey "q" push-line
bindkey -R " -~" self-insert
"#;
        let bindings = parse_zsh_bindkeys(
            listing,
            Path::new("/home/u/.zshrc"),
            "bindkey '^e' edit-command-line\n",
        );
        let chords: Vec<_> = bindings
            .iter()
            .map(|item| format!("{}+{}", item.modifiers.join("+"), item.key))
            .collect();
        assert_eq!(
            chords,
            [
                "Ctrl+A",
                "Ctrl+E",
                "Ctrl+X",
                "Ctrl+RIGHT",
                "Shift+TAB",
                "Alt+PERIOD",
                "Ctrl+SPACE",
                "+BACKSPACE"
            ]
        );
        assert_eq!(bindings[1].line, 1);
        assert_eq!(
            bindings[2].action,
            "prefix · ^X^E edit-command-line; ^Xu undo"
        );
    }
}
