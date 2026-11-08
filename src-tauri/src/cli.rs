//! Headless command line: `keyventory free` and `keyventory check`.
//! It never opens a window, so agents and scripts can call it.

use crate::scanner::{self, Binding, ScanResult};
use serde::Serialize;

const USAGE: &str = "\
Usage:
  keyventory                  open the desktop app
  keyventory free [--scope global|all] [--mods super,super+shift] [--json]
  keyventory check CHORD [--scope global|all] [--json]

free   lists chords that no blocking source binds.
check  says whether CHORD (for example super+shift+t) is taken, and by what.

--scope global  (default) a chord is taken when a running compositor or
                hotkey daemon binds it; application bindings are warnings.
--scope all     application bindings block too.
check exits 0 when the chord is free, 1 when taken, 2 on bad usage.";

/// Keys `free` considers, in keyboard order.
const KEYS: &[&str] = &[
    "ESC",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "PRINT",
    "GRAVE",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "0",
    "MINUS",
    "EQUAL",
    "BACKSPACE",
    "TAB",
    "Q",
    "W",
    "E",
    "R",
    "T",
    "Y",
    "U",
    "I",
    "O",
    "P",
    "BRACKETLEFT",
    "BRACKETRIGHT",
    "BACKSLASH",
    "A",
    "S",
    "D",
    "F",
    "G",
    "H",
    "J",
    "K",
    "L",
    "SEMICOLON",
    "APOSTROPHE",
    "ENTER",
    "Z",
    "X",
    "C",
    "V",
    "B",
    "N",
    "M",
    "COMMA",
    "PERIOD",
    "SLASH",
    "SPACE",
    "INSERT",
    "DELETE",
    "HOME",
    "END",
    "PAGEUP",
    "PAGEDOWN",
    "LEFT",
    "UP",
    "DOWN",
    "RIGHT",
];

type Chord = (Vec<String>, String);

/// Runs a subcommand and returns its exit code, or None to start the GUI.
pub fn run(args: &[String]) -> Option<i32> {
    let command = args.first()?;
    let rest = &args[1..];
    let result = match command.as_str() {
        "free" => free(rest),
        "check" => check(rest),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(0)
        }
        other => Err(format!("unknown command: {other}")),
    };
    Some(result.unwrap_or_else(|error| {
        eprintln!("keyventory: {error}\n\n{USAGE}");
        2
    }))
}

struct Options {
    scope_all: bool,
    json: bool,
    mods: Option<String>,
    positional: Vec<String>,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        scope_all: false,
        json: false,
        mods: None,
        positional: vec![],
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || {
            inline
                .clone()
                .or_else(|| args.next().cloned())
                .ok_or(format!("{flag} needs a value"))
        };
        match flag {
            "--json" => options.json = true,
            "--scope" => {
                options.scope_all = match value()?.as_str() {
                    "global" => false,
                    "all" => true,
                    other => return Err(format!("unknown scope: {other}")),
                }
            }
            "--mods" => options.mods = Some(value()?),
            _ if flag.starts_with('-') && flag.len() > 1 => {
                return Err(format!("unknown option: {flag}"))
            }
            _ => options.positional.push(arg.clone()),
        }
    }
    Ok(options)
}

/// Parses `super+shift+t` style text. Every part but the last must be a
/// modifier; the last is the key.
fn parse_chord(text: &str) -> Result<Chord, String> {
    let parts: Vec<_> = text.split('+').map(str::trim).collect();
    let (key, modifier_parts) = parts.split_last().ok_or("empty chord")?;
    let modifiers = parse_modifier_list(modifier_parts)?;
    let key = scanner::canonical_key(key);
    if key.is_empty() {
        return Err(format!("no key in chord: {text}"));
    }
    Ok((modifiers, key))
}

fn parse_modifier_list(parts: &[&str]) -> Result<Vec<String>, String> {
    let mut modifiers = vec![];
    for part in parts
        .iter()
        .filter(|part| !part.is_empty() && **part != "none")
    {
        let parsed = scanner::parse_modifiers(part);
        if parsed.is_empty() {
            return Err(format!("unknown modifier: {part}"));
        }
        modifiers.extend(parsed);
    }
    Ok(scanner::parse_modifiers(&modifiers.join(" ")))
}

