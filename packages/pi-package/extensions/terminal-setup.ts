import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import type { ExtensionAPI, ExtensionContext } from "@mariozechner/pi-coding-agent";

const SEND_SEQUENCE_COMMAND = "workbench.action.terminal.sendSequence";
const OFFER_DELAY_MS = 900;

type TerminalBinding = {
  key: string;
  label: string;
  sequenceLiteral: string;
  acceptedSequenceLiterals: string[];
  acceptedSequenceActuals: string[];
};

type EditorProfile = {
  name: string;
  path: string;
};

function terminalBindings(): TerminalBinding[] {
  return [
    {
      key: "shift+enter",
      label: "Shift+Enter",
      sequenceLiteral: "\\u001b[13;2u",
      acceptedSequenceLiterals: ["\\u001b[13;2u"],
      acceptedSequenceActuals: ["\u001b[13;2u"],
    },
    {
      key: "ctrl+enter",
      label: "Ctrl+Enter",
      sequenceLiteral: "\\u001b[13;5u",
      acceptedSequenceLiterals: ["\\u001b[13;5u"],
      acceptedSequenceActuals: ["\u001b[13;5u"],
    },
    {
      key: "alt+up",
      label: "Alt+Up",
      sequenceLiteral: "\\u001b[1;3A",
      acceptedSequenceLiterals: ["\\u001b[1;3A"],
      acceptedSequenceActuals: ["\u001b[1;3A"],
    },
  ];
}

function isInteractiveTerminal(): boolean {
  return Boolean(process.stdin.isTTY && process.stdout.isTTY);
}

function isVsCodeLikeTerminal(): boolean {
  return process.env.TERM_PROGRAM === "vscode" ||
    process.env.VSCODE_INJECTION === "1" ||
    Boolean(process.env.VSCODE_IPC_HOOK_CLI || process.env.VSCODE_GIT_ASKPASS_NODE);
}

function envLooksLikeCursor(): boolean {
  const candidates = [
    process.env.VSCODE_GIT_ASKPASS_NODE,
    process.env.VSCODE_GIT_ASKPASS_MAIN,
    process.env.VSCODE_CWD,
    process.env.TERM_PROGRAM,
    process.env.__CFBundleIdentifier,
  ];
  return candidates.some((value) => value?.toLowerCase().includes("cursor"));
}

function editorProfiles(): EditorProfile[] {
  if (process.platform === "win32") {
    const appData = process.env.APPDATA || join(homedir(), "AppData", "Roaming");
    return [
      { name: "Cursor", path: join(appData, "Cursor", "User", "keybindings.json") },
      { name: "VS Code", path: join(appData, "Code", "User", "keybindings.json") },
      { name: "VS Code Insiders", path: join(appData, "Code - Insiders", "User", "keybindings.json") },
      { name: "VSCodium", path: join(appData, "VSCodium", "User", "keybindings.json") },
    ];
  }

  if (process.platform === "darwin") {
    const support = join(homedir(), "Library", "Application Support");
    return [
      { name: "Cursor", path: join(support, "Cursor", "User", "keybindings.json") },
      { name: "VS Code", path: join(support, "Code", "User", "keybindings.json") },
      { name: "VS Code Insiders", path: join(support, "Code - Insiders", "User", "keybindings.json") },
      { name: "VSCodium", path: join(support, "VSCodium", "User", "keybindings.json") },
    ];
  }

  const config = process.env.XDG_CONFIG_HOME || join(homedir(), ".config");
  return [
    { name: "Cursor", path: join(config, "Cursor", "User", "keybindings.json") },
    { name: "VS Code", path: join(config, "Code", "User", "keybindings.json") },
    { name: "VS Code Insiders", path: join(config, "Code - Insiders", "User", "keybindings.json") },
    { name: "VSCodium", path: join(config, "VSCodium", "User", "keybindings.json") },
  ];
}

function activeEditorProfile(): EditorProfile {
  const profiles = editorProfiles();
  const preferCursor = envLooksLikeCursor();
  const preferred = preferCursor ? profiles.find((profile) => profile.name === "Cursor") : profiles.find((profile) => profile.name === "VS Code");
  if (preferred) return preferred;
  return profiles.find((profile) => existsSync(profile.path)) || profiles[0];
}

function hasBinding(text: string, binding: TerminalBinding): boolean {
  const normalized = text.toLowerCase();
  const hasKey = normalized.includes(`"key": "${binding.key}"`) || normalized.includes(`"key":"${binding.key}"`);
  const hasSequence = binding.acceptedSequenceLiterals.some((sequence) => text.includes(sequence)) ||
    binding.acceptedSequenceActuals.some((sequence) => text.includes(sequence));
  return hasKey && hasSequence && text.includes(SEND_SEQUENCE_COMMAND);
}

function missingBindings(path: string): TerminalBinding[] {
  const text = existsSync(path) ? readFileSync(path, "utf8") : "";
  return terminalBindings().filter((binding) => !hasBinding(text, binding));
}

function bindingText(binding: TerminalBinding): string {
  return [
    "  {",
    `    "key": "${binding.key}",`,
    `    "command": "${SEND_SEQUENCE_COMMAND}",`,
    `    "args": { "text": "${binding.sequenceLiteral}" },`,
    "    \"when\": \"terminalFocus\"",
    "  }",
  ].join("\n");
}

