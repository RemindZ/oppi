import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Key, matchesKey, truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";

type ThinkingLevel = "off" | "minimal" | "low" | "medium" | "high" | "xhigh";

const ALL_LEVELS: ThinkingLevel[] = ["off", "minimal", "low", "medium", "high", "xhigh"];
const WIDGET_KEY = "oppi.effort-slider";

const LEVEL_LABELS: Record<ThinkingLevel, string> = {
  off: "Off",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "XHigh",
};

const LEVEL_DESCRIPTIONS: Record<ThinkingLevel, string> = {
  off: "No explicit reasoning effort.",
  minimal: "Fastest reasoning-capable setting.",
  low: "Light reasoning for simple edits and quick answers.",
  medium: "Balanced reasoning for normal coding work.",
  high: "Deep reasoning for complex debugging, architecture, and multi-step work.",
  xhigh: "Maximum effort where the selected model supports it.",
};

function modelName(model: any): string {
  if (!model) return "unknown model";
  return `${model.provider ?? "unknown"}/${model.id ?? model.name ?? "unknown"}`;
}

function supportsXhigh(model: any): boolean {
  const id = String(model?.id ?? "").toLowerCase();

  // Keep this intentionally conservative. Pi will still clamp if a provider/model rejects xhigh.
  return (
    id.includes("gpt-5.2") ||
    id.includes("gpt-5.3") ||
    id.includes("gpt-5.4") ||
    id.includes("gpt-5.5") ||
    id.includes("gpt-5.1-codex-max") ||
    id.includes("opus-4-6") ||
    id.includes("opus-4.6") ||
    id.includes("opus-4-7") ||
    id.includes("opus-4.7")
  );
}

function isAnthropicLike(model: any): boolean {
  const provider = String(model?.provider ?? "").toLowerCase();
  const id = String(model?.id ?? "").toLowerCase();
  return provider.includes("anthropic") || provider === "meridian" || id.includes("claude") || id.includes("opus") || id.includes("sonnet");
}

function levelLabelForModel(model: any, level: ThinkingLevel): string {
  if (level === "xhigh" && isAnthropicLike(model)) return "Max";
  return LEVEL_LABELS[level];
}

function allowedLevelsForModel(model: any): ThinkingLevel[] {
  if (!model?.reasoning) return ["off"];
  return supportsXhigh(model) ? ALL_LEVELS : ALL_LEVELS.filter((level) => level !== "xhigh");
}

function recommendedLevelForModel(model: any): ThinkingLevel {
  if (!model?.reasoning) return "off";

  const provider = String(model.provider ?? "").toLowerCase();
  const id = String(model.id ?? "").toLowerCase();

  if (provider === "openai" || id.includes("gpt-5") || id.includes("o3") || id.includes("o4")) {
    return supportsXhigh(model) ? "high" : "medium";
  }

  if (isAnthropicLike(model)) {
    return supportsXhigh(model) ? "high" : "medium";
  }

  if (provider.includes("google") || id.includes("gemini")) return "medium";

  return "medium";
}

function normalizeLevel(input: string | undefined): ThinkingLevel | undefined {
  const value = input?.trim().toLowerCase();
  if (!value) return undefined;
  if (value === "min") return "minimal";
  if (value === "med") return "medium";
  if (value === "max") return "xhigh";
  if (value === "none") return "off";
  if ((ALL_LEVELS as string[]).includes(value)) return value as ThinkingLevel;
  return undefined;
}

function normalizeCurrentLevel(value: unknown): ThinkingLevel {
  return (ALL_LEVELS as unknown[]).includes(value) ? (value as ThinkingLevel) : "off";
}

function fitLine(line: string, width: number): string {
  return truncateToWidth(line, Math.max(1, width));
}

function padVisible(line: string, width: number, fill = " "): string {
  const pad = Math.max(0, width - visibleWidth(line));
  return line + fill.repeat(pad);
}

function levelToken(level: ThinkingLevel): any {
  return `thinking${level[0].toUpperCase()}${level.slice(1)}` as any;
}

function levelColor(theme: any, level: ThinkingLevel, text: string): string {
  return theme.fg(levelToken(level), text);
}

function topBorder(theme: any, width: number, title: string): string {
  if (width <= 1) return theme.fg("accent", "╭");
  const inner = Math.max(0, width - 2);
  const label = fitLine(`─ ${title} `, inner);
  return theme.fg("accent", `╭${padVisible(label, inner, "─")}╮`);
}

function bottomBorder(theme: any, width: number): string {
  if (width <= 1) return theme.fg("accent", "╰");
  return theme.fg("accent", `╰${"─".repeat(Math.max(0, width - 2))}╯`);
}

