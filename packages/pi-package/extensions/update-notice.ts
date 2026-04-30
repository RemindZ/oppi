import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, normalize, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { Spacer, Text } from "@mariozechner/pi-tui";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const PATCHED_KEY = Symbol.for("oppi.updateNotice.patched");
const SHOWN_KEY = Symbol.for("oppi.updateNotice.shown");
const DEFAULT_CHANGELOG_URL = "https://github.com/RemindZ/oppi/blob/main/CHANGELOG.md";

type InteractiveModeLike = {
  chatContainer?: { addChild: (child: any) => void };
  ui?: { requestRender: (force?: boolean) => void };
};

function resolvePiMainPath(): string {
  const require = createRequire(import.meta.url);
  try {
    return require.resolve("@mariozechner/pi-coding-agent");
  } catch {
    // Package extensions can be loaded from the local repo while Pi itself is global.
  }

  const needle = `${sep}node_modules${sep}@mariozechner${sep}pi-coding-agent${sep}`;
  for (const raw of process.argv) {
    const value = normalize(raw || "");
    const index = value.indexOf(needle);
    if (index >= 0) return join(value.slice(0, index + needle.length), "dist", "index.js");
  }

  const candidates = [
    process.env.APPDATA ? join(process.env.APPDATA, "npm", "node_modules", "@mariozechner", "pi-coding-agent", "dist", "index.js") : undefined,
    process.env.npm_config_prefix ? join(process.env.npm_config_prefix, "node_modules", "@mariozechner", "pi-coding-agent", "dist", "index.js") : undefined,
  ].filter(Boolean) as string[];

  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }

  throw new Error("Cannot resolve @mariozechner/pi-coding-agent internals for OPPi update notice");
}

async function importPiInternal(relativePath: string): Promise<any> {
  const mainPath = resolvePiMainPath();
  return import(pathToFileURL(join(dirname(mainPath), relativePath)).href);
}

function shouldShowNotice(): boolean {
  return Boolean(process.env.OPPI_UPDATE_LATEST_VERSION?.trim());
}

function showOppiUpdateNotice(mode: InteractiveModeLike, DynamicBorder: any, theme: any): void {
  const globalStore = globalThis as Record<symbol, boolean | undefined>;
  if (globalStore[SHOWN_KEY]) return;
  if (!shouldShowNotice()) return;
  if (!mode.chatContainer || !mode.ui) return;

  const latestVersion = process.env.OPPI_UPDATE_LATEST_VERSION!.trim();
  const currentVersion = process.env.OPPI_UPDATE_CURRENT_VERSION?.trim();
  const changelogUrl = process.env.OPPI_CHANGELOG_URL?.trim() || DEFAULT_CHANGELOG_URL;
  const action = theme.fg("accent", "oppi update");
  const installed = currentVersion ? ` (installed ${currentVersion})` : "";
  const updateInstruction = theme.fg("muted", `New OPPi version ${latestVersion} is available${installed}. Run `) + action;
  const changelogLine = theme.fg("muted", "Changelog: ") + theme.fg("accent", changelogUrl);

  globalStore[SHOWN_KEY] = true;
  mode.chatContainer.addChild(new Spacer(1));
  mode.chatContainer.addChild(new DynamicBorder((text: string) => theme.fg("warning", text)));
  mode.chatContainer.addChild(new Text(`${theme.bold(theme.fg("warning", "Update Available"))}\n${updateInstruction}\n${changelogLine}`, 1, 0));
  mode.chatContainer.addChild(new DynamicBorder((text: string) => theme.fg("warning", text)));
  mode.ui.requestRender();
}

async function installUpdateNoticePatch(): Promise<void> {
  const globalStore = globalThis as Record<symbol, boolean | undefined>;
  if (globalStore[PATCHED_KEY]) return;
  globalStore[PATCHED_KEY] = true;

  const [interactive, dynamicBorderModule, themeModule] = await Promise.all([
    importPiInternal("modes/interactive/interactive-mode.js"),
    importPiInternal("modes/interactive/components/dynamic-border.js"),
    importPiInternal("modes/interactive/theme/theme.js"),
  ]);

  const InteractiveMode = interactive.InteractiveMode;
  const DynamicBorder = dynamicBorderModule.DynamicBorder;
  const theme = themeModule.theme;
  const proto = InteractiveMode?.prototype;
  if (!proto || proto.__oppiUpdateNoticePatched) return;

  const originalInit = proto.init;
  proto.init = async function initWithOppiUpdateNotice(this: InteractiveModeLike, ...args: any[]) {
    const result = await originalInit.apply(this, args);
    showOppiUpdateNotice(this, DynamicBorder, theme);
    return result;
  };

  proto.__oppiUpdateNoticePatched = true;
}

export default async function updateNoticeExtension(_pi: ExtensionAPI) {
  await installUpdateNoticePatch();
}
