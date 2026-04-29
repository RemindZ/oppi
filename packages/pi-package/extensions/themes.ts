import type { ExtensionAPI, ExtensionCommandContext, Theme } from "@mariozechner/pi-coding-agent";
import { SettingsManager } from "@mariozechner/pi-coding-agent";
import type { Component } from "@mariozechner/pi-tui";
import { Key, matchesKey, truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";

export type OppiTheme = {
  name: string;
  label: string;
  description: string;
};

type ThemeContext = Pick<ExtensionCommandContext, "cwd" | "ui"> & { hasUI?: boolean };

export const OPPI_THEMES: OppiTheme[] = [
  { name: "oppi-cyan", label: "oppi-cyan", description: "Dark cyan, calm and sharp" },
  { name: "oppi-cyan-light", label: "oppi-cyan-light", description: "Soft off-white cyan with synced terminal colors" },
];

const TERMINAL_THEME_SYNC_INTERVAL_MS = 750;

const TERMINAL_RESET_SEQUENCE = [
  "\u001b]104\u0007", // Reset ANSI palette.
  "\u001b]110\u0007", // Reset default foreground.
  "\u001b]111\u0007", // Reset default background.
  "\u001b]112\u0007", // Reset cursor color.
].join("");

type TerminalPalette = {
  foreground: string;
  background: string;
  cursor: string;
  ansi: readonly string[];
};

const TERMINAL_PALETTES: Record<"oppi-cyan" | "oppi-cyan-light", TerminalPalette> = {
  "oppi-cyan": {
    foreground: "#d9e6ea",
    background: "#0f1419",
    cursor: "#39d7e5",
    ansi: [
      "#111820", "#f7768e", "#9ece6a", "#e0af68",
      "#7aa2f7", "#bb9af7", "#39d7e5", "#c0caf5",
      "#66717d", "#ff8fa3", "#b9f27c", "#f0c987",
      "#9abaff", "#cdb2ff", "#8be9f0", "#ffffff",
    ],
  },
  "oppi-cyan-light": {
    foreground: "#16252f",
    background: "#eef7f4",
    cursor: "#006d7d",
    ansi: [
      "#16252f", "#c62828", "#2e7d32", "#a66a00",
      "#245db2", "#7b3db8", "#008fa3", "#c7d8df",
      "#7c8c96", "#d64242", "#3b8c40", "#b87400",
      "#3267c9", "#8a49c5", "#009fb4", "#ffffff",
    ],
  },
};

let lastTerminalPaletteKey: string | undefined;
let terminalPaletteWasApplied = false;
let terminalThemeSyncTimer: ReturnType<typeof setInterval> | undefined;

export function normalizeThemeName(name: string | undefined): string {
  const trimmed = (name ?? "").trim();
  return trimmed === "oppi-cyan-dark" ? "oppi-cyan" : trimmed;
}

function terminalPaletteKey(name: string | undefined): keyof typeof TERMINAL_PALETTES | undefined {
  const normalized = normalizeThemeName(name);
  if (normalized === "dark" || normalized === "oppi-cyan") return "oppi-cyan";
  if (normalized === "light" || normalized === "oppi-cyan-light") return "oppi-cyan-light";
  return undefined;
}

function terminalThemeSyncDisabled(): boolean {
  return process.env.OPPI_THEME_TERMINAL_SYNC === "0" || process.env.OPPI_TERMINAL_THEME_SYNC === "0";
}

function canWriteTerminalOsc(): boolean {
  return Boolean(process.stdout.isTTY && process.env.TERM !== "dumb");
}

function osc(code: string, value: string): string {
  return `\u001b]${code};${value}\u0007`;
}

function toRgbSpec(hex: string): string {
  const cleaned = hex.replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(cleaned)) return hex;
  return `rgb:${cleaned.slice(0, 2)}/${cleaned.slice(2, 4)}/${cleaned.slice(4, 6)}`;
}

function writeTerminalOsc(sequence: string): void {
  if (!canWriteTerminalOsc()) return;
  try {
    process.stdout.write(sequence);
  } catch {
    // Some terminals or RPC-ish hosts reject raw writes. Theme tokens still apply.
  }
}

function terminalPaletteSequence(palette: TerminalPalette): string {
  return [
    osc("10", palette.foreground),
    osc("11", palette.background),
    osc("12", palette.cursor),
    ...palette.ansi.map((color, index) => osc("4", `${index};${toRgbSpec(color)}`)),
  ].join("");
}

export function resetTerminalThemeColors(): void {
  if (!terminalPaletteWasApplied && lastTerminalPaletteKey === undefined) return;
  writeTerminalOsc(TERMINAL_RESET_SEQUENCE);
  terminalPaletteWasApplied = false;
  lastTerminalPaletteKey = undefined;
}

export function syncTerminalThemeColors(name: string | undefined): void {
  if (terminalThemeSyncDisabled() || !canWriteTerminalOsc()) return;

  const key = terminalPaletteKey(name);
  if (!key) {
    if (terminalPaletteWasApplied) resetTerminalThemeColors();
    lastTerminalPaletteKey = "<reset>";
    return;
  }

  if (lastTerminalPaletteKey === key) return;
  writeTerminalOsc(terminalPaletteSequence(TERMINAL_PALETTES[key]));
  terminalPaletteWasApplied = true;
  lastTerminalPaletteKey = key;
}

