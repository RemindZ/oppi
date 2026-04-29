import { Buffer } from "node:buffer";
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, Theme } from "@mariozechner/pi-coding-agent";
import { DynamicBorder } from "@mariozechner/pi-coding-agent";
import { Container, matchesKey, Text, truncateToWidth, visibleWidth } from "@mariozechner/pi-tui";

const OPENAI_CODEX_BASE_URL = "https://chatgpt.com/backend-api";
const USAGE_REFRESH_MS = 60_000;
const FIVE_HOURS_MS = 5 * 60 * 60 * 1_000;
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1_000;

type ModelLike = {
  id: string;
  name?: string;
  provider: string;
  baseUrl?: string;
  reasoning?: boolean;
  contextWindow?: number;
};

type UsageWindow = {
  label: string;
  usedPercent: number | null;
  windowMinutes?: number;
  resetsAt?: number;
  usedTokens?: number | null;
  limitTokens?: number;
  source: "live" | "local" | "headers" | "unknown";
  note?: string;
};

type UsageSnapshot = {
  key: string;
  provider: string;
  modelId: string;
  modelName?: string;
  auth: "subscription" | "api-key" | "unknown";
  planType?: string;
  source: "openai-codex" | "local" | "headers" | "unknown";
  fetchedAt: number;
  fiveHour: UsageWindow;
  weekly: UsageWindow;
  additionalWindows?: UsageWindow[];
  notes: string[];
};

type UsageReport = {
  snapshots: UsageSnapshot[];
  active: UsageSnapshot;
  context: UsageWindow;
  generatedAt: number;
};

type OpenAIRateLimitWindow = {
  usedPercent: number;
  windowMinutes?: number;
  resetsAt?: number;
};

type OpenAIRateLimitSnapshot = {
  limitId?: string;
  limitName?: string;
  planType?: string;
  primary?: OpenAIRateLimitWindow;
  secondary?: OpenAIRateLimitWindow;
};

type HeaderLimit = {
  usedPercent: number;
  resetsAt?: number;
  label: string;
};

let requestRender: (() => void) | undefined;
let permissionMode = "auto-review";
const remoteUsageCache = new Map<string, UsageSnapshot>();
const headerLimitCache = new Map<string, HeaderLimit>();
const refreshTimes = new Map<string, number>();

function modelKeyFor(model: ModelLike | undefined): string {
  return `${model?.provider ?? "unknown"}/${model?.id ?? "unknown"}`;
}

function modelKey(ctx: ExtensionContext): string {
  return modelKeyFor(ctx.model as ModelLike | undefined);
}

function providerFamily(provider: string | undefined): "openai-codex" | "openai" | "anthropic" | "other" {
  if (!provider) return "other";
  if (provider === "openai-codex") return "openai-codex";
  if (provider.includes("openai")) return "openai";
  if (provider === "anthropic" || provider.includes("anthropic")) return "anthropic";
  return "other";
}

function usageGroupKey(model: ModelLike | undefined): string {
  if (!model) return "unknown";
  const family = providerFamily(model.provider);
  if (family === "openai-codex") return "openai-codex";
  if (family === "anthropic") return "anthropic";
  return model.provider;
}

function isSameModel(a: ModelLike | undefined, b: ModelLike | undefined): boolean {
  return Boolean(a && b && a.provider === b.provider && a.id === b.id);
}

function clampPercent(value: number | null | undefined): number | null {
  if (value === null || value === undefined || Number.isNaN(value)) return null;
  return Math.max(0, Math.min(100, value));
}

function formatTokens(tokens: number | null | undefined): string {
  if (tokens === null || tokens === undefined) return "";
  if (tokens < 1_000) return String(tokens);
  if (tokens < 10_000) return `${(tokens / 1_000).toFixed(1)}k`;
  if (tokens < 1_000_000) return `${Math.round(tokens / 1_000)}k`;
  return `${(tokens / 1_000_000).toFixed(tokens < 10_000_000 ? 1 : 0)}M`;
}

function formatWindowLabel(windowMinutes: number | undefined, fallback: string): string {
  if (!windowMinutes) return fallback;
  if (Math.abs(windowMinutes - 300) <= 5) return "5h";
  if (Math.abs(windowMinutes - 10_080) <= 30) return "7d";
  if (windowMinutes < 60) return `${Math.round(windowMinutes)}m`;
  if (windowMinutes < 24 * 60) return `${Math.round(windowMinutes / 60)}h`;
  return `${Math.round(windowMinutes / 1440)}d`;
}

function displayWindowLabel(label: string): string {
  const normalized = label.toLowerCase();
  if (normalized.startsWith("7d") || normalized.includes("week")) return "week";
  if (normalized.startsWith("5h")) return "5h";
  return label;
}

function formatDuration(ms: number, preferHours = false): string {
  if (ms <= 0) return "now";
  const minutes = ms / 60_000;
  if (minutes < 60) return `${Math.max(1, Math.ceil(minutes))}m`;
  const hours = ms / 3_600_000;
  if (preferHours || hours < 36) return `${hours < 10 ? hours.toFixed(1) : Math.round(hours)}h`;
  const days = ms / 86_400_000;
  return `${days < 10 ? days.toFixed(1) : Math.round(days)}d`;
}

