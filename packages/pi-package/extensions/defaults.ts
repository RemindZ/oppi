import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { getAgentDir, SettingsManager } from "@mariozechner/pi-coding-agent";

type RawOppiDefaults = {
  steeringMode?: "all" | "one-at-a-time";
  followUpMode?: "all" | "one-at-a-time";
  collapseChangelog?: boolean;
  hideThinkingBlock?: boolean;
  theme?: string;
};

async function applyOppiSettingsDefaults(cwd: string): Promise<void> {
  const settings = SettingsManager.create(cwd);
  const global = settings.getGlobalSettings() as RawOppiDefaults;
  const project = settings.getProjectSettings() as RawOppiDefaults;

  // Only fill in unset values. User/project choices still win.
  if (global.steeringMode === undefined && project.steeringMode === undefined) {
    settings.setSteeringMode("all");
  }
  if (global.followUpMode === undefined && project.followUpMode === undefined) {
    settings.setFollowUpMode("all");
  }
  if (global.collapseChangelog === undefined && project.collapseChangelog === undefined) {
    settings.setCollapseChangelog(true);
  }
  if (global.hideThinkingBlock === undefined && project.hideThinkingBlock === undefined) {
    settings.setHideThinkingBlock(true);
  }
  if (global.theme === undefined && project.theme === undefined) {
    settings.setTheme("oppi-cyan");
  } else if (global.theme === "oppi-cyan-dark" && project.theme === undefined) {
    // Merge the short-lived dark alias back into the canonical dark theme.
    settings.setTheme("oppi-cyan");
  }

  await settings.flush();
}

function stringArray(value: unknown): string[] | undefined {
  if (typeof value === "string") return [value];
  if (!Array.isArray(value)) return undefined;
  return value.filter((item): item is string => typeof item === "string");
}

function sameArray(a: string[] | undefined, b: string[]): boolean {
  return Boolean(a && a.length === b.length && a.every((value, index) => value === b[index]));
}

function removeKey(keys: string[] | undefined, key: string): string[] | undefined {
  if (!keys || !keys.includes(key)) return keys;
  return keys.filter((value) => value !== key);
}

function applyOppiKeybindingDefaults(): void {
  const path = join(getAgentDir(), "keybindings.json");
  let keybindings: Record<string, unknown> = {};

  if (existsSync(path)) {
    try {
      keybindings = JSON.parse(readFileSync(path, "utf8"));
    } catch {
      return;
    }
  }

  let changed = false;

  // Follow-up remains Alt+Enter at the static keybinding layer. OPPi's enter-routing
  // extension rewrites busy plain Enter to Alt+Enter at runtime; keeping Enter out of
  // this static binding preserves Pi's editor submitValue() path while idle, including
  // clearing the textbox after a normal send.
  const followUp = stringArray(keybindings["app.message.followUp"]);
  if (followUp) {
    const next = followUp.filter((key) => key !== "enter");
    if (!next.includes("alt+enter")) next.push("alt+enter");
    if (!sameArray(followUp, next)) {
      keybindings["app.message.followUp"] = next;
      changed = true;
    }
  } else if (keybindings["app.message.followUp"] === undefined) {
    keybindings["app.message.followUp"] = ["alt+enter"];
    changed = true;
  }

  const dequeue = stringArray(keybindings["app.message.dequeue"]);
  if (dequeue) {
    const next = [...dequeue];
    if (!next.includes("alt+up")) next.push("alt+up");
    if (!sameArray(dequeue, next)) {
      keybindings["app.message.dequeue"] = next;
      changed = true;
    }
  } else if (keybindings["app.message.dequeue"] === undefined) {
    keybindings["app.message.dequeue"] = ["alt+up"];
    changed = true;
  }

  // Alt+Up must be reserved for queued-message restore in the main editor. Some old
  // hand-written configs mapped it to editor-up/history, which made Alt+Up pull stale
  // editor history (often prior todo/status text) instead of only the live queue.
  const cursorUp = stringArray(keybindings["tui.editor.cursorUp"]);
  const cleanedCursorUp = removeKey(cursorUp, "alt+up");
  if (cursorUp && cleanedCursorUp && !sameArray(cursorUp, cleanedCursorUp)) {
    const next = cleanedCursorUp.length > 0 ? cleanedCursorUp : ["up"];
    keybindings["tui.editor.cursorUp"] = next.length === 1 ? next[0] : next;
    changed = true;
  }

  // Keep plain Enter as a submit key for extension text editors/dialogs and for normal
  // idle sends. Ctrl+Enter uses the same submit path, which steers while streaming.
  const desiredSubmit = ["enter", "ctrl+enter"];
  const submit = stringArray(keybindings["tui.input.submit"]);
  if (keybindings["tui.input.submit"] === undefined) {
    keybindings["tui.input.submit"] = desiredSubmit;
    changed = true;
  } else if (submit && !sameArray(submit, desiredSubmit)) {
    const next = [...submit];
    for (const key of desiredSubmit) {
      if (!next.includes(key)) next.push(key);
    }
    keybindings["tui.input.submit"] = next;
    changed = true;
  }

  // Pi's newline remains the Claude Code-style Shift+Enter only. Clean up the
  // short-lived Ctrl+Enter/Super+Enter/Ctrl+J aliases from earlier OPPi builds.
  const newline = stringArray(keybindings["tui.input.newLine"]);
  if (newline) {
    const cleaned = newline.filter((key) => !["ctrl+enter", "super+enter", "ctrl+j"].includes(key));
    if (!cleaned.includes("shift+enter")) cleaned.unshift("shift+enter");
    if (!sameArray(newline, cleaned)) {
      keybindings["tui.input.newLine"] = cleaned;
      changed = true;
    }
  } else if (keybindings["tui.input.newLine"] === undefined && keybindings.newLine === undefined) {
    keybindings["tui.input.newLine"] = ["shift+enter"];
    changed = true;
  }

  if (!changed) return;
  mkdirSync(getAgentDir(), { recursive: true });
  writeFileSync(path, `${JSON.stringify(keybindings, null, 2)}\n`, "utf8");
}

async function applyOppiDefaults(cwd: string): Promise<void> {
  await applyOppiSettingsDefaults(cwd);
  applyOppiKeybindingDefaults();
}

export default async function defaultsExtension(pi: ExtensionAPI) {
  await applyOppiDefaults(process.cwd());

  pi.on("session_start", async (_event, ctx) => {
    await applyOppiDefaults(ctx.cwd);
  });
}
