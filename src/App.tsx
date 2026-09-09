import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type Binding = {
  id: string;
  sourceId: string;
  sourceName: string;
  scope: "global" | "application" | string;
  context: string;
  modifiers: string[];
  key: string;
  action: string;
  sourceFile: string;
  line: number;
  editable: boolean;
};

type SourceScan = {
  id: string;
  name: string;
  kind: string;
  scope: string;
  status: "ready" | "warning" | "empty" | "missing";
  configPaths: string[];
  bindings: Binding[];
  warnings: string[];
};

type ScanResult = { sources: SourceScan[] };
type KeyDef = { code: string; label: string; width?: number };

const MODIFIER_ORDER = ["Super", "Ctrl", "Alt", "Shift"];
const SOURCE_COLORS: Record<string, string> = {
  hyprland: "#b6d96c",
  kitty: "#e6a15a",
  sxhkd: "#78b7c5",
  tmux: "#b48bc8",
  neovim: "#76b98d",
  dwm: "#d98278",
  dwl: "#779fda",
};
const sourceColor = (sourceId: string) => SOURCE_COLORS[sourceId] ?? "#9aa6b2";

function sourceFill(bindings: Binding[]) {
  const colors = [...new Set(bindings.map((binding) => sourceColor(binding.sourceId)))];
  if (!colors.length) return "none";
  if (colors.length === 1) return `linear-gradient(135deg, ${colors[0]}, ${colors[0]})`;
  const stops = colors.flatMap((color, index) => [
    `${color} ${(index / colors.length) * 100}%`,
    `${color} ${((index + 1) / colors.length) * 100}%`,
  ]);
  return `linear-gradient(135deg, ${stops.join(", ")})`;
}
const key = (code: string, label = code, width = 1): KeyDef => ({ code, label, width });
const KEYBOARD: KeyDef[][] = [
  [key("ESC", "Esc"), key("F1"), key("F2"), key("F3"), key("F4"), key("F5"), key("F6"), key("F7"), key("F8"), key("F9"), key("F10"), key("F11"), key("F12")],
  [key("GRAVE", "`"), key("1"), key("2"), key("3"), key("4"), key("5"), key("6"), key("7"), key("8"), key("9"), key("0"), key("MINUS", "−"), key("EQUAL", "="), key("BACKSPACE", "Backspace", 2)],
  [key("TAB", "Tab", 1.5), key("Q"), key("W"), key("E"), key("R"), key("T"), key("Y"), key("U"), key("I"), key("O"), key("P"), key("BRACKETLEFT", "["), key("BRACKETRIGHT", "]"), key("BACKSLASH", "\\", 1.5)],
  [key("CAPSLOCK", "Caps", 1.8), key("A"), key("S"), key("D"), key("F"), key("G"), key("H"), key("J"), key("K"), key("L"), key("SEMICOLON", ";"), key("APOSTROPHE", "'"), key("ENTER", "Enter", 2.2)],
  [key("SHIFTLEFT", "Shift", 2.3), key("Z"), key("X"), key("C"), key("V"), key("B"), key("N"), key("M"), key("COMMA", ","), key("PERIOD", "."), key("SLASH", "/"), key("SHIFTRIGHT", "Shift", 2.7)],
  [key("CTRLLEFT", "Ctrl", 1.4), key("SUPERLEFT", "Super", 1.4), key("ALTLEFT", "Alt", 1.4), key("SPACE", "Space", 6), key("ALTRIGHT", "Alt", 1.4), key("SUPERRIGHT", "Super", 1.4), key("CTRLRIGHT", "Ctrl", 1.4)],
  [key("INSERT", "Ins"), key("HOME", "Home"), key("PAGEUP", "PgUp"), key("DELETE", "Del"), key("END", "End"), key("PAGEDOWN", "PgDn"), key("LEFT", "←"), key("UP", "↑"), key("DOWN", "↓"), key("RIGHT", "→")],
];