function formatTimeLeft(resetsAt: number | undefined, preferHours = false): string {
  if (!resetsAt) return "";
  return formatDuration(resetsAt * 1_000 - Date.now(), preferHours);
}

function formatReset(resetsAt: number | undefined, compact = false): string {
  if (!resetsAt) return "";
  const ms = resetsAt * 1_000 - Date.now();
  if (ms <= 0) return "now";

  const date = new Date(resetsAt * 1_000);
  const sameDay = date.toDateString() === new Date().toDateString();
  const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (compact || sameDay) return time;
  const day = date.toLocaleDateString([], { weekday: "short" });
  return `${day} ${time}`;
}

function bar(percent: number | null, width: number, theme: Theme): string {
  const empty = "░".repeat(width);
  if (percent === null) return theme.fg("dim", empty);
  const filled = Math.max(0, Math.min(width, Math.round((percent / 100) * width)));
  const color = percent >= 90 ? "error" : percent >= 70 ? "warning" : "success";
  return theme.fg(color, "█".repeat(filled)) + theme.fg("dim", "░".repeat(width - filled));
}

function footerLimitWindow(window: UsageWindow, theme: Theme, width = 7): string {
  const label = displayWindowLabel(window.label);
  const pct = window.usedPercent === null ? "?" : `${Math.round(window.usedPercent)}%`;
  const left = formatTimeLeft(window.resetsAt, label === "5h");
  const leftPart = left ? `${theme.fg("dim", `${left} left`)} ` : "";
  return `${theme.fg("muted", label)} ${leftPart}${bar(window.usedPercent, width, theme)} ${theme.fg("dim", pct)}`;
}

function footerContextWindow(window: UsageWindow, theme: Theme, width = 7): string {
  const pct = window.usedPercent === null ? "?" : `${Math.round(window.usedPercent)}%`;
  const tokens = window.limitTokens
    ? `${formatTokens(window.usedTokens) || "?"}/${formatTokens(window.limitTokens)}`
    : formatTokens(window.usedTokens) || "?";
  return `${theme.fg("muted", "ctx")} ${theme.fg("dim", tokens)} ${bar(window.usedPercent, width, theme)} ${theme.fg("dim", pct)}`;
}

function contextWindow(ctx: ExtensionContext): UsageWindow {
  const usage = ctx.getContextUsage();
  const fallbackWindow = (ctx.model as ModelLike | undefined)?.contextWindow;
  const limitTokens = usage?.contextWindow ?? fallbackWindow;
  const tokens = usage?.tokens ?? null;
  const percent = usage?.percent ?? (tokens !== null && limitTokens ? (tokens / limitTokens) * 100 : null);
  return {
    label: "ctx",
    usedPercent: clampPercent(percent),
    usedTokens: tokens,
    limitTokens,
    source: "local",
  };
}

function authKindFor(ctx: ExtensionContext, model: ModelLike | undefined): "subscription" | "api-key" | "unknown" {
  if (!model) return "unknown";
  try {
    if (ctx.modelRegistry.isUsingOAuth(model as any)) return "subscription";
    if (ctx.modelRegistry.hasConfiguredAuth(model as any)) return "api-key";
  } catch {
    return "unknown";
  }
  return "unknown";
}

function totalUsageTokens(usage: any): number {
  if (!usage) return 0;
  return Number(usage.totalTokens ?? usage.total ?? 0) ||
    (Number(usage.input ?? 0) || 0) +
      (Number(usage.output ?? 0) || 0) +
      (Number(usage.cacheRead ?? 0) || 0) +
      (Number(usage.cacheWrite ?? 0) || 0);
}

function collectLocalTokens(ctx: ExtensionContext, model: ModelLike | undefined, windowMs: number): { usedTokens: number; resetAt?: number } {
  const now = Date.now();
  const cutoff = now - windowMs;
  let usedTokens = 0;
  let oldest: number | undefined;

  for (const entry of ctx.sessionManager.getEntries()) {
    if (entry.type !== "message") continue;
    const message: any = entry.message;
    if (message.role !== "assistant") continue;
    if (model?.provider && message.provider && message.provider !== model.provider) continue;
    if (model?.id && message.model && message.model !== model.id) continue;

    const timestamp = typeof message.timestamp === "number" ? message.timestamp : Date.parse(entry.timestamp);
    if (!timestamp || timestamp < cutoff) continue;
    const tokens = totalUsageTokens(message.usage);
    if (!tokens) continue;
    usedTokens += tokens;
    oldest = oldest === undefined ? timestamp : Math.min(oldest, timestamp);
  }

  return { usedTokens, resetAt: oldest ? Math.floor((oldest + windowMs) / 1_000) : undefined };
}

function envLimit(provider: string | undefined, modelId: string | undefined, kind: "5h" | "weekly"): number | undefined {
  const normalizedProvider = (provider ?? "unknown").toUpperCase().replace(/[^A-Z0-9]+/g, "_");
  const normalizedModel = (modelId ?? "unknown").toUpperCase().replace(/[^A-Z0-9]+/g, "_");
  const suffix = kind === "5h" ? "5H_TOKENS" : "WEEKLY_TOKENS";
  const candidates = [
    `OPPI_USAGE_${normalizedProvider}_${normalizedModel}_${suffix}`,
    `OPPI_USAGE_${normalizedProvider}_${suffix}`,
    `OPPI_USAGE_${providerFamily(provider).toUpperCase().replace(/[^A-Z0-9]+/g, "_")}_${suffix}`,
  ];
  for (const name of candidates) {
    const raw = process.env[name];
    if (!raw) continue;
    const value = Number(raw.replace(/_/g, ""));
    if (Number.isFinite(value) && value > 0) return value;
  }
  return undefined;
}

