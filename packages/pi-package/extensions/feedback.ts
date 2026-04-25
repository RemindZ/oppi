import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { StringEnum } from "@mariozechner/pi-ai";
import { Type, type Static } from "typebox";
import { mkdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { join } from "node:path";
import { arch, platform, release } from "node:os";

const DEFAULT_REPO = "RemindZ/oppi";
const DEFAULT_ENDPOINT = "";
const MAX_FIELD_CHARS = 12_000;
const MAX_LOG_CHARS = 24_000;

const feedbackTypeSchema = StringEnum(["bug-report", "feature-request"] as const, {
  description: "Feedback type. Use bug-report for broken behavior and feature-request for requested behavior.",
});

const feedbackSubmitSchema = Type.Object(
  {
    type: feedbackTypeSchema,
    summary: Type.String({ description: "Short issue title, ideally under 100 characters." }),
    description: Type.Optional(Type.String({ description: "General description supplied by the user." })),

    // Bug-report fields
    whatHappened: Type.Optional(Type.String({ description: "What went wrong or what the user observed." })),
    expectedBehavior: Type.Optional(Type.String({ description: "What the user expected instead." })),
    reproduction: Type.Optional(Type.String({ description: "Steps, command, workflow, or context that reproduces the problem." })),
    impact: Type.Optional(Type.String({ description: "Impact, severity, or how often it happens." })),

    // Feature request fields
    requestedBehavior: Type.Optional(Type.String({ description: "The behavior or capability the user wants." })),
    userValue: Type.Optional(Type.String({ description: "Why this would help the user's workflow." })),
    exampleWorkflow: Type.Optional(Type.String({ description: "Example workflow, UI sketch, command, or scenario." })),
    acceptanceCriteria: Type.Optional(Type.Array(Type.String(), { description: "Concrete acceptance criteria or examples." })),

    includeDiagnostics: Type.Optional(Type.Boolean({ description: "Include sanitized local diagnostics. Defaults to true." })),
    includeLogs: Type.Optional(Type.Boolean({ description: "Include sanitized command/context logs where available. Defaults to false." })),
    repo: Type.Optional(Type.String({ description: "Target GitHub repo, owner/name. Defaults to RemindZ/oppi." })),
  },
  { additionalProperties: false },
);

type FeedbackSubmitParams = Static<typeof feedbackSubmitSchema>;
type FeedbackKind = FeedbackSubmitParams["type"];

type FeedbackDetails = {
  type: FeedbackKind;
  repo: string;
  endpoint?: string;
  draftPath?: string;
  issueUrl?: string;
  submitted: boolean;
};

function clamp(value: string | undefined, max = MAX_FIELD_CHARS): string | undefined {
  if (!value) return undefined;
  const normalized = value.trim();
  if (!normalized) return undefined;
  return normalized.length > max ? `${normalized.slice(0, max)}\n… [truncated]` : normalized;
}

function sanitize(value: string | undefined, max = MAX_LOG_CHARS): string | undefined {
  const input = clamp(value, max);
  if (!input) return undefined;

  return input
    .replace(/(authorization\s*[:=]\s*bearer\s+)[^\s\n]+/gi, "$1<redacted>")
    .replace(/((?:api[_-]?key|token|password|secret|client[_-]?secret)\s*[:=]\s*)[^\s\n]+/gi, "$1<redacted>")
    .replace(/(gh[pousr]_[A-Za-z0-9_]+)/g, "<redacted-github-token>")
    .replace(/(sk-[A-Za-z0-9_-]{16,})/g, "<redacted-openai-key>")
    .replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g, "<redacted-private-key>")
    .replace(/([A-Z0-9_]{8,}\s*=\s*)[A-Za-z0-9_./+=:-]{16,}/g, "$1<redacted>");
}

function markdownSection(title: string, body: string | undefined): string {
  const trimmed = clamp(body);
  if (!trimmed) return "";
  return `## ${title}\n\n${trimmed}\n\n`;
}

function listSection(title: string, items: string[] | undefined): string {
  const filtered = (items ?? []).map((item) => clamp(item, 500)).filter((item): item is string => Boolean(item));
  if (filtered.length === 0) return "";
  return `## ${title}\n\n${filtered.map((item) => `- ${item}`).join("\n")}\n\n`;
}