export function setOppiTheme(ctx: ThemeContext, name: string): { success: boolean; error?: string } {
  const normalized = normalizeThemeName(name);
  const result = ctx.ui.setTheme(normalized);
  if (result.success) syncTerminalThemeColors(normalized);
  return result;
}

function startTerminalThemeSync(ctx: ThemeContext): void {
  if (terminalThemeSyncTimer) clearInterval(terminalThemeSyncTimer);
  if (ctx.hasUI === false) return;

  const applyCurrent = () => syncTerminalThemeColors(ctx.ui.theme.name ?? currentThemeName(ctx));
  applyCurrent();

  if (terminalThemeSyncDisabled() || !canWriteTerminalOsc()) return;
  terminalThemeSyncTimer = setInterval(applyCurrent, TERMINAL_THEME_SYNC_INTERVAL_MS);
  terminalThemeSyncTimer.unref?.();
}

function stopTerminalThemeSync(): void {
  if (terminalThemeSyncTimer) {
    clearInterval(terminalThemeSyncTimer);
    terminalThemeSyncTimer = undefined;
  }
  resetTerminalThemeColors();
}

export function themeLabel(name: string): string {
  const theme = OPPI_THEMES.find((item) => item.name === normalizeThemeName(name));
  return theme ? `${theme.label}  — ${theme.description}` : name;
}

export function availableOppiThemes(ctx: ThemeContext): OppiTheme[] {
  const available = new Set(ctx.ui.getAllThemes().map((theme) => theme.name));
  return OPPI_THEMES.filter((theme) => available.has(theme.name));
}

export function currentThemeName(ctx: ThemeContext): string {
  const settingsTheme = SettingsManager.create(ctx.cwd).getTheme();
  return normalizeThemeName(ctx.ui.theme.name ?? settingsTheme ?? "oppi-cyan");
}

function applyThemePreview(ctx: ExtensionCommandContext, name: string): { success: boolean; error?: string } {
  const loaded = ctx.ui.getTheme(name);
  if (!loaded) return { success: false, error: `Theme not found: ${name}` };
  // Passing a Theme instance previews without writing settings. Commit uses the name.
  const result = ctx.ui.setTheme(loaded);
  if (result.success) syncTerminalThemeColors(name);
  return result;
}

function restoreThemePreview(ctx: ExtensionCommandContext, name: string): void {
  const loaded = ctx.ui.getTheme(name);
  if (loaded) {
    ctx.ui.setTheme(loaded);
    syncTerminalThemeColors(name);
  }
}

function repeat(char: string, count: number): string {
  return count > 0 ? char.repeat(count) : "";
}

function fit(text: string, width: number): string {
  return truncateToWidth(text, Math.max(0, width), "…");
}

function padAnsi(text: string, width: number): string {
  const clipped = fit(text, width);
  const pad = Math.max(0, width - visibleWidth(clipped));
  return `${clipped}${" ".repeat(pad)}`;
}

function boxedLine(theme: Theme, content: string, innerWidth: number): string {
  return `${theme.fg("border", "│")}${padAnsi(content, innerWidth)}${theme.fg("border", "│")}`;
}

function blankLine(theme: Theme, innerWidth: number): string {
  return boxedLine(theme, "", innerWidth);
}

function topBorder(theme: Theme, title: string, width: number): string {
  if (width <= 2) return theme.fg("borderAccent", repeat("─", width));
  const safeTitle = ` ${title} `;
  const titleText = theme.fg("accent", safeTitle);
  const remaining = Math.max(0, width - 2 - visibleWidth(safeTitle));
  return `${theme.fg("borderAccent", "╭")}${theme.fg("borderAccent", "─")}${titleText}${theme.fg("borderAccent", repeat("─", Math.max(0, remaining - 1)))}${theme.fg("borderAccent", "╮")}`;
}

function bottomBorder(theme: Theme, width: number): string {
  if (width <= 2) return theme.fg("borderAccent", repeat("─", width));
  return theme.fg("borderAccent", `╰${repeat("─", width - 2)}╯`);
}

class ThemePreviewSelector implements Component {
  private selectedIndex: number;

  constructor(
    private readonly themes: OppiTheme[],
    private readonly originalTheme: string,
    private readonly theme: Theme,
    private readonly done: (value: string | undefined) => void,
    private readonly preview: (value: string) => void,
  ) {
    const currentIndex = themes.findIndex((item) => item.name === originalTheme);
    this.selectedIndex = currentIndex >= 0 ? currentIndex : 0;
  }