function localWindow(ctx: ExtensionContext, model: ModelLike | undefined, label: string, windowMs: number, limitKind: "5h" | "weekly"): UsageWindow {
  const { usedTokens, resetAt } = collectLocalTokens(ctx, model, windowMs);
  const limitTokens = envLimit(model?.provider, model?.id, limitKind);
  return {
    label,
    usedPercent: limitTokens ? clampPercent((usedTokens / limitTokens) * 100) : null,
    resetsAt: resetAt,
    usedTokens,
    limitTokens,
    source: "local",
  };
}

function localSnapshot(ctx: ExtensionContext, model: ModelLike | undefined = ctx.model as ModelLike | undefined, notes: string[] = []): UsageSnapshot {
  return {
    key: modelKeyFor(model),
    provider: model?.provider ?? "unknown",
    modelId: model?.id ?? "unknown",
    modelName: model?.name,
    auth: authKindFor(ctx, model),
    source: "local",
    fetchedAt: Date.now(),
    fiveHour: localWindow(ctx, model, "5h", FIVE_HOURS_MS, "5h"),
    weekly: localWindow(ctx, model, "7d", SEVEN_DAYS_MS, "weekly"),
    notes,
  };
}

function decodeJwtPayload(token: string): any | undefined {
  try {
    const payload = token.split(".")[1];
    if (!payload) return undefined;
    const normalized = payload.replace(/-/g, "+").replace(/_/g, "/");
    const padded = normalized + "=".repeat((4 - (normalized.length % 4)) % 4);
    return JSON.parse(Buffer.from(padded, "base64").toString("utf8"));
  } catch {
    return undefined;
  }
}

function openAIAccountId(ctx: ExtensionContext, model: ModelLike, token: string): string | undefined {
  const credential = ctx.modelRegistry.authStorage.get(model.provider);
  const fromCredential = (credential as any)?.accountId;
  if (typeof fromCredential === "string" && fromCredential) return fromCredential;
  const payload = decodeJwtPayload(token);
  const accountId = payload?.["https://api.openai.com/auth"]?.chatgpt_account_id;
  return typeof accountId === "string" && accountId ? accountId : undefined;
}

function normalizeBaseUrl(value: string | undefined): string {
  return (value && value.trim() ? value : OPENAI_CODEX_BASE_URL).replace(/\/+$/, "");
}

function parseOpenAIWindow(raw: any): OpenAIRateLimitWindow | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const usedPercent = Number(raw.used_percent);
  const seconds = Number(raw.limit_window_seconds);
  const resetAt = Number(raw.reset_at);
  return {
    usedPercent: Number.isFinite(usedPercent) ? usedPercent : 0,
    windowMinutes: Number.isFinite(seconds) && seconds > 0 ? seconds / 60 : undefined,
    resetsAt: Number.isFinite(resetAt) && resetAt > 0 ? resetAt : undefined,
  };
}

function parseOpenAISnapshot(limitId: string | undefined, limitName: string | undefined, rateLimit: any, planType: string | undefined): OpenAIRateLimitSnapshot {
  return {
    limitId,
    limitName,
    planType,
    primary: parseOpenAIWindow(rateLimit?.primary_window),
    secondary: parseOpenAIWindow(rateLimit?.secondary_window),
  };
}

function openAISnapshotsFromPayload(payload: any): OpenAIRateLimitSnapshot[] {
  const planType = typeof payload?.plan_type === "string" ? payload.plan_type : undefined;
  const snapshots: OpenAIRateLimitSnapshot[] = [parseOpenAISnapshot("codex", undefined, payload?.rate_limit, planType)];

  const additional = Array.isArray(payload?.additional_rate_limits) ? payload.additional_rate_limits : [];
  for (const item of additional) {
    snapshots.push(
      parseOpenAISnapshot(
        typeof item?.metered_feature === "string" ? item.metered_feature : undefined,
        typeof item?.limit_name === "string" ? item.limit_name : undefined,
        item?.rate_limit,
        planType,
      ),
    );
  }

  return snapshots.filter((snapshot) => snapshot.primary || snapshot.secondary);
}

function normalizeSearch(value: string | undefined): string {
  return (value ?? "").toLowerCase().replace(/[^a-z0-9]+/g, "");
}

function chooseOpenAISnapshot(snapshots: OpenAIRateLimitSnapshot[], modelId: string): OpenAIRateLimitSnapshot | undefined {
  const modelNeedle = normalizeSearch(modelId);
  const modelMatch = snapshots.find((snapshot) => {
    const haystack = normalizeSearch(`${snapshot.limitId ?? ""} ${snapshot.limitName ?? ""}`);
    return haystack && (haystack.includes(modelNeedle) || modelNeedle.includes(haystack));
  });
  return modelMatch ?? snapshots.find((snapshot) => snapshot.limitId === "codex") ?? snapshots[0];
}