fn chord_label((modifiers, key): &Chord) -> String {
    modifiers
        .iter()
        .chain(std::iter::once(key))
        .cloned()
        .collect::<Vec<_>>()
        .join("+")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BindingOut {
    source: String,
    source_name: String,
    scope: String,
    context: String,
    chord: String,
    action: String,
    file: String,
    line: usize,
}

impl BindingOut {
    fn new(binding: &Binding) -> Self {
        BindingOut {
            source: binding.source_id.clone(),
            source_name: binding.source_name.clone(),
            scope: binding.scope.clone(),
            context: binding.context.clone(),
            chord: chord_label(&(binding.modifiers.clone(), binding.key.clone())),
            action: binding.action.clone(),
            file: binding.source_file.clone(),
            line: binding.line,
        }
    }

    fn text(&self) -> String {
        let location = if self.line > 0 {
            format!("{}:{}", self.file, self.line)
        } else {
            self.file.clone()
        };
        format!(
            "{} [{}]: {}  ({location})",
            self.source_name, self.context, self.action
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceOut {
    id: String,
    name: String,
    scope: String,
    status: String,
    active: bool,
    bindings: usize,
    warnings: Vec<String>,
}

/// What binds one chord: blockers make it taken, clashes are application
/// bindings it would shadow, and inactive are bindings in sources that are
/// not running.
struct Lookup {
    blockers: Vec<BindingOut>,
    clashes: Vec<BindingOut>,
    inactive: Vec<BindingOut>,
}

fn lookup(scan: &ScanResult, chord: &Chord, scope_all: bool) -> Lookup {
    let mut found = Lookup {
        blockers: vec![],
        clashes: vec![],
        inactive: vec![],
    };
    for source in &scan.sources {
        for binding in &source.bindings {
            if binding.modifiers != chord.0 || binding.key != chord.1 {
                continue;
            }
            let out = BindingOut::new(binding);
            if !source.active {
                found.inactive.push(out);
            } else if scope_all || source.scope == "global" {
                found.blockers.push(out);
            } else {
                found.clashes.push(out);
            }
        }
    }
    found
}

fn sources_out(scan: &ScanResult) -> Vec<SourceOut> {
    scan.sources
        .iter()
        .map(|source| SourceOut {
            id: source.id.clone(),
            name: source.name.clone(),
            scope: source.scope.clone(),
            status: source.status.clone(),
            active: source.active,
            bindings: source.bindings.len(),
            warnings: source.warnings.clone(),
        })
        .collect()
}

fn scope_name(scope_all: bool) -> &'static str {
    if scope_all {
        "all"
    } else {
        "global"
    }
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("CLI output should serialize")
    );
}

fn check(args: &[String]) -> Result<i32, String> {
    let options = parse_options(args)?;
    if options.mods.is_some() {
        return Err("--mods applies to free".into());
    }
    let [chord_text] = options.positional.as_slice() else {
        return Err("check needs exactly one CHORD".into());
    };
    let chord = parse_chord(chord_text)?;
    let scan = scanner::scan_all();
    let found = lookup(&scan, &chord, options.scope_all);
    let free = found.blockers.is_empty();

    if options.json {
        #[derive(Serialize)]
        struct CheckOut<'a> {
            version: u32,
            chord: String,
            modifiers: &'a [String],
            key: &'a str,
            scope: &'a str,
            free: bool,
            blockers: Vec<BindingOut>,
            clashes: Vec<BindingOut>,
            inactive: Vec<BindingOut>,
            sources: Vec<SourceOut>,
        }
        print_json(&CheckOut {
            version: 1,
            chord: chord_label(&chord),
            modifiers: &chord.0,
            key: &chord.1,
            scope: scope_name(options.scope_all),
            free,
            blockers: found.blockers,
            clashes: found.clashes,
            inactive: found.inactive,
            sources: sources_out(&scan),
        });
    } else {
        println!(
            "{}: {}",
            chord_label(&chord),
            if free { "free" } else { "taken" }
        );
        for binding in &found.blockers {
            println!("  taken by {}", binding.text());
        }
        for binding in &found.clashes {
            println!("  warning: shadows {}", binding.text());
        }
        for binding in &found.inactive {
            println!("  not running: {}", binding.text());
        }
    }
    Ok(if free { 0 } else { 1 })
}

fn free(args: &[String]) -> Result<i32, String> {
    let options = parse_options(args)?;
    if !options.positional.is_empty() {
        return Err(format!("unexpected argument: {}", options.positional[0]));
    }
    let mod_sets = options
        .mods
        .as_deref()
        .unwrap_or("super,super+shift")
        .split(',')
        .map(|set| parse_modifier_list(&set.split('+').map(str::trim).collect::<Vec<_>>()))
        .collect::<Result<Vec<_>, _>>()?;
    let scan = scanner::scan_all();

    #[derive(Serialize)]
    struct FreeChord {
        chord: String,
        modifiers: Vec<String>,
        key: String,
        clashes: Vec<BindingOut>,
    }
    let mut free_chords = vec![];
    for modifiers in &mod_sets {
        for key in KEYS {
            let chord = (modifiers.clone(), key.to_string());
            let found = lookup(&scan, &chord, options.scope_all);
            if found.blockers.is_empty() {
                free_chords.push(FreeChord {
                    chord: chord_label(&chord),
                    modifiers: chord.0,
                    key: chord.1,
                    clashes: found.clashes,
                });
            }
        }
    }

    if options.json {
        #[derive(Serialize)]
        struct FreeOut<'a> {
            version: u32,
            scope: &'a str,
            mods: &'a [Vec<String>],
            free: Vec<FreeChord>,
            sources: Vec<SourceOut>,
        }
        print_json(&FreeOut {
            version: 1,
            scope: scope_name(options.scope_all),
            mods: &mod_sets,
            free: free_chords,
            sources: sources_out(&scan),
        });
        return Ok(0);
    }

    for modifiers in &mod_sets {
        let keys: Vec<_> = free_chords
            .iter()
            .filter(|chord| &chord.modifiers == modifiers)
            .map(|chord| {
                if chord.clashes.is_empty() {
                    chord.key.clone()
                } else {
                    format!("{}*", chord.key)
                }
            })
            .collect();
        let label = if modifiers.is_empty() {
            "(none)".to_string()
        } else {
            modifiers.join("+")
        };
        println!("{label}: {}", keys.join(" "));
    }
    let clashes: Vec<_> = free_chords
        .iter()
        .flat_map(|chord| chord.clashes.iter())
        .collect();
    if !clashes.is_empty() {
        println!("\n* free, but shadows an application binding:");
        for binding in clashes {
            println!("  {}  {}", binding.chord, binding.text());
        }
    }
    let ignored: Vec<_> = scan
        .sources
        .iter()
        .filter(|source| !source.active && source.status != "missing")
        .map(|source| source.name.as_str())
        .collect();
    if !ignored.is_empty() {
        println!("\nNot running, ignored: {}", ignored.join(", "));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chord_text() {
        assert_eq!(
            parse_chord("super+shift+t"),
            Ok((vec!["Super".into(), "Shift".into()], "T".into()))
        );
        assert_eq!(
            parse_chord("Shift+Win+Backspace"),
            Ok((vec!["Super".into(), "Shift".into()], "BACKSPACE".into()))
        );
        assert!(parse_chord("hyper+t").is_err());
        assert!(parse_chord("super+").is_err());
    }

    #[test]
    fn separates_blockers_clashes_and_inactive() {
        let binding = |source: &str, scope: &str| Binding {
            id: source.into(),
            source_id: source.into(),
            source_name: source.into(),
            scope: scope.into(),
            context: "keys".into(),
            modifiers: vec!["Super".into()],
            key: "T".into(),
            action: "act".into(),
            source_file: "/f".into(),
            line: 1,
            editable: false,
        };
        let source = |id: &str, scope: &str, active: bool| scanner::SourceScan {
            id: id.into(),
            name: id.into(),
            kind: "k".into(),
            scope: scope.into(),
            status: "ready".into(),
            active,
            config_paths: vec![],
            bindings: vec![binding(id, scope)],
            warnings: vec![],
        };
        let scan = ScanResult {
            sources: vec![
                source("dwl", "global", true),
                source("kitty", "application", true),
                source("hyprland", "global", false),
            ],
        };
        let chord = (vec!["Super".to_string()], "T".to_string());
        let found = lookup(&scan, &chord, false);
        assert_eq!(found.blockers.len(), 1);
        assert_eq!(found.clashes[0].source, "kitty");
        assert_eq!(found.inactive[0].source, "hyprland");
        assert_eq!(lookup(&scan, &chord, true).blockers.len(), 2);
    }
}
