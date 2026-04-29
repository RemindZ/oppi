import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { ExtensionAPI, ExtensionContext, ExtensionCommandContext, Theme } from "@mariozechner/pi-coding-agent";
import { DynamicBorder, getAgentDir } from "@mariozechner/pi-coding-agent";
import { Container, matchesKey, SettingsList, Text } from "@mariozechner/pi-tui";

const CHECK_INTERVAL_MS = 30_000;
const DEFAULT_IDLE_MINUTES = 5;
const DEFAULT_IDLE_THRESHOLD_PERCENT = 70;
const DEFAULT_SMART_THRESHOLD_PERCENT = 65;
export const VALID_IDLE_MINUTES = [2, 5, 10] as const;
export const VALID_IDLE_THRESHOLDS = [50, 60, 70, 80] as const;
export const VALID_SMART_THRESHOLDS = [50, 55, 60, 65, 70, 75] as const;

export type IdleMinutes = (typeof VALID_IDLE_MINUTES)[number];
export type IdleThresholdPercent = (typeof VALID_IDLE_THRESHOLDS)[number];
export type SmartThresholdPercent = (typeof VALID_SMART_THRESHOLDS)[number];

export type IdleCompactConfig = {
  enabled: boolean;
  idleMinutes: IdleMinutes;
  thresholdPercent: IdleThresholdPercent;
};

export type SmartCompactConfig = {
  thresholdPercent: SmartThresholdPercent;
};

export type OppiCompactConfig = {
  idleCompact: IdleCompactConfig;
  smartCompact: SmartCompactConfig;
};

type OppiSettingsFile = Record<string, any> & {
  oppi?: {
    idleCompact?: Partial<IdleCompactConfig>;
    smartCompact?: Partial<SmartCompactConfig>;
  };
};

function globalSettingsPath(): string {
  return join(getAgentDir(), "settings.json");
}

function projectSettingsPath(cwd: string): string {
  return join(cwd, ".pi", "settings.json");
}

function readJson(path: string): OppiSettingsFile {
  try {
    if (!existsSync(path)) return {};
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return {};
  }
}

export function coerceIdleMinutes(value: unknown): IdleMinutes {
  const numeric = Number(value);
  return VALID_IDLE_MINUTES.includes(numeric as IdleMinutes) ? (numeric as IdleMinutes) : DEFAULT_IDLE_MINUTES;
}

export function coerceIdleThreshold(value: unknown): IdleThresholdPercent {
  const numeric = Number(value);
  return VALID_IDLE_THRESHOLDS.includes(numeric as IdleThresholdPercent)
    ? (numeric as IdleThresholdPercent)
    : DEFAULT_IDLE_THRESHOLD_PERCENT;
}

export function coerceSmartThreshold(value: unknown): SmartThresholdPercent {
  const numeric = Number(value);
  return VALID_SMART_THRESHOLDS.includes(numeric as SmartThresholdPercent)
    ? (numeric as SmartThresholdPercent)
    : DEFAULT_SMART_THRESHOLD_PERCENT;
}

function normalizeIdleConfig(value: Partial<IdleCompactConfig> | undefined): IdleCompactConfig {
  return {
    enabled: value?.enabled !== false,
    idleMinutes: coerceIdleMinutes(value?.idleMinutes),
    thresholdPercent: coerceIdleThreshold(value?.thresholdPercent),
  };
}

function normalizeSmartConfig(value: Partial<SmartCompactConfig> | undefined): SmartCompactConfig {
  return {
    thresholdPercent: coerceSmartThreshold(value?.thresholdPercent),
  };
}

export function readOppiCompactConfig(cwd: string): OppiCompactConfig {
  const global = readJson(globalSettingsPath()).oppi;
  const project = readJson(projectSettingsPath(cwd)).oppi;
  return {
    idleCompact: normalizeIdleConfig({ ...global?.idleCompact, ...project?.idleCompact }),
    smartCompact: normalizeSmartConfig({ ...global?.smartCompact, ...project?.smartCompact }),
  };
}

function readIdleCompactConfig(cwd: string): IdleCompactConfig {
  return readOppiCompactConfig(cwd).idleCompact;
}