function windowsFromOpenAISnapshot(snapshot: OpenAIRateLimitSnapshot): Pick<UsageSnapshot, "fiveHour" | "weekly"> {
  const windows = [snapshot.primary, snapshot.secondary].filter((window): window is OpenAIRateLimitWindow => Boolean(window));
  const short =
    windows.find((window) => Math.abs((window.windowMinutes ?? 0) - 300) <= 5) ??
    windows.slice().sort((a, b) => (a.windowMinutes ?? Infinity) - (b.windowMinutes ?? Infinity))[0];
  const weekly =
    windows.find((window) => Math.abs((window.windowMinutes ?? 0) - 10_080) <= 30) ??
    windows.slice().sort((a, b) => (b.windowMinutes ?? 0) - (a.windowMinutes ?? 0))[0];

  return {
    fiveHour: {
      label: formatWindowLabel(short?.windowMinutes, "5h"),
      usedPercent: clampPercent(short?.usedPercent),
      windowMinutes: short?.windowMinutes,
      resetsAt: short?.resetsAt,
      source: "live",
    },
    weekly: {
      label: formatWindowLabel(weekly?.windowMinutes, "7d"),
      usedPercent: clampPercent(weekly?.usedPercent),
      windowMinutes: weekly?.windowMinutes,
      resetsAt: weekly?.resetsAt,
      source: "live",
    },
  };
}

async function fetchOpenAICodexUsage(ctx: ExtensionContext, model: ModelLike): Promise<UsageSnapshot> {
  const auth = await ctx.modelRegistry.getApiKeyAndHeaders(model as any);
  if (!auth.ok) throw new Error(auth.error);
  if (!auth.apiKey) throw new Error("No OpenAI Codex OAuth token available");

  const accountId = openAIAccountId(ctx, model, auth.apiKey);
  if (!accountId) throw new Error("Could not resolve ChatGPT account id from OAuth token");

  const base = normalizeBaseUrl(model.baseUrl);
  const headers: Record<string, string> = {
    authorization: `Bearer ${auth.apiKey}`,
    "chatgpt-account-id": accountId,
    originator: "pi",
    accept: "application/json",
    "user-agent": "oppi-pi-package-usage/0.0.0",
  };

  const urls = [`${base}/wham/usage`, `${base}/api/codex/usage`];
  let lastError: string | undefined;
  for (const url of urls) {
    const response = await fetch(url, { headers, signal: ctx.signal }).catch((error) => {
      lastError = error instanceof Error ? error.message : String(error);
      return undefined;
    });
    if (!response) continue;
    const text = await response.text();
    if (!response.ok) {
      lastError = `${response.status} ${response.statusText}: ${text.slice(0, 300)}`;
      continue;
    }

    const payload = JSON.parse(text);
    const snapshots = openAISnapshotsFromPayload(payload);
    const selected = chooseOpenAISnapshot(snapshots, model.id);
    if (!selected) throw new Error("ChatGPT usage response did not include rate limits");
    const windows = windowsFromOpenAISnapshot(selected);
    return {
      key: modelKeyFor(model),
      provider: model.provider,
      modelId: model.id,
      modelName: model.name,
      auth: "subscription",
      planType: selected.planType,
      source: "openai-codex",
      fetchedAt: Date.now(),
      ...windows,
      notes: [
        `Live ChatGPT Codex usage from ${url.replace(base, "")}`,
        selected.limitName ? `Matched limit: ${selected.limitName}` : `Matched limit: ${selected.limitId ?? "codex"}`,
      ],
    };
  }
  throw new Error(lastError ?? "Could not fetch ChatGPT usage");
}

function compactProviderError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const status = raw.match(/\b(\d{3})\s+([^:]{2,80})(?::|$)/);
  const hasHtmlChallenge = /<!doctype html|<html|just a moment|cloudflare/i.test(raw);
  if (status && hasHtmlChallenge) return `${status[1]} ${status[2].trim()} (provider returned an HTML challenge)`;
  if (status) return `${status[1]} ${status[2].trim()}`;
  return raw
    .replace(/<[^>]*>/g, " ")
    .replace(/[\r\n\t]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 220);
}

function parseResetHeader(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const asDate = Date.parse(value);
  if (!Number.isNaN(asDate)) return Math.floor(asDate / 1_000);

  const match = value.match(/(?:(\d+)h)?(?:(\d+)m)?(?:(\d+(?:\.\d+)?)s)?/i);
  if (!match) return undefined;
  const hours = Number(match[1] ?? 0);
  const mins = Number(match[2] ?? 0);
  const secs = Number(match[3] ?? 0);
  const total = hours * 3600 + mins * 60 + secs;
  return total > 0 ? Math.floor(Date.now() / 1_000 + total) : undefined;
}

