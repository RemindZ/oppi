import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext } from "@mariozechner/pi-coding-agent";
import { getAgentDir } from "@mariozechner/pi-coding-agent";

export type PromptVariant = "off" | "promptname_a" | "promptname_b";

export type PromptVariantSurface =
  | "main-system-append.md"
  | "review-system-append.md"
  | "permissions-auto-review-system.md"
  | "image-gen-codex-native-adapter-instructions.md";

type PromptVariantInfo = {
  id: PromptVariant;
  label: string;
  description: string;
  appendPath?: string;
};

type OppiSettingsFile = Record<string, any> & {
  oppi?: {
    promptVariant?: PromptVariant;
  };
};

const VARIANTS: PromptVariantInfo[] = [
  {
    id: "off",
    label: "off",
    description: "Use the normal OPPi/Pi system prompt.",
  },
  {
    id: "promptname_a",
    label: "promptname_a",
    description: "Agentic-loop overlay with normal OPPi output style.",
    appendPath: "systemprompts/experiments/promptname_a/main-system-append.md",
  },
  {
    id: "promptname_b",
    label: "promptname_b",
    description: "Caveman-full compressed overlay, while preserving normal OPPi output style.",
    appendPath: "systemprompts/experiments/promptname_b/main-system-append.md",
  },
];

let warnedMissingVariant = false;

function settingsPath(): string {
  return join(getAgentDir(), "settings.json");
}

function readJson(path: string): OppiSettingsFile {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

function parseVariant(value: unknown): PromptVariant | undefined {
  return VARIANTS.some((variant) => variant.id === value) ? (value as PromptVariant) : undefined;
}

function normalizeVariant(value: unknown): PromptVariant {
  return parseVariant(value) ?? "off";
}

function envVariant(): PromptVariant | undefined {
  const raw = process.env.OPPI_SYSTEM_PROMPT_VARIANT;
  if (!raw) return undefined;
  return parseVariant(raw.trim()) ?? "off";
}

function readStoredVariant(): PromptVariant {
  return normalizeVariant(readJson(settingsPath()).oppi?.promptVariant);
}

export function selectedPromptVariant(): PromptVariant {
  return envVariant() ?? readStoredVariant();
}

function writeStoredVariant(variant: PromptVariant): void {
  const path = settingsPath();
  const data = readJson(path);
  data.oppi = data.oppi ?? {};
  data.oppi.promptVariant = variant;
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function variantInfo(variant: PromptVariant): PromptVariantInfo {
  return VARIANTS.find((item) => item.id === variant) ?? VARIANTS[0];
}

function repoRoot(): string {
  const here = dirname(fileURLToPath(import.meta.url));
  return resolve(here, "../../..");
}

function readPromptVariantPath(relativePath: string): { text?: string; path?: string; error?: string } {
  const candidates = [
    resolve(repoRoot(), relativePath),
    resolve(process.cwd(), relativePath),
  ];

  const path = candidates.find((candidate) => existsSync(candidate));
  if (!path) return { error: `Prompt variant file not found: ${relativePath}` };

  try {
    return { path, text: readFileSync(path, "utf8").trim() };
  } catch (error) {
    return { path, error: error instanceof Error ? error.message : String(error) };
  }
}

function readVariantAppend(info: PromptVariantInfo): { text?: string; path?: string; error?: string } {
  if (!info.appendPath) return {};
  return readPromptVariantPath(info.appendPath);
}

export function readPromptVariantSurface(surface: PromptVariantSurface): { variant: PromptVariant; text?: string; path?: string; error?: string } {
  const variant = selectedPromptVariant();
  if (variant === "off") return { variant };
  const result = readPromptVariantPath(`systemprompts/experiments/${variant}/${surface}`);
  return { variant, ...result };
}

function setStatus(ctx: ExtensionContext): void {
  const current = selectedPromptVariant();
  ctx.ui.setStatus("oppi-prompt-variant", current === "off" ? undefined : ctx.ui.theme.fg("accent", `prompt:${current}`));
}

async function chooseVariant(ctx: ExtensionCommandContext): Promise<PromptVariant | undefined> {
  const current = selectedPromptVariant();
  const options = VARIANTS.map((variant) => {
    const active = variant.id === current ? "  [current]" : "";
    return `${variant.label}${active} — ${variant.description}`;
  });
  const selected = await ctx.ui.select("Select OPPi prompt variant", options);
  if (!selected) return undefined;
  const label = selected.split(" — ")[0].replace(/\s+\[current\]$/, "");
  return normalizeVariant(label);
}

function completionItems(prefix: string) {
  return VARIANTS
    .filter((variant) => variant.id.startsWith(prefix.trim().toLowerCase()))
    .map((variant) => ({ value: variant.id, label: `${variant.label} — ${variant.description}` }));
}

export default function promptVariantExtension(pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    setStatus(ctx);
  });

  pi.on("before_agent_start", (event, ctx) => {
    const current = selectedPromptVariant();
    const info = variantInfo(current);
    if (info.id === "off") return;

    const append = readVariantAppend(info);
    if (append.error || !append.text) {
      if (!warnedMissingVariant) {
        warnedMissingVariant = true;
        ctx.ui.notify(`OPPi prompt variant ${info.id} skipped: ${append.error ?? "empty variant file"}`, "warning");
      }
      return;
    }

    setStatus(ctx);
    return {
      systemPrompt: `${event.systemPrompt}\n\n<!-- OPPi prompt variant: ${info.id} (${append.path}) -->\n\n${append.text}`,
    };
  });

  pi.registerCommand("prompt-variant", {
    description: "Select OPPi system prompt A/B variant: /prompt-variant [off|promptname_a|promptname_b].",
    getArgumentCompletions: completionItems,
    handler: async (args, ctx) => {
      const trimmed = args.trim();
      const requested = trimmed ? parseVariant(trimmed) : await chooseVariant(ctx);
      if (!requested) {
        if (trimmed) ctx.ui.notify(`Unknown prompt variant: ${trimmed}`, "warning");
        return;
      }

      if (process.env.OPPI_SYSTEM_PROMPT_VARIANT) {
        ctx.ui.notify("OPPI_SYSTEM_PROMPT_VARIANT is set; env override wins until unset.", "warning");
      }

      writeStoredVariant(requested);
      setStatus(ctx);
      const info = variantInfo(requested);
      ctx.ui.notify(
        requested === "off"
          ? "OPPi prompt variant disabled."
          : `OPPi prompt variant set to ${info.id}: ${info.description}`,
        "info",
      );
    },
  });

}