const demoSources: SourceScan[] = [
  {
    id: "hyprland", name: "Hyprland", kind: "compositor", scope: "global", status: "ready",
    configPaths: ["~/.config/hypr/hyprland.conf"], warnings: [],
    bindings: [
      demoBinding("hyprland", "Hyprland", "global", ["Super"], "ENTER", "exec kitty", 42),
      demoBinding("hyprland", "Hyprland", "global", ["Super"], "Q", "killactive", 58),
      demoBinding("hyprland", "Hyprland", "global", ["Super", "Shift"], "S", "exec screenshot", 73),
    ],
  },
  {
    id: "kitty", name: "Kitty", kind: "terminal", scope: "application", status: "ready",
    configPaths: ["~/.config/kitty/kitty.conf"], warnings: [],
    bindings: [
      demoBinding("kitty", "Kitty", "application", ["Ctrl", "Shift"], "T", "new_tab", 16),
      demoBinding("kitty", "Kitty", "application", ["Super"], "Q", "close_window", 24),
    ],
  },
  {
    id: "tmux", name: "tmux", kind: "terminal multiplexer", scope: "application", status: "ready",
    configPaths: ["~/.tmux.conf"], warnings: [],
    bindings: [demoBinding("tmux", "tmux", "application", ["Ctrl", "Shift"], "T", "new-window", 11)],
  },
];

function demoBinding(sourceId: string, sourceName: string, scope: string, modifiers: string[], bindingKey: string, action: string, line: number): Binding {
  return {
    id: `${sourceId}:${line}`, sourceId, sourceName, scope, context: sourceId,
    modifiers, key: bindingKey, action, sourceFile: `~/.config/${sourceId}/config`, line, editable: true,
  };
}

function sameModifiers(left: string[], right: string[]) {
  return MODIFIER_ORDER.every((modifier) => left.includes(modifier) === right.includes(modifier));
}

function chordLabel(binding: Binding) {
  return [...binding.modifiers, binding.key].join(" + ");
}