function parseHeaderLimit(provider: string, headers: Record<string, string>): HeaderLimit | undefined {
  const lower = Object.fromEntries(Object.entries(headers).map(([key, value]) => [key.toLowerCase(), value]));
  if (providerFamily(provider) === "anthropic") {
    const limit = Number(lower["anthropic-ratelimit-tokens-limit"] ?? lower["anthropic-ratelimit-requests-limit"]);
    const remaining = Number(lower["anthropic-ratelimit-tokens-remaining"] ?? lower["anthropic-ratelimit-requests-remaining"]);
    if (Number.isFinite(limit) && limit > 0 && Number.isFinite(remaining)) {
      return {
        usedPercent: clampPercent(((limit - remaining) / limit) * 100) ?? 0,
        resetsAt: parseResetHeader(lower["anthropic-ratelimit-tokens-reset"] ?? lower["anthropic-ratelimit-requests-reset"]),
        label: "API",
      };
    }
  }

  if (providerFamily(provider) === "openai") {
    const limit = Number(lower["x-ratelimit-limit-tokens"] ?? lower["x-ratelimit-limit-requests"]);
    const remaining = Number(lower["x-ratelimit-remaining-tokens"] ?? lower["x-ratelimit-remaining-requests"]);
    if (Number.isFinite(limit) && limit > 0 && Number.isFinite(remaining)) {
      return {
        usedPercent: clampPercent(((limit - remaining) / limit) * 100) ?? 0,
        resetsAt: parseResetHeader(lower["x-ratelimit-reset-tokens"] ?? lower["x-ratelimit-reset-requests"]),
        label: "API",
      };
    }
  }

  return undefined;
}

function hasConfiguredAuth(ctx: ExtensionContext, model: ModelLike): boolean {
  try {
    return ctx.modelRegistry.hasConfiguredAuth(model as any);
  } catch {
    return false;
  }
}

function isOAuthModel(ctx: ExtensionContext, model: ModelLike | undefined): boolean {
  if (!model) return false;
  try {
    return ctx.modelRegistry.isUsingOAuth(model as any);
  } catch {
    return false;
  }
}

function preferRepresentative(ctx: ExtensionContext, candidate: ModelLike, existing: ModelLike): boolean {
  const current = ctx.model as ModelLike | undefined;
  if (isSameModel(candidate, current)) return true;
  if (isSameModel(existing, current)) return false;

  const candidateOAuth = isOAuthModel(ctx, candidate);
  const existingOAuth = isOAuthModel(ctx, existing);
  if (candidateOAuth !== existingOAuth) return candidateOAuth;

  return candidate.id.length < existing.id.length;
}

function shouldShowInUsage(model: ModelLike | undefined): boolean {
  // Claude's claude.ai usage endpoint is web-protected and unreliable from Pi.
  // If users want Claude subscription access, OPPi integrates through Meridian;
  // Anthropic/Meridian Claude usage is intentionally omitted from /usage and footer bars.
  return providerFamily(model?.provider) !== "anthropic" && model?.provider !== "meridian";
}

function connectedUsageModels(ctx: ExtensionContext): ModelLike[] {
  const current = shouldShowInUsage(ctx.model as ModelLike | undefined) ? (ctx.model as ModelLike | undefined) : undefined;
  const all = ctx.modelRegistry.getAll() as ModelLike[];
  const available = all.filter((model) => shouldShowInUsage(model) && hasConfiguredAuth(ctx, model));
  const candidates = current ? [current, ...available] : available;
  const representatives = new Map<string, ModelLike>();

  for (const model of candidates) {
    if (!model?.provider || !model.id || !shouldShowInUsage(model)) continue;
    if (!isSameModel(model, current) && !hasConfiguredAuth(ctx, model)) continue;
    const group = usageGroupKey(model);
    const existing = representatives.get(group);
    if (!existing || preferRepresentative(ctx, model, existing)) representatives.set(group, model);
  }

  const priority = (model: ModelLike) => {
    if (isSameModel(model, current)) return -10;
    const group = usageGroupKey(model);
    if (group === "openai-codex") return 0;
    if (group === "anthropic") return 1;
    if (group === "openai") return 2;
    return 10;
  };

  return Array.from(representatives.values()).sort((a, b) => priority(a) - priority(b) || a.provider.localeCompare(b.provider) || a.id.localeCompare(b.id));
}

async function refreshUsageForModel(ctx: ExtensionContext, model: ModelLike | undefined, force = false): Promise<UsageSnapshot> {
  const key = modelKeyFor(model);
  const last = refreshTimes.get(key) ?? 0;
  if (!force && Date.now() - last < USAGE_REFRESH_MS) {
    return currentUsageForModel(ctx, model);
  }

  refreshTimes.set(key, Date.now());
  if (model && providerFamily(model.provider) === "openai-codex" && isOAuthModel(ctx, model)) {
    try {
      const snapshot = await fetchOpenAICodexUsage(ctx, model);
      remoteUsageCache.set(key, snapshot);
      requestRender?.();
      return snapshot;
    } catch (error) {
      const fallback = localSnapshot(ctx, model, [`Could not fetch live ChatGPT usage: ${compactProviderError(error)}`]);
      remoteUsageCache.set(key, fallback);
      requestRender?.();
      return fallback;
    }
  }

  const fallback = localSnapshot(ctx, model, ["Live subscription windows are not exposed for this provider yet; showing local rolling usage."]);
  remoteUsageCache.set(key, fallback);
  requestRender?.();
  return fallback;
}

