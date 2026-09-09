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

    ScanResult {
        sources: vec![
            scan_hyprland(&home),
            scan_kitty(&home),
            scan_sxhkd(&home),
            scan_tmux(&home),
            scan_neovim(&home),
            scan_dwm(&home),
            scan_dwl(&home),
        ],
    }
}

fn missing_source(id: &str, name: &str, kind: &str, scope: &str, path: &Path) -> SourceScan {
    SourceScan {
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
    warnings.push("Built-in Kitty defaults are not included yet".into());
    parse_kitty_file(
        &path,
        home,
        &mut kitty_mod,
        &mut visited,
        &mut files,
        &mut bindings,
        &mut warnings,
    );

    SourceScan {
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
        if action.is_empty() || action == "no_op" {
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
        .unwrap_or("Mod4Mask");
    let mut bindings = vec![];
    let mut in_keys = false;
    let mut in_block_comment = false;

    for (index, original) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_c_comments(original, &mut in_block_comment);
        let line = line.trim();
        if !in_keys {
            in_keys = line.contains("Key keys[]") && line.contains('{');
            continue;
        }
        if line.starts_with("};") {
            break;
        }
        if let Some(arguments) = line.strip_prefix("TAGKEYS(") {
            let arguments = arguments.trim_end_matches([',', ')']).trim();
            let fields = split_c_fields(arguments);
            if let Some(raw_key) = fields.first() {
                let key = canonical_c_key(raw_key, key_prefix);
                for (suffix, action, extra_modifiers) in [
                    ("view", "view tag", vec![]),
                    ("toggleview", "toggle tag view", vec!["Ctrl"]),
                    ("tag", "move client to tag", vec!["Shift"]),
                    ("toggletag", "toggle client tag", vec!["Ctrl", "Shift"]),
                ] {
                    let mut modifiers = parse_c_modifiers("MODKEY", modkey);
                    modifiers.extend(extra_modifiers.into_iter().map(str::to_string));
                    sort_modifiers(&mut modifiers);
                    let mut item = binding(
                        (id, name),
                        ("global", "tags"),
                        (modifiers, key.clone()),
                        action.into(),
                        (path, line_number),
                        true,
                    );
                    item.id.push_str(&format!(":{suffix}"));
                    bindings.push(item);
                }
            }
            continue;
        }
        let Some(row) = line.strip_prefix('{') else {
            continue;
        };
        let fields = split_c_fields(row);
        if fields.len() < 3 {
            continue;
        }
        let key = canonical_c_key(fields[1], key_prefix);
        if key.is_empty() {
            continue;
        }
        bindings.push(binding(
            (id, name),
            ("global", "keys"),
            (parse_c_modifiers(fields[0], modkey), key),
            fields[2].trim().to_string(),
            (path, line_number),
            true,
        ));
    }

    SourceScan {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        scope: "global".into(),
        status: status_for(&bindings, &[]),
        config_paths: vec![path.display().to_string()],
        bindings,
        warnings: vec![],
    }
}

fn strip_c_comments<'a>(line: &'a str, in_block_comment: &mut bool) -> &'a str {
    if *in_block_comment {
        if let Some((_, tail)) = line.split_once("*/") {
            *in_block_comment = false;
            return tail;
        }
        return "";
    }
    let line = line.split_once("//").map_or(line, |(content, _)| content);
    if let Some((content, _)) = line.split_once("/*") {
        *in_block_comment = !line.contains("*/");
        content
    } else {
        line
    }
}

fn split_c_fields(row: &str) -> Vec<&str> {
    let mut fields = vec![];
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quote = None;
    for (index, character) in row.char_indices() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth <= 0 => {
                fields.push(row[start..index].trim());
                start = index + 1;
                if fields.len() == 3 {
                    break;
                }
            }
            _ => {}
        }
    }
    if fields.len() < 3 && start < row.len() {
        fields.push(row[start..].trim());
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

fn canonical_c_key(raw: &str, prefix: &str) -> String {
    canonical_key(raw.trim().trim_start_matches(prefix))
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

fn parse_modifiers(raw: &str) -> Vec<String> {
    let mut modifiers = raw
        .split_whitespace()
        .filter_map(|part| match part.trim().to_ascii_uppercase().as_str() {
            "SUPER" | "META" | "MOD4" | "CMD" | "COMMAND" => Some("Super".to_string()),
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

fn canonical_key(raw: &str) -> String {
    let key = raw.trim().trim_matches(['\'', '"']).to_ascii_uppercase();
    match key.as_str() {
        "RETURN" | "KP_ENTER" => "ENTER".into(),
        "ESCAPE" => "ESC".into(),
        "SPACEBAR" => "SPACE".into(),
        "PGUP" | "PRIOR" => "PAGEUP".into(),
        "PGDN" | "NEXT" => "PAGEDOWN".into(),
        "DEL" => "DELETE".into(),
        "BS" => "BACKSPACE".into(),
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
            "#define MODKEY Mod4Mask\nstatic Key keys[] = {\n\
             { MODKEY|ShiftMask, XK_Return, spawn, {0} },\n\
             TAGKEYS(XK_q, 0)\n};\n",
        )
        .expect("dwm fixture should be written");

        let source = scan_dwm(&root);
        assert_eq!(source.bindings.len(), 5);
        assert_eq!(source.bindings[0].modifiers, vec!["Super", "Shift"]);
        assert_eq!(source.bindings[0].key, "ENTER");
        assert_eq!(source.bindings[1].action, "view tag");
        fs::remove_dir_all(&root).expect("fixture directory should be removed");
    }
}