function App() {
  const [sources, setSources] = useState<SourceScan[]>([]);
  const [enabledSources, setEnabledSources] = useState<Set<string>>(new Set());
  const [activeModifiers, setActiveModifiers] = useState<string[]>(["Super"]);
  const [selectedKey, setSelectedKey] = useState("ENTER");
  const [query, setQuery] = useState("");
  const [scanning, setScanning] = useState(true);
  const [isDemo, setIsDemo] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setScanning(true);
    setScanError(null);
    try {
      const inDesktop = "__TAURI_INTERNALS__" in window;
      const result = inDesktop ? await invoke<ScanResult>("scan_bindings") : { sources: demoSources };
      setSources(result.sources);
      setEnabledSources(new Set(result.sources.filter((source) => source.status !== "missing").map((source) => source.id)));
      setIsDemo(!inDesktop);
    } catch (error) {
      setScanError(error instanceof Error ? error.message : String(error));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => { void scan(); }, [scan]);

  useEffect(() => {
    const modifierFor = (event: KeyboardEvent) => {
      if (event.key === "Meta" || event.key === "OS" || event.key === "Super") return "Super";
      if (event.key === "Control") return "Ctrl";
      if (event.key === "Alt" || event.key === "AltGraph") return "Alt";
      if (event.key === "Shift") return "Shift";
      return null;
    };
    const syncHeldModifiers = (event: KeyboardEvent) => {
      const heldModifiers = new Set<string>();
      if (event.metaKey) heldModifiers.add("Super");
      if (event.ctrlKey) heldModifiers.add("Ctrl");
      if (event.altKey) heldModifiers.add("Alt");
      if (event.shiftKey) heldModifiers.add("Shift");

      if (event.type === "keyup") {
        const releasedModifier = modifierFor(event);
        if (releasedModifier) heldModifiers.delete(releasedModifier);
      }
      setActiveModifiers(MODIFIER_ORDER.filter((modifier) => heldModifiers.has(modifier)));
    };
    const clearHeldModifiers = () => setActiveModifiers([]);
    window.addEventListener("keydown", syncHeldModifiers);
    window.addEventListener("keyup", syncHeldModifiers);
    window.addEventListener("blur", clearHeldModifiers);
    return () => {
      window.removeEventListener("keydown", syncHeldModifiers);
      window.removeEventListener("keyup", syncHeldModifiers);
      window.removeEventListener("blur", clearHeldModifiers);
    };
  }, []);

  const selectedBindings = useMemo(
    () => sources.flatMap((source) => enabledSources.has(source.id) ? source.bindings : []),
    [sources, enabledSources],
  );
  const visibleBindings = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return selectedBindings;
    return selectedBindings.filter((binding) =>
      `${binding.sourceName} ${binding.action} ${chordLabel(binding)} ${binding.context}`.toLowerCase().includes(needle),
    );
  }, [selectedBindings, query]);
  const bindingsFor = useCallback((code: string) => visibleBindings.filter(
    (binding) => binding.key === code && sameModifiers(binding.modifiers, activeModifiers),
  ), [visibleBindings, activeModifiers]);

  const stats = useMemo(() => {
    let occupied = 0;
    let conflicts = 0;
    KEYBOARD.flat().forEach(({ code }) => {
      const count = bindingsFor(code).length;
      if (count === 1) occupied += 1;
      if (count > 1) conflicts += 1;
    });
    return { free: KEYBOARD.flat().length - occupied - conflicts, occupied, conflicts };
  }, [bindingsFor]);
  const detailBindings = bindingsFor(selectedKey);

  const toggleSource = (sourceId: string) => {
    setEnabledSources((current) => {
      const next = new Set(current);
      next.has(sourceId) ? next.delete(sourceId) : next.add(sourceId);
      return next;
    });
  };
  const toggleModifier = (modifier: string) => {
    setActiveModifiers((current) => {
      const next = current.includes(modifier) ? current.filter((item) => item !== modifier) : [...current, modifier];
      return MODIFIER_ORDER.filter((item) => next.includes(item));
    });
  };

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark" aria-hidden="true"><span>K</span></div>
          <div><h1>Keyventory</h1><p>Your shortcut map, across every tool.</p></div>
        </div>
        <div className="topbar-actions">
          {isDemo && <span className="demo-badge">Browser preview</span>}
          <button className="scan-button" onClick={() => void scan()} disabled={scanning}>
            <span className={scanning ? "spinner" : "scan-icon"}>↻</span>{scanning ? "Scanning…" : "Rescan configs"}
          </button>
        </div>
      </header>

      {scanError && <div className="error-banner">Scan failed: {scanError}</div>}

      <section className="workspace">
        <aside className="source-panel">
          <div className="panel-heading">
            <div><span className="eyebrow">Sources</span><strong>{enabledSources.size} selected</strong></div>
            <button className="text-button" onClick={() => setEnabledSources(new Set(sources.filter((source) => source.status !== "missing").map((source) => source.id)))}>All</button>
          </div>
          <div className="source-list">
            {sources.map((source) => {
              const enabled = enabledSources.has(source.id);
              return (
                <button
                  key={source.id}
                  className={`source-card ${enabled ? "is-enabled" : ""} is-${source.status}`}
                  style={{ "--source-color": sourceColor(source.id) } as CSSProperties}
                  onClick={() => source.status !== "missing" && toggleSource(source.id)}
                  disabled={source.status === "missing"}
                  title={source.configPaths.join("\n")}
                >
                  <span className="source-icon">{source.name.slice(0, 2).toUpperCase()}</span>
                  <span className="source-copy"><strong>{source.name}</strong><small>{source.status === "missing" ? "Not found" : `${source.bindings.length} bindings${source.warnings.length ? ` · ${source.warnings.length} warning` : ""}`}</small></span>
                  <span className="source-toggle" aria-hidden="true"><span /></span>
                </button>
              );
            })}
          </div>
          <div className="coming-soon">
            <span className="eyebrow">Scanner coverage</span>
            <p>Hyprland · Kitty · sxhkd · tmux · Neovim · dwm · dwl</p>
          </div>
        </aside>

        <section className="map-panel">
          <div className="map-toolbar">
            <label className="search-box">
              <span>⌕</span>
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search actions, tools, or keys" />
              {query && <button onClick={() => setQuery("")} aria-label="Clear search">×</button>}
            </label>
            <div className="modifier-bank" aria-label="Active modifiers" title="Hold modifier keys to preview their layer">
              {MODIFIER_ORDER.map((modifier) => (
                <button key={modifier} className={activeModifiers.includes(modifier) ? "is-active" : ""} onClick={() => toggleModifier(modifier)}>{modifier}</button>
              ))}
              <button className={!activeModifiers.length ? "is-active" : ""} onClick={() => setActiveModifiers([])}>None</button>
            </div>
          </div>

          <div className="map-summary">
            <div><span className="eyebrow">Viewing layer</span><h2>{activeModifiers.length ? activeModifiers.join(" + ") : "No modifier"}</h2></div>
            <div className="stats">
              <span><i className="dot free" />{stats.free} free</span>
              <span><i className="dot used" />{stats.occupied} used</span>
              <span><i className="dot conflict" />{stats.conflicts} possible conflicts</span>
            </div>
          </div>

          <div className="keyboard" aria-label="Keyboard binding map">
            {KEYBOARD.map((row, rowIndex) => (
              <div className="key-row" key={rowIndex}>
                {row.map((keyDef) => {
                  const matches = bindingsFor(keyDef.code);
                  const state = matches.length > 1 ? "conflict" : matches.length === 1 ? "used" : "free";
                  const accent = matches.length ? sourceColor(matches[0].sourceId) : "#59636d";
                  return (
                    <button
                      key={keyDef.code}
                      className={`keycap is-${state} ${matches.length ? "has-bindings" : ""} ${selectedKey === keyDef.code ? "is-selected" : ""}`}
                      style={{
                        flex: keyDef.width ?? 1,
                        "--source-color": accent,
                        "--source-fill": sourceFill(matches),
                      } as CSSProperties}
                      onClick={() => setSelectedKey(keyDef.code)}
                      title={matches.length ? matches.map((item) => `${item.sourceName}: ${item.action}`).join("\n") : "Available"}
                    >
                      <span>{keyDef.label}</span>
                      {matches.length > 0 && <b>{matches.length}</b>}
                    </button>
                  );
                })}
              </div>
            ))}
          </div>

          <section className={`key-detail ${detailBindings.length > 1 ? "has-conflict" : ""}`}>
            <div className="detail-title">
              <span className="selected-chord">{[...activeModifiers, selectedKey].join(" + ")}</span>
              <div>
                <strong>{detailBindings.length === 0 ? "Available" : detailBindings.length === 1 ? "Assigned once" : "Possible conflict"}</strong>
                <small>{detailBindings.length === 0 ? "No selected source claims this chord." : `${detailBindings.length} actions claim this chord in the selected layers.`}</small>
              </div>
            </div>
            <div className="detail-bindings">
              {detailBindings.map((binding) => (
                <article key={binding.id}>
                  <span className="binding-source" style={{ "--source-color": sourceColor(binding.sourceId) } as CSSProperties}>{binding.sourceName}</span>
                  <div><strong>{binding.action}</strong><small>{binding.context} · {binding.sourceFile}{binding.line > 0 ? `:${binding.line}` : ""}</small></div>
                  <button disabled title="Round-trip editing is planned after parser validation">Edit soon</button>
                </article>
              ))}
            </div>
          </section>

          {query && (
            <section className="search-results">
              <span className="eyebrow">{visibleBindings.length} matching bindings</span>
              {visibleBindings.slice(0, 6).map((binding) => (
                <button key={binding.id} onClick={() => { setActiveModifiers(binding.modifiers); setSelectedKey(binding.key); }}>
                  <span style={{ "--source-color": sourceColor(binding.sourceId) } as CSSProperties}>{binding.sourceName}</span><strong>{binding.action}</strong><kbd>{chordLabel(binding)}</kbd>
                </button>
              ))}
            </section>
          )}
        </section>
      </section>
    </main>
  );
}

export default App;