function hasEnoughContext(params: FeedbackSubmitParams): { ok: boolean; missing: string[] } {
  const missing: string[] = [];
  if (!clamp(params.summary, 160)) missing.push("summary");

  if (params.type === "bug-report") {
    if (!clamp(params.whatHappened) && !clamp(params.description)) missing.push("what happened");
    if (!clamp(params.expectedBehavior)) missing.push("expected behavior");
    if (!clamp(params.reproduction)) missing.push("reproduction/context");
  } else {
    if (!clamp(params.requestedBehavior) && !clamp(params.description)) missing.push("requested behavior");
    if (!clamp(params.userValue)) missing.push("why it matters / workflow value");
    if (!clamp(params.exampleWorkflow) && (!params.acceptanceCriteria || params.acceptanceCriteria.length === 0)) {
      missing.push("example workflow or acceptance criteria");
    }
  }

  return { ok: missing.length === 0, missing };
}

async function git(pi: ExtensionAPI, cwd: string, args: string[], timeout = 5_000): Promise<string | undefined> {
  const result = await pi.exec("git", args, { cwd, timeout }).catch(() => undefined as any);
  if (!result || result.code !== 0) return undefined;
  return sanitize(String(result.stdout ?? ""), 8_000)?.trim();
}

async function collectDiagnostics(pi: ExtensionAPI, cwd: string, includeLogs: boolean): Promise<Record<string, string | undefined>> {
  const [remote, branch, commit, status, piVersion] = await Promise.all([
    git(pi, cwd, ["remote", "get-url", "origin"]),
    git(pi, cwd, ["branch", "--show-current"]),
    git(pi, cwd, ["rev-parse", "--short=12", "HEAD"]),
    includeLogs ? git(pi, cwd, ["status", "--short"], 8_000) : Promise.resolve(undefined),
    pi.exec("pi", ["--version"], { cwd, timeout: 5_000 }).then((r) => sanitize(String(r.stdout || r.stderr || ""), 2_000)?.trim()).catch(() => undefined),
  ]);

  return {
    platform: `${platform()} ${release()} ${arch()}`,
    cwd,
    gitRemote: remote,
    gitBranch: branch,
    gitCommit: commit,
    gitStatus: status,
    piVersion,
  };
}

function renderBody(params: FeedbackSubmitParams, diagnostics?: Record<string, string | undefined>): string {
  let body = "";

  if (params.type === "bug-report") {
    body += markdownSection("Summary", params.summary);
    body += markdownSection("What happened", params.whatHappened || params.description);
    body += markdownSection("Expected behavior", params.expectedBehavior);
    body += markdownSection("Reproduction / context", params.reproduction);
    body += markdownSection("Impact", params.impact);
  } else {
    body += markdownSection("Summary", params.summary);
    body += markdownSection("Requested behavior", params.requestedBehavior || params.description);
    body += markdownSection("Why this matters", params.userValue);
    body += markdownSection("Example workflow", params.exampleWorkflow);
    body += listSection("Acceptance criteria", params.acceptanceCriteria);
  }

  if (diagnostics) {
    const lines = Object.entries(diagnostics)
      .filter(([, value]) => Boolean(value))
      .map(([key, value]) => `- ${key}: ${value}`);
    if (lines.length) body += `## Sanitized diagnostics\n\n${lines.join("\n")}\n\n`;
  }

  body += "---\n\nCreated from OPPi feedback intake. Sensitive values are redacted client-side and again by the intake worker.\n";
  return body;
}

function titleFor(params: FeedbackSubmitParams): string {
  const prefix = params.type === "bug-report" ? "[Bug]" : "[Feature]";
  const summary = clamp(params.summary, 120) || (params.type === "bug-report" ? "Bug report" : "Feature request");
  return `${prefix} ${summary}`.replace(/\s+/g, " ").trim().slice(0, 140);
}

function feedbackEndpoint(): string {
  return process.env.OPPI_FEEDBACK_ENDPOINT || DEFAULT_ENDPOINT;
}

function feedbackRepo(params: FeedbackSubmitParams): string {
  return params.repo || process.env.OPPI_FEEDBACK_REPO || DEFAULT_REPO;
}