  handleInput(data: string): void {
    if (matchesKey(data, Key.up) || data === "k") {
      this.move(-1);
      return;
    }
    if (matchesKey(data, Key.down) || data === "j") {
      this.move(1);
      return;
    }
    if (matchesKey(data, Key.enter)) {
      this.done(this.themes[this.selectedIndex]?.name);
      return;
    }
    if (matchesKey(data, Key.escape) || matchesKey(data, Key.ctrl("c"))) {
      this.done(undefined);
      return;
    }
    if (/^[1-9]$/.test(data)) {
      const index = Number(data) - 1;
      if (index >= 0 && index < this.themes.length) {
        this.selectedIndex = index;
        this.previewSelected();
      }
    }
  }

  render(width: number): string[] {
    const panelWidth = Math.max(30, Math.min(width, 78));
    const innerWidth = Math.max(0, panelWidth - 2);
    const t = this.theme;
    const selected = this.themes[this.selectedIndex];
    const lines: string[] = [];

    lines.push(topBorder(t, "OPPi themes", panelWidth));
    lines.push(boxedLine(t, ` ${t.fg("muted", "↑/↓ preview live")} ${t.fg("dim", "·")} ${t.fg("accent", "Enter apply")} ${t.fg("dim", "· Esc cancel")}`, innerWidth));
    lines.push(blankLine(t, innerWidth));

    for (let index = 0; index < this.themes.length; index += 1) {
      const item = this.themes[index];
      const isSelected = index === this.selectedIndex;
      const isCurrent = item.name === this.originalTheme;
      const marker = isSelected ? t.fg("accent", "›") : " ";
      const number = t.fg("dim", `${index + 1}.`);
      const name = isSelected ? t.bold(t.fg("accent", item.label)) : item.label;
      const badges = [
        isSelected ? t.fg("warning", "preview") : undefined,
        isCurrent ? t.fg("success", "current") : undefined,
      ].filter(Boolean).join(t.fg("dim", " · "));
      const suffix = badges ? ` ${t.fg("dim", "[")}${badges}${t.fg("dim", "]")}` : "";
      lines.push(boxedLine(t, ` ${marker} ${number} ${name}${suffix}`, innerWidth));
      lines.push(boxedLine(t, `     ${t.fg("muted", item.description)}`, innerWidth));
    }

    lines.push(blankLine(t, innerWidth));
    lines.push(boxedLine(t, ` ${t.fg("dim", "Live preview")}: ${t.fg("accent", selected?.label ?? "theme")}`, innerWidth));
    lines.push(boxedLine(t, ` ${t.fg("accent", "OPPi")} ${t.fg("muted", "cyan polish")} ${t.fg("success", "success")} ${t.fg("warning", "warning")} ${t.fg("error", "error")}`, innerWidth));
    lines.push(boxedLine(t, ` ${t.bg("userMessageBg", t.fg("userMessageText", " user "))} ${t.fg("text", "Make this feel crisp and safe.")}`, innerWidth));
    lines.push(boxedLine(t, ` ${t.bg("toolSuccessBg", ` ${t.fg("success", "✓")} ${t.fg("toolTitle", "read")} ${t.fg("toolOutput", " README.md ")}`)}`, innerWidth));
    lines.push(boxedLine(t, ` ${t.fg("mdHeading", "## Heading")} ${t.fg("mdLink", "link")} ${t.fg("mdCode", "`code`")} ${t.fg("syntaxKeyword", "const")} ${t.fg("syntaxString", "\"cyan\"")}`, innerWidth));
    lines.push(blankLine(t, innerWidth));
    lines.push(boxedLine(t, ` ${t.fg("dim", "j/k also move · number keys preview · Esc restores")}`, innerWidth));
    lines.push(bottomBorder(t, panelWidth));

    return lines.map((line) => fit(line, width));
  }

  invalidate(): void {
    // Rendering is computed from the live theme proxy each time.
  }

  private move(delta: number): void {
    if (this.themes.length === 0) return;
    this.selectedIndex = (this.selectedIndex + delta + this.themes.length) % this.themes.length;
    this.previewSelected();
  }

  private previewSelected(): void {
    const selected = this.themes[this.selectedIndex];
    if (selected) this.preview(selected.name);
  }
}

export async function openThemePreview(ctx: ExtensionCommandContext): Promise<string | undefined> {
  const themes = availableOppiThemes(ctx);
  if (themes.length === 0) {
    ctx.ui.notify("No OPPi themes are available.", "warning");
    return undefined;
  }

  const original = currentThemeName(ctx);
  const selected = await ctx.ui.custom<string | undefined>(
    (_tui, theme, _keybindings, done) => new ThemePreviewSelector(
      themes,
      original,
      theme,
      done,
      (name) => {
        const result = applyThemePreview(ctx, name);
        if (!result.success) ctx.ui.notify(result.error ?? `Could not preview theme ${name}.`, "error");
      },
    ),
    undefined,
  );

  if (!selected) {
    restoreThemePreview(ctx, original);
    return undefined;
  }

  return normalizeThemeName(selected);
}

export default function themesExtension(pi: ExtensionAPI) {
  // Theme switching now lives in OPPi settings. Keep this extension focused on
  // package theme metadata, live-preview helpers, and terminal color syncing.
  pi.on("session_start", async (_event, ctx) => {
    startTerminalThemeSync(ctx);
  });

  pi.on("session_shutdown", async () => {
    stopTerminalThemeSync();
  });
}