function panelLine(theme: any, content: string, width: number): string {
  if (width <= 1) return fitLine(content, width);
  const inner = Math.max(0, width - 2);
  return `${theme.fg("accent", "│")}${padVisible(fitLine(content, inner), inner)}${theme.fg("accent", "│")}`;
}

function levelPositions(count: number, width: number): number[] {
  const size = Math.max(1, width);
  if (count <= 1) return [0];
  return Array.from({ length: count }, (_unused, index) => Math.round((index * (size - 1)) / (count - 1)));
}

function progressLabels(theme: any, model: any, allowed: ThinkingLevel[], selected: number, width: number): string {
  const positions = levelPositions(allowed.length, width);
  let cursor = 0;
  let output = "";

  allowed.forEach((level, index) => {
    const label = levelLabelForModel(model, level);
    const labelWidth = visibleWidth(label);
    const centered = positions[index] - Math.floor(labelWidth / 2);
    const clamped = Math.max(0, Math.min(width - labelWidth, centered));
    const target = Math.max(clamped, cursor + (index === 0 ? 0 : 1));
    if (target >= width) return;

    output += " ".repeat(Math.max(0, target - cursor));
    const colored = levelColor(theme, level, label);
    output += index === selected ? theme.bold(colored) : colored;
    cursor = target + labelWidth;
  });

  return padVisible(output, width);
}

function levelForRailPosition(allowed: ThinkingLevel[], positions: number[], index: number): ThinkingLevel {
  const next = positions.findIndex((position) => index <= position);
  if (next <= 0) return allowed[0] ?? "off";
  return allowed[next] ?? allowed[allowed.length - 1] ?? "off";
}

function progressRail(theme: any, allowed: ThinkingLevel[], selected: number, width: number): string {
  const size = Math.max(1, width);
  const positions = levelPositions(allowed.length, size);
  let output = "";

  for (let index = 0; index < size; index++) {
    const tickIndex = positions.indexOf(index);
    const level = tickIndex !== -1 ? allowed[tickIndex] ?? "off" : levelForRailPosition(allowed, positions, index);
    if (tickIndex !== -1) {
      if (tickIndex === selected) output += theme.bold(levelColor(theme, level, "◆"));
      else output += levelColor(theme, level, tickIndex < selected ? "●" : "○");
    } else {
      output += levelColor(theme, level, index < (positions[selected] ?? 0) ? "━" : "─");
    }
  }

  return output;
}

function renderDockedSlider(theme: any, width: number, model: any, allowed: ThinkingLevel[], current: ThinkingLevel, selected: number): string[] {
  const panelWidth = Math.max(1, width);
  const inner = Math.max(1, panelWidth - 2);
  const level = allowed[selected] ?? "off";
  const recommended = recommendedLevelForModel(model);
  const percent = allowed.length <= 1 ? 0 : selected / (allowed.length - 1);
  const barWidth = Math.max(10, Math.min(52, inner - 18));

  const railPrefix = "Effort ";
  const labelsLine = `${" ".repeat(visibleWidth(railPrefix))}${progressLabels(theme, model, allowed, selected, barWidth)}`;

  const currentLine = [
    theme.fg("muted", "Current effort "),
    levelColor(theme, current, levelLabelForModel(model, current)),
  ].join("");

  const barLine = [
    theme.fg("muted", railPrefix),
    progressRail(theme, allowed, selected, barWidth),
    " ",
    levelColor(theme, level, levelLabelForModel(model, level)),
    theme.fg("dim", `  ${Math.round(percent * 100)}%`),
    level === recommended ? theme.fg("accent", "  ★") : "",
  ].join("");

  const maxLevel = allowed[allowed.length - 1] ?? "off";
  const maxLabel = levelLabelForModel(model, maxLevel);
  const support = model?.reasoning
    ? `Current model's rail caps at ${maxLabel}. Switch models with /model.`
    : "Current model is not marked reasoning-capable, so the rail is locked to Off.";

  const help = `${theme.fg("dim", "◀/▶ or h/l adjust")}  ${theme.fg("accent", "a")} ${theme.fg("dim", "auto")}  ${theme.fg("accent", "enter")} ${theme.fg("dim", "apply")}  ${theme.fg("accent", "esc")} ${theme.fg("dim", "cancel")}`;

  return [
    topBorder(theme, panelWidth, `Effort · ${modelName(model)}`),
    panelLine(theme, currentLine, panelWidth),
    panelLine(theme, labelsLine, panelWidth),
    panelLine(theme, barLine, panelWidth),
    panelLine(theme, levelColor(theme, level, LEVEL_DESCRIPTIONS[level]), panelWidth),
    panelLine(theme, theme.fg("muted", support), panelWidth),
    panelLine(theme, help, panelWidth),
    bottomBorder(theme, panelWidth),
  ];
}