function replaceExistingSendSequence(text: string, binding: TerminalBinding): { text: string; changed: boolean } {
  const key = binding.key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const command = SEND_SEQUENCE_COMMAND.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(
    `("key"\\s*:\\s*"${key}"[\\s\\S]*?"command"\\s*:\\s*"${command}"[\\s\\S]*?"text"\\s*:\\s*")([^"]*)(")`,
    "i",
  );
  const next = text.replace(pattern, `$1${binding.sequenceLiteral}$3`);
  return { text: next, changed: next !== text };
}

function installBindings(profile: EditorProfile): { added: TerminalBinding[]; path: string } {
  let missing = missingBindings(profile.path);
  if (missing.length === 0) return { added: [], path: profile.path };

  mkdirSync(dirname(profile.path), { recursive: true });

  if (!existsSync(profile.path) || readFileSync(profile.path, "utf8").trim() === "") {
    writeFileSync(profile.path, `[\n${missing.map(bindingText).join(",\n")}\n]\n`, "utf8");
    return { added: missing, path: profile.path };
  }

  let text = readFileSync(profile.path, "utf8");
  let updatedExisting = false;
  for (const binding of missing) {
    const result = replaceExistingSendSequence(text, binding);
    text = result.text;
    updatedExisting ||= result.changed;
  }
  if (updatedExisting) {
    writeFileSync(profile.path, text, "utf8");
    missing = missingBindings(profile.path);
    if (missing.length === 0) return { added: terminalBindings(), path: profile.path };
  }

  const closeIndex = text.lastIndexOf("]");
  if (closeIndex === -1) {
    throw new Error(`Could not update ${profile.path}: expected a VS Code keybindings JSON array.`);
  }

  const beforeClose = text.slice(0, closeIndex);
  const trimmedBefore = beforeClose.trimEnd();
  const needsComma = trimmedBefore !== "[" && !trimmedBefore.endsWith(",");
  const insertion = `${needsComma ? "," : ""}\n${missing.map(bindingText).join(",\n")}\n`;
  writeFileSync(profile.path, `${text.slice(0, closeIndex)}${insertion}${text.slice(closeIndex)}`, "utf8");
  return { added: missing, path: profile.path };
}

function setupStatus(profile: EditorProfile): string {
  const missing = missingBindings(profile.path);
  if (missing.length === 0) {
    return `${profile.name} OPPi terminal shortcuts are installed: ${terminalBindings().map((binding) => binding.label).join(", ")}.`;
  }
  return `${profile.name} OPPi terminal setup missing: ${missing.map((binding) => binding.label).join(", ")}.`;
}

async function maybeOfferTerminalSetup(ctx: ExtensionContext): Promise<void> {
  if (process.env.OPPI_TERMINAL_SETUP_OFFER === "0") return;
  if (!isInteractiveTerminal() || !isVsCodeLikeTerminal()) return;

  const profile = activeEditorProfile();
  const missing = missingBindings(profile.path);
  if (missing.length === 0) return;

  const labels = missing.map((binding) => binding.label).join(", ");
  const accepted = await ctx.ui.confirm(
    "Configure OPPi terminal shortcuts?",
    `OPPi can update ${profile.name}'s terminal keybindings so ${labels} reach Pi correctly. This one-click setup covers Shift+Enter newlines, Ctrl+Enter steering, and Alt+Up queued-message editing. Update ${profile.path}?`,
  );

  if (!accepted) {
    ctx.ui.notify("No problem. Run /oppi-terminal-setup whenever you want Shift+Enter/Ctrl+Enter/Alt+Up forwarding.", "info");
    return;
  }

  const result = installBindings(profile);
  ctx.ui.notify(`Installed ${result.added.map((binding) => binding.label).join(", ")} terminal bindings. Restart the terminal or run /reload if needed.`, "info");
}

export default function terminalSetupExtension(pi: ExtensionAPI) {
  pi.registerCommand("oppi-terminal-setup", {
    description: "Install VS Code/Cursor terminal keybindings for Shift+Enter newlines, Ctrl+Enter steering, and Alt+Up queued-message editing.",
    handler: async (args, ctx) => {
      const profile = activeEditorProfile();
      const action = args.trim().toLowerCase();

      try {
        if (action === "status") {
          ctx.ui.notify(setupStatus(profile), missingBindings(profile.path).length === 0 ? "info" : "warning");
          return;
        }

        const result = installBindings(profile);
        if (result.added.length === 0) {
          ctx.ui.notify(`${profile.name} OPPi terminal shortcuts are already installed at ${result.path}.`, "info");
          return;
        }

        ctx.ui.notify(`Installed ${result.added.map((binding) => binding.label).join(", ")} terminal bindings in ${result.path}. Restart the terminal or run /reload if needed.`, "info");
      } catch (error) {
        ctx.ui.notify(error instanceof Error ? error.message : String(error), "error");
      }
    },
  });

  pi.on("session_start", async (_event, ctx) => {
    const timer = setTimeout(() => {
      void maybeOfferTerminalSetup(ctx).catch((error) => {
        ctx.ui.notify(error instanceof Error ? error.message : String(error), "error");
      });
    }, OFFER_DELAY_MS);
    timer.unref?.();
  });
}