export function writeGlobalOppiCompactConfig(config: OppiCompactConfig): void {
  const path = globalSettingsPath();
  const data = readJson(path);
  data.oppi = data.oppi ?? {};
  data.oppi.idleCompact = config.idleCompact;
  data.oppi.smartCompact = config.smartCompact;
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function settingsTheme(theme: Theme) {
  return {
    label: (text: string, selected: boolean) => selected ? theme.fg("accent", theme.bold(text)) : theme.fg("toolOutput", text),
    value: (text: string, selected: boolean) => selected ? theme.fg("success", text) : theme.fg("muted", text),
    description: (text: string) => theme.fg("dim", text),
    cursor: theme.fg("accent", "›"),
    hint: (text: string) => theme.fg("dim", text),
  };
}

async function showIdleCompactSettings(ctx: ExtensionCommandContext): Promise<void> {
  const initial = readOppiCompactConfig(ctx.cwd);

  await ctx.ui.custom<void>((_tui, theme, _kb, done) => {
    const container = new Container();
    let current = initial;
    const list = new SettingsList(
      [
        {
          id: "idle.enabled",
          label: "Idle compaction",
          description: "Compact only after OPPi has been left idle long enough and context is full enough.",
          currentValue: current.idleCompact.enabled ? "true" : "false",
          values: ["true", "false"],
        },
        {
          id: "idle.idleMinutes",
          label: "Idle time",
          description: "How long OPPi waits after the agent becomes idle before compacting.",
          currentValue: String(current.idleCompact.idleMinutes),
          values: VALID_IDLE_MINUTES.map(String),
        },
        {
          id: "idle.thresholdPercent",
          label: "Idle context threshold",
          description: "Only idle-compact when context usage is at or above this percentage.",
          currentValue: String(current.idleCompact.thresholdPercent),
          values: VALID_IDLE_THRESHOLDS.map(String),
        },
        {
          id: "smart.thresholdPercent",
          label: "Smart compact threshold",
          description: "During todo-driven work, compact after todo_write checkpoints at or above this context usage.",
          currentValue: String(current.smartCompact.thresholdPercent),
          values: VALID_SMART_THRESHOLDS.map(String),
        },
      ],
      7,
      settingsTheme(theme),
      (id, value) => {
        current = {
          idleCompact: {
            ...current.idleCompact,
            ...(id === "idle.enabled" ? { enabled: value === "true" } : {}),
            ...(id === "idle.idleMinutes" ? { idleMinutes: coerceIdleMinutes(value) } : {}),
            ...(id === "idle.thresholdPercent" ? { thresholdPercent: coerceIdleThreshold(value) } : {}),
          },
          smartCompact: {
            ...current.smartCompact,
            ...(id === "smart.thresholdPercent" ? { thresholdPercent: coerceSmartThreshold(value) } : {}),
          },
        };
        writeGlobalOppiCompactConfig(current);
        list.updateValue(id, value);
      },
      () => done(),
      { enableSearch: false },
    );

    container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));
    container.addChild(new Text(theme.bold(theme.fg("accent", "OPPi compaction")), 1, 0));
    container.addChild(new Text(theme.fg("dim", "Defaults: idle 5 minutes/70%; smart todo checkpoint 65%. Stored in ~/.pi/agent/settings.json under oppi."), 1, 0));
    container.addChild(list);
    container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));

    return {
      render: (width: number) => container.render(width),
      invalidate: () => container.invalidate(),
      handleInput: (data: string) => {
        if (matchesKey(data, "ctrl+c") || matchesKey(data, "escape")) {
          done();
          return;
        }
        list.handleInput(data);
      },
    };
  });
}

class IdleCompactor {
  private timer: NodeJS.Timeout | undefined;
  private idleSince: number | undefined;
  private running = false;
  private lastTriggerAt = 0;
  private lastTriggeredTokens: number | null | undefined;

  start(ctx: ExtensionContext): void {
    this.stop();
    this.idleSince = ctx.isIdle() ? Date.now() : undefined;
    this.timer = setInterval(() => this.tick(ctx), CHECK_INTERVAL_MS);
    this.timer.unref?.();
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
    this.idleSince = undefined;
    this.running = false;
  }

  markBusy(): void {
    this.idleSince = undefined;
  }

  markIdle(): void {
    this.idleSince = Date.now();
  }

  private tick(ctx: ExtensionContext): void {
    const config = readIdleCompactConfig(ctx.cwd);
    if (!config.enabled) return;

    if (!ctx.isIdle()) {
      this.markBusy();
      return;
    }

    const now = Date.now();
    this.idleSince ??= now;
    if (now - this.idleSince < config.idleMinutes * 60_000) return;

    const usage = ctx.getContextUsage();
    const percent = usage?.percent;
    if (percent === null || percent === undefined || percent < config.thresholdPercent) return;

    const tokens = usage?.tokens ?? null;
    if (this.running) return;
    if (tokens !== null && tokens === this.lastTriggeredTokens && now - this.lastTriggerAt < 30 * 60_000) return;

    this.running = true;
    this.lastTriggerAt = now;
    this.lastTriggeredTokens = tokens;
    this.idleSince = now;
    ctx.ui.notify(`OPPi compacting idle context (${Math.round(percent)}% ≥ ${config.thresholdPercent}%).`, "info");
    ctx.compact({
      customInstructions: "Automatic idle compaction triggered by OPPi.",
      onComplete: () => {
        this.running = false;
        this.idleSince = Date.now();
        this.lastTriggeredTokens = undefined;
      },
      onError: () => {
        this.running = false;
        this.idleSince = Date.now();
      },
    });
  }
}

export default function idleCompactExtension(pi: ExtensionAPI) {
  const compactor = new IdleCompactor();

  pi.on("session_start", async (_event, ctx) => {
    compactor.start(ctx);
  });

  pi.on("agent_start", async () => {
    compactor.markBusy();
  });

  pi.on("agent_end", async () => {
    compactor.markIdle();
  });

  pi.on("session_shutdown", async () => {
    compactor.stop();
  });

  // Settings are consolidated under /settings:oppi in the memory/settings extension.
  // This extension owns only automatic idle compaction lifecycle work.
}