function currentUsageForModel(ctx: ExtensionContext, model: ModelLike | undefined = ctx.model as ModelLike | undefined): UsageSnapshot {
  const key = modelKeyFor(model);
  const cached = remoteUsageCache.get(key);
  if (cached) return cached;

  const header = headerLimitCache.get(key);
  if (header) {
    const local = localSnapshot(ctx, model);
    return {
      ...local,
      source: "headers",
      notes: ["Using latest provider rate-limit headers plus local rolling usage."],
      fiveHour: {
        label: header.label,
        usedPercent: clampPercent(header.usedPercent),
        resetsAt: header.resetsAt,
        source: "headers",
      },
    };
  }

  return localSnapshot(ctx, model);
}

function currentUsageReport(ctx: ExtensionContext): UsageReport {
  const models = connectedUsageModels(ctx);
  const selected = ctx.model as ModelLike | undefined;
  const activeModel = shouldShowInUsage(selected) ? selected : models[0];
  const active = currentUsageForModel(ctx, activeModel);
  const snapshots = models.length > 0 ? models.map((model) => currentUsageForModel(ctx, model)) : [active];
  if (!snapshots.some((snapshot) => snapshot.key === active.key)) snapshots.unshift(active);
  return { snapshots, active, context: contextWindow(ctx), generatedAt: Date.now() };
}

async function refreshUsageReport(ctx: ExtensionContext, force = false): Promise<UsageReport> {
  const models = connectedUsageModels(ctx);
  const selected = ctx.model as ModelLike | undefined;
  const activeModel = shouldShowInUsage(selected) ? selected : models[0];
  const uniqueModels = new Map<string, ModelLike | undefined>();
  if (activeModel) uniqueModels.set(modelKeyFor(activeModel), activeModel);
  for (const model of models) uniqueModels.set(modelKeyFor(model), model);
  if (uniqueModels.size === 0) uniqueModels.set("unknown/unknown", undefined);

  const snapshots = await Promise.all(Array.from(uniqueModels.values()).map((model) => refreshUsageForModel(ctx, model, force)));
  const active = snapshots.find((snapshot) => snapshot.key === modelKeyFor(activeModel)) ?? snapshots[0];
  return { snapshots, active, context: contextWindow(ctx), generatedAt: Date.now() };
}

function backgroundRefresh(ctx: ExtensionContext, force = false): void {
  if (!ctx.hasUI) return;
  void refreshUsageReport(ctx, force).catch(() => undefined);
}

function pwdLine(ctx: ExtensionContext, footerData: any, theme: Theme, width: number): string {
  let pwd = ctx.cwd;
  const home = process.env.HOME || process.env.USERPROFILE;
  if (home && pwd.startsWith(home)) pwd = `~${pwd.slice(home.length)}`;

  const branch = footerData.getGitBranch?.();
  if (branch) pwd = `${pwd} (${branch})`;

  const sessionName = ctx.sessionManager.getSessionName();
  if (sessionName) pwd = `${pwd} • ${sessionName}`;

  return truncateToWidth(theme.fg("dim", pwd), width, theme.fg("dim", "…"));
}

