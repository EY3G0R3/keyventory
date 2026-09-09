use glob::glob;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
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
    let path = home.join(".config/hypr/hyprland.conf");
    if !path.is_file() {
        return missing_source("hyprland", "Hyprland", "compositor", "global", &path);
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
}