export default function effortExtension(pi: ExtensionAPI) {
  pi.registerCommand("effort", {
    description: "Set thinking effort with a model-aware slider. Usage: /effort [off|minimal|low|medium|high|xhigh|auto]",
    getArgumentCompletions: (prefix: string) => {
      const values = ["auto", ...ALL_LEVELS];
      const filtered = values.filter((value) => value.startsWith(prefix.toLowerCase()));
      return filtered.map((value) => ({ value, label: value }));
    },
    handler: async (args, ctx) => {
      const model = (ctx as any).model;
      const allowed = allowedLevelsForModel(model);
      const current = normalizeCurrentLevel(pi.getThinkingLevel());
      const requested = args.trim().toLowerCase();

      if (requested) {
        const next = requested === "auto" ? recommendedLevelForModel(model) : normalizeLevel(requested);
        if (!next) {
          ctx.ui.notify(`Unknown effort '${args}'. Use off, minimal, low, medium, high, xhigh, or auto.`, "error");
          return;
        }
        if (!allowed.includes(next)) {
          ctx.ui.notify(`${modelName(model)} does not expose '${next}' effort in this slider. Allowed: ${allowed.join(", ")}.`, "warning");
          return;
        }
        pi.setThinkingLevel(next);
        ctx.ui.notify(`Effort set to ${levelLabelForModel(model, next)} for ${modelName(model)}.`, "info");
        return;
      }

      if (!ctx.hasUI) {
        ctx.ui.notify(`Current effort: ${current}. Non-interactive mode: use /effort high, /effort xhigh, etc.`, "info");
        return;
      }

      const initial = allowed.includes(current) ? current : allowed[allowed.length - 1] ?? "off";
      let selected = Math.max(0, allowed.indexOf(initial));

      const result = await new Promise<ThinkingLevel | null>((resolve) => {
        let closed = false;
        let requestRender = () => {};
        let unsubscribe: (() => void) | undefined;

        const setSelected = (level: ThinkingLevel) => {
          const index = allowed.indexOf(level);
          selected = index >= 0 ? index : Math.max(0, allowed.length - 1);
        };

        const close = (value: ThinkingLevel | null) => {
          if (closed) return;
          closed = true;
          unsubscribe?.();
          ctx.ui.setWidget(WIDGET_KEY, undefined);
          resolve(value);
        };

        const redraw = () => requestRender();

        ctx.ui.setWidget(WIDGET_KEY, (tui, theme) => {
          requestRender = () => tui.requestRender();
          return {
            render: (width: number) => renderDockedSlider(theme, width, model, allowed, current, selected),
            invalidate: () => {},
          };
        }, { placement: "aboveEditor" });

        unsubscribe = ctx.ui.onTerminalInput((data: string) => {
          if (closed) return undefined;

          if ((matchesKey(data, Key.left) || matchesKey(data, Key.up) || data === "h" || data === "k") && selected > 0) {
            selected--;
            redraw();
            return { consume: true };
          }

          if ((matchesKey(data, Key.right) || matchesKey(data, Key.down) || data === "l" || data === "j") && selected < allowed.length - 1) {
            selected++;
            redraw();
            return { consume: true };
          }

          if (data === "a") {
            setSelected(recommendedLevelForModel(model));
            redraw();
            return { consume: true };
          }

          if (matchesKey(data, Key.home) || data === "0") {
            selected = 0;
            redraw();
            return { consume: true };
          }

          if (matchesKey(data, Key.end)) {
            selected = Math.max(0, allowed.length - 1);
            redraw();
            return { consume: true };
          }

          const quickPick = Number(data);
          if (Number.isInteger(quickPick) && quickPick >= 1 && quickPick <= allowed.length) {
            selected = quickPick - 1;
            redraw();
            return { consume: true };
          }

          if (matchesKey(data, Key.enter) || matchesKey(data, Key.ctrl("enter"))) {
            close(allowed[selected] ?? "off");
            return { consume: true };
          }

          if (matchesKey(data, Key.escape) || matchesKey(data, Key.ctrl("c"))) {
            close(null);
            return { consume: true };
          }

          return { consume: true };
        });
      });

      if (!result) {
        ctx.ui.notify("Effort unchanged.", "info");
        return;
      }

      pi.setThinkingLevel(result);
      ctx.ui.notify(`Effort set to ${levelLabelForModel(model, result)} for ${modelName(model)}.`, "info");
    },
  });
}