function compactModelId(modelId: string): string {
  return modelId.replace(/^models\//, "");
}

function thinkingToken(thinking: string): any {
  return `thinking${thinking[0]?.toUpperCase() ?? ""}${thinking.slice(1)}` as any;
}

function modelLabel(pi: ExtensionAPI, ctx: ExtensionContext, theme: Theme, includeProvider = false): string {
  const model = ctx.model as ModelLike | undefined;
  if (!model) return theme.fg("dim", "no-model");

  const slug = compactModelId(model.id);
  const modelPart = includeProvider
    ? `${theme.fg("dim", `${model.provider}/`)}${theme.fg("accent", slug)}`
    : theme.fg("accent", slug);
  const thinking = model.reasoning ? String(pi.getThinkingLevel() ?? "") : "";
  if (!thinking) return modelPart;

  return [
    modelPart,
    theme.fg("dim", " • "),
    theme.fg(thinkingToken(thinking), thinking),
  ].join("");
}

function joinFooterParts(parts: string[], theme: Theme): string {
  return parts.join(theme.fg("dim", "  ·  "));
}

function alignRight(line: string, width: number): string {
  const lineWidth = visibleWidth(line);
  if (lineWidth >= width) return truncateToWidth(line, width, "…");
  return " ".repeat(width - lineWidth) + line;
}

function permissionLabel(theme: Theme): string {
  const color = permissionMode === "read-only"
    ? "success"
    : permissionMode === "default"
      ? "warning"
      : permissionMode === "full-access"
        ? "error"
        : "accent";
  return `${theme.fg("dim", "perm ")}${theme.fg(color as any, permissionMode)}`;
}

function keybindingHintsLine(theme: Theme, width: number, footerData?: any): string {
  const hasSuggestion = Boolean(footerData?.getExtensionStatuses?.()?.get?.("oppi.suggestNext"));
  if (hasSuggestion) {
    const suggestionParts = [
      `${theme.fg("accent", "Enter")} ${theme.fg("dim", "send suggestion")}`,
      `${theme.fg("accent", "→")} ${theme.fg("dim", "accept")}`,
      `${theme.fg("accent", "type")} ${theme.fg("dim", "replace")}`,
    ];
    return truncateToWidth(joinFooterParts(suggestionParts, theme), width, theme.fg("dim", "…"));
  }

  const fullParts = [
    `${theme.fg("accent", "Enter")} ${theme.fg("dim", "follow-up")}`,
    `${theme.fg("accent", "Ctrl+Enter")} ${theme.fg("dim", "steer")}`,
    `${theme.fg("accent", "Shift+Enter")} ${theme.fg("dim", "newline")}`,
    `${theme.fg("accent", "Alt+Up")} ${theme.fg("dim", "edit queued")}`,
    `${theme.fg("accent", "Ctrl+Alt+B")} ${theme.fg("dim", "background")}`,
  ];
  const compactParts = [
    `${theme.fg("accent", "Enter")} ${theme.fg("dim", "follow-up")}`,
    `${theme.fg("accent", "Ctrl+Enter")} ${theme.fg("dim", "steer")}`,
    `${theme.fg("accent", "Shift+Enter")} ${theme.fg("dim", "newline")}`,
  ];
  const full = joinFooterParts(fullParts, theme);
  const compact = joinFooterParts(compactParts, theme);
  return truncateToWidth(visibleWidth(full) <= width ? full : compact, width, theme.fg("dim", "…"));
}

function footerStatsLine(pi: ExtensionAPI, ctx: ExtensionContext, theme: Theme, width: number, footerData?: any): string {
  const report = currentUsageReport(ctx);
  const selected = ctx.model as ModelLike | undefined;
  const selectedHasUsage = shouldShowInUsage(selected);
  const active = selectedHasUsage ? currentUsageForModel(ctx, selected) : undefined;
  const memoryStatus = footerData?.getExtensionStatuses?.()?.get?.("oppi.memory");
  const memoryPart = typeof memoryStatus === "string" && memoryStatus.trim()
    ? theme.fg("accent", memoryStatus.trim())
    : undefined;
  const fullParts = active
    ? [
        footerLimitWindow(active.fiveHour, theme, 8),
        footerLimitWindow(active.weekly, theme, 8),
        modelLabel(pi, ctx, theme, true),
        permissionLabel(theme),
        memoryPart,
        footerContextWindow(report.context, theme, 8),
      ].filter((part): part is string => Boolean(part))
    : [modelLabel(pi, ctx, theme, true), permissionLabel(theme), memoryPart, footerContextWindow(report.context, theme, 8)].filter((part): part is string => Boolean(part));
  let line = joinFooterParts(fullParts, theme);
  if (visibleWidth(line) <= width) return alignRight(line, width);

  const compactParts = active
    ? [
        footerLimitWindow(active.fiveHour, theme, 5),
        footerLimitWindow(active.weekly, theme, 5),
        modelLabel(pi, ctx, theme, false),
        permissionLabel(theme),
        memoryPart,
        footerContextWindow(report.context, theme, 5),
      ].filter((part): part is string => Boolean(part))
    : [modelLabel(pi, ctx, theme, false), permissionLabel(theme), memoryPart, footerContextWindow(report.context, theme, 5)].filter((part): part is string => Boolean(part));
  line = joinFooterParts(compactParts, theme);
  return alignRight(line, width);
}

class OppiFooter {
  constructor(
    private readonly pi: ExtensionAPI,
    private readonly ctx: ExtensionContext,
    private readonly theme: Theme,
    private readonly footerData: any,
  ) {}

  invalidate(): void {}
  dispose(): void {}

  render(width: number): string[] {
    // Keep OPPi's footer intentionally tight. Do not append extension status
    // passthrough lines here: token/cost meter extensions commonly publish
    // verbose status strings ("sess ... ($...) | week ..."), which recreates
    // the clutter this footer is meant to replace.
    return [
      pwdLine(this.ctx, this.footerData, this.theme, width),
      footerStatsLine(this.pi, this.ctx, this.theme, width, this.footerData),
      keybindingHintsLine(this.theme, width, this.footerData),
    ];
  }
}

function installFooter(pi: ExtensionAPI, ctx: ExtensionContext): void {
  if (!ctx.hasUI) return;
  ctx.ui.setFooter((tui, theme, footerData) => {
    requestRender = () => tui.requestRender();
    return new OppiFooter(pi, ctx, theme, footerData);
  });
}

function installFooterAfterSessionHandlers(pi: ExtensionAPI, ctx: ExtensionContext): void {
  installFooter(pi, ctx);
  // Re-assert once after the current session_start dispatch finishes. This
  // makes the OPPi footer win over any other package that also calls setFooter
  // during startup without requiring a Pi fork.
  setTimeout(() => {
    try {
      installFooter(pi, ctx);
      requestRender?.();
    } catch {
      // Session may already be stale in short print-mode runs.
    }
  }, 0);
}

function usageWindowLine(window: UsageWindow, theme: Theme): string {
  const pct = window.usedPercent === null ? "unknown" : `${window.usedPercent.toFixed(1)}% used`;
  const left = formatTimeLeft(window.resetsAt, displayWindowLabel(window.label) === "5h");
  const reset = formatReset(window.resetsAt);
  const tokens = window.limitTokens
    ? ` • ${formatTokens(window.usedTokens) || "?"}/${formatTokens(window.limitTokens)}`
    : window.usedTokens !== undefined && window.usedTokens !== null
      ? ` • ${formatTokens(window.usedTokens)} observed`
      : "";
  const note = window.note ? theme.fg("dim", ` • ${window.note}`) : "";
  const resetText = left ? ` • ${left} left${reset ? ` (resets ${reset})` : ""}` : reset ? ` • resets ${reset}` : "";
  return `${window.label.padEnd(12)} ${bar(window.usedPercent, 20, theme)}  ${pct}${resetText}${tokens}${note}`;
}

function snapshotHeader(snapshot: UsageSnapshot, activeKey: string, theme: Theme): string {
  const active = snapshot.key === activeKey ? theme.fg("success", " active") : "";
  const plan = snapshot.planType ? ` • ${snapshot.planType}` : "";
  return `${theme.fg("accent", theme.bold(`${snapshot.provider}/${snapshot.modelId}`))}${active}${theme.fg("dim", ` • ${snapshot.auth} • ${snapshot.source}${plan}`)}`;
}

function usageLines(ctx: ExtensionContext, report: UsageReport, theme: Theme): string[] {
  const lines: string[] = [];
  lines.push(theme.fg("accent", theme.bold("OPPi Usage")));
  lines.push("");

  for (const [index, snapshot] of report.snapshots.entries()) {
    if (index > 0) lines.push("");
    lines.push(snapshotHeader(snapshot, report.active.key, theme));
    lines.push(usageWindowLine(snapshot.fiveHour, theme));
    lines.push(usageWindowLine(snapshot.weekly, theme));

    if (snapshot.additionalWindows?.length) {
      lines.push(theme.fg("muted", "Provider buckets:"));
      for (const window of snapshot.additionalWindows) lines.push(usageWindowLine(window, theme));
    }

    if (snapshot.notes.length > 0) {
      for (const note of snapshot.notes) lines.push(theme.fg("dim", `• ${note}`));
    }
  }

  lines.push("");
  lines.push(theme.fg("muted", "Active context:"));
  lines.push(usageWindowLine(report.context, theme));

  const localOnly = report.snapshots.some((snapshot) => snapshot.source === "local" || snapshot.source === "headers");
  if (localOnly) {
    lines.push("");
    lines.push(theme.fg("dim", "Tip: set OPPI_USAGE_<PROVIDER>_5H_TOKENS and OPPI_USAGE_<PROVIDER>_WEEKLY_TOKENS for calibrated local bars."));
  }

  lines.push("");
  lines.push(theme.fg("dim", "Enter/Esc closes • /usage refreshes connected non-Claude providers"));
  return lines;
}

async function showUsage(ctx: ExtensionCommandContext, report: UsageReport): Promise<void> {
  if (!ctx.hasUI) {
    const summary = report.snapshots
      .map((snapshot) => `${snapshot.provider}/${snapshot.modelId}: ${snapshot.fiveHour.label} ${snapshot.fiveHour.usedPercent ?? "?"}% • ${snapshot.weekly.label} ${snapshot.weekly.usedPercent ?? "?"}%`)
      .join(" | ");
    ctx.ui.notify(summary, "info");
    return;
  }

  await ctx.ui.custom<void>((_tui, theme, _kb, done) => {
    const container = new Container();
    const rebuild = () => {
      container.clear();
      container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));
      for (const line of usageLines(ctx, report, theme)) container.addChild(new Text(line, 1, 0));
      container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));
    };
    rebuild();
    return {
      render: (width: number) => container.render(width),
      invalidate: () => {
        container.invalidate();
        rebuild();
      },
      handleInput: (data: string) => {
        if (matchesKey(data, "enter") || matchesKey(data, "escape") || matchesKey(data, "ctrl+c")) done();
      },
    };
  });
}