async function writeDraft(cwd: string, params: FeedbackSubmitParams, body: string): Promise<string> {
  const dir = join(cwd, ".oppi", "feedback-drafts");
  await mkdir(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const hash = createHash("sha256").update(`${params.type}:${params.summary}:${stamp}`).digest("hex").slice(0, 8);
  const file = join(dir, `${stamp}-${params.type}-${hash}.md`);
  await writeFile(file, `# ${titleFor(params)}\n\n${body}`, "utf8");
  return file;
}

async function submitToWorker(endpoint: string, repo: string, params: FeedbackSubmitParams, body: string): Promise<string> {
  const url = `${endpoint.replace(/\/$/, "")}/v1/intake/${params.type}`;
  const headers: Record<string, string> = {
    "content-type": "application/json",
    "user-agent": "oppi-pi-package-feedback/0.0.0",
  };
  if (process.env.OPPI_FEEDBACK_TOKEN) {
    headers["x-oppi-intake-token"] = process.env.OPPI_FEEDBACK_TOKEN;
  }

  const response = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify({
      repo,
      type: params.type,
      title: titleFor(params),
      body,
      summary: clamp(params.summary, 200),
      labels: params.type === "bug-report" ? ["bug"] : ["enhancement"],
    }),
  });

  const text = await response.text();
  let parsed: any;
  try {
    parsed = text ? JSON.parse(text) : undefined;
  } catch {
    parsed = undefined;
  }

  if (!response.ok) {
    const message = parsed?.error || parsed?.message || text || `HTTP ${response.status}`;
    throw new Error(`feedback worker rejected request: ${message}`);
  }

  const issueUrl = parsed?.issueUrl || parsed?.html_url;
  if (!issueUrl) throw new Error("feedback worker response did not include issueUrl");
  return issueUrl;
}

function triagePrompt(type: FeedbackKind, initialDescription: string): string {
  const commandName = type === "bug-report" ? "/bug-report" : "/feature-request";
  const required =
    type === "bug-report"
      ? "summary, what happened, expected behavior, reproduction/context, and whether diagnostics/logs may be included"
      : "summary, requested behavior, why it matters, example workflow or acceptance criteria, and whether diagnostics may be included";

  const starter = initialDescription.trim()
    ? `The user started ${commandName} with this description:\n\n${initialDescription.trim()}`
    : `The user started ${commandName} without a description.`;

  return `${starter}\n\nYour job: help create a high-quality OPPi ${type === "bug-report" ? "bug report" : "feature request"}.\n\nVerify enough context before submitting. Required context: ${required}. If anything important is missing, ask concise follow-up questions first. Once you have enough context, call the \`oppi_feedback_submit\` tool with structured fields. Prefer includeDiagnostics=true. Only set includeLogs=true if the user agrees or logs are clearly needed. Do not include secrets or raw private conversation history.`;
}

export default function feedbackExtension(pi: ExtensionAPI) {
  pi.registerCommand("bug-report", {
    description: "Create an OPPi bug report via the feedback intake flow. Usage: /bug-report [description]",
    handler: async (args, ctx) => {
      await ctx.sendUserMessage(triagePrompt("bug-report", args));
    },
  });

  pi.registerCommand("feature-request", {
    description: "Create an OPPi feature request via the feedback intake flow. Usage: /feature-request [description]",
    handler: async (args, ctx) => {
      await ctx.sendUserMessage(triagePrompt("feature-request", args));
    },
  });

  pi.registerTool({
    name: "oppi_feedback_submit",
    label: "OPPi Feedback",
    description:
      "Submit a validated OPPi bug report or feature request. Use only after you have enough user-provided context. Creates a GitHub issue through the configured OPPi intake worker, or writes a local draft when no endpoint is configured.",
    parameters: feedbackSubmitSchema,
    async execute(_toolCallId, params: FeedbackSubmitParams, _signal, _onUpdate, ctx) {
      const repo = feedbackRepo(params);
      const sufficiency = hasEnoughContext(params);
      if (!sufficiency.ok) {
        return {
          content: [
            {
              type: "text",
              text: `Not enough context to submit ${params.type}. Ask the user for: ${sufficiency.missing.join(", ")}.`,
            },
          ],
          details: { type: params.type, repo, submitted: false } satisfies FeedbackDetails,
        };
      }

      const includeDiagnostics = params.includeDiagnostics !== false;
      const includeLogs = params.includeLogs === true;
      const diagnostics = includeDiagnostics ? await collectDiagnostics(pi, ctx.cwd, includeLogs) : undefined;
      const body = renderBody(params, diagnostics);
      const endpoint = feedbackEndpoint();

      if (!endpoint || process.env.OPPI_FEEDBACK_DISABLED === "1") {
        const draftPath = await writeDraft(ctx.cwd, params, body);
        return {
          content: [
            {
              type: "text",
              text: `Feedback intake endpoint is not configured, so I wrote a local draft instead:\n${draftPath}\n\nSet OPPI_FEEDBACK_ENDPOINT to submit directly through the OPPi intake worker.`,
            },
          ],
          details: { type: params.type, repo, draftPath, submitted: false } satisfies FeedbackDetails,
        };
      }

      const issueUrl = await submitToWorker(endpoint, repo, params, body);
      return {
        content: [{ type: "text", text: `Created ${params.type}: ${issueUrl}` }],
        details: { type: params.type, repo, endpoint, issueUrl, submitted: true } satisfies FeedbackDetails,
      };
    },
  });
}