export default function usageExtension(pi: ExtensionAPI) {
  pi.events.on("oppi.permissions.mode", (data) => {
    const mode = (data as { mode?: string } | undefined)?.mode;
    if (mode) {
      permissionMode = mode;
      requestRender?.();
    }
  });

  pi.on("session_start", async (_event, ctx) => {
    installFooterAfterSessionHandlers(pi, ctx);
    backgroundRefresh(ctx);
  });

  pi.on("model_select", async (_event, ctx) => {
    installFooter(pi, ctx);
    backgroundRefresh(ctx, true);
  });

  pi.on("after_provider_response", async (event, ctx) => {
    const model = ctx.model as ModelLike | undefined;
    if (!model || !shouldShowInUsage(model)) return;
    const headerLimit = parseHeaderLimit(model.provider, event.headers);
    if (headerLimit) {
      headerLimitCache.set(modelKey(ctx), headerLimit);
      requestRender?.();
    }
  });

  pi.on("agent_end", async (_event, ctx) => {
    installFooter(pi, ctx);
    backgroundRefresh(ctx);
    requestRender?.();
  });

  pi.on("session_shutdown", async () => {
    requestRender = undefined;
  });

  pi.registerCommand("usage", {
    description: "Show unified usage for connected non-Claude providers plus active context.",
    handler: async (_args, ctx) => {
      ctx.ui.notify("Refreshing connected provider usage…", "info");
      const report = await refreshUsageReport(ctx, true);
      await showUsage(ctx, report);
    },
  });

  pi.registerCommand("stats", {
    description: "Alias for /usage. OPPi replaces Pi's token/cost stats with subscription-aware usage.",
    handler: async (_args, ctx) => {
      ctx.ui.notify("/stats is now /usage in OPPi.", "info");
      const report = await refreshUsageReport(ctx, true);
      await showUsage(ctx, report);
    },
  });
}
