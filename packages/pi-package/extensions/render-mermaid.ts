import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Text } from "@mariozechner/pi-tui";
import { Type } from "typebox";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, isAbsolute, resolve } from "node:path";

const MAX_MERMAID_SOURCE_LENGTH = 20_000;
const MAX_RENDERED_OUTPUT_LENGTH = 80_000;
const MAX_RENDER_LINES = 240;

const renderMermaidSchema = Type.Object(
  {
    mermaid: Type.String({ description: "Mermaid diagram source to render as terminal-friendly ASCII." }),
    config: Type.Optional(
      Type.Any({
        description:
          "Optional renderer configuration for beautiful-mermaid. Only plain JSON values are forwarded; invalid values are ignored.",
      }),
    ),
    outputPath: Type.Optional(
      Type.String({
        description:
          "Optional text file path for the full ASCII output. Relative paths resolve from the current working directory.",
      }),
    ),
    overwrite: Type.Optional(Type.Boolean({ description: "Allow overwriting outputPath. Defaults to false." })),
  },
  { additionalProperties: false },
) as any;

type RenderMermaidInput = {
  mermaid: string;
  config?: unknown;
  outputPath?: string;
  overwrite?: boolean;
};

type RenderMermaidDetails = {
  engine: "beautiful-mermaid" | "fallback";
  ascii: string;
  outputPath?: string;
  truncated: boolean;
};

type MermaidRenderer = (source: string, config?: Record<string, unknown>) => string;

function normalizeSource(source: string): string {
  return source.replace(/\r\n?/g, "\n").trim();
}

function sanitizeConfig(value: unknown): Record<string, unknown> | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;

  const clean: Record<string, unknown> = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    if (typeof raw === "string" || typeof raw === "boolean") {
      clean[key] = raw;
    } else if (typeof raw === "number" && Number.isFinite(raw)) {
      clean[key] = raw;
    } else if (raw && typeof raw === "object" && !Array.isArray(raw)) {
      const nested = sanitizeConfig(raw);
      if (nested && Object.keys(nested).length > 0) clean[key] = nested;
    }
  }

  return Object.keys(clean).length > 0 ? clean : undefined;
}

async function loadBeautifulMermaidRenderer(): Promise<MermaidRenderer | undefined> {
  try {
    const dynamicImport = new Function("specifier", "return import(specifier)") as (specifier: string) => Promise<Record<string, unknown>>;
    const mod = await dynamicImport("beautiful-mermaid");
    const renderer = mod.renderMermaidASCII ?? mod.renderMermaidAscii ?? (mod.default as Record<string, unknown> | undefined)?.renderMermaidASCII;
    return typeof renderer === "function" ? (renderer as MermaidRenderer) : undefined;
  } catch {
    return undefined;
  }
}

function parseNodeLabel(token: string): string {
  const trimmed = token.trim();
  const bracket = /^([A-Za-z0-9_:-]+)\s*[\[({]([^\]})]+)[\]})]/.exec(trimmed);
  if (bracket) return bracket[2].trim();
  return trimmed.replace(/^[A-Za-z0-9_:-]+\s*/, "").replace(/[\[\]{}()]/g, "").trim() || trimmed;
}

function renderFallbackFlowchart(lines: string[]): string | undefined {
  const edges: string[] = [];
  const standalone: string[] = [];

  for (const line of lines) {
    const cleaned = line.replace(/%%.*$/, "").trim();
    if (!cleaned || /^(flowchart|graph)\b/i.test(cleaned) || /^subgraph\b/i.test(cleaned) || /^end$/i.test(cleaned)) continue;

    const edgeMatch = cleaned.match(/(.+?)\s*(-->|---|==>|-.->|--[^-]+-->)\s*(.+)/);
    if (edgeMatch) {
      const from = parseNodeLabel(edgeMatch[1]);
      const to = parseNodeLabel(edgeMatch[3]);
      if (from && to) edges.push(`${from} -> ${to}`);
      continue;
    }

    const node = parseNodeLabel(cleaned);
    if (node) standalone.push(node);
  }

  if (edges.length === 0 && standalone.length === 0) return undefined;
  return ["Mermaid flowchart (simple ASCII fallback)", "", ...edges.map((edge) => `  ${edge}`), ...standalone.map((node) => `  [${node}]`)].join("\n");
}

function renderFallbackSequence(lines: string[]): string | undefined {
  const messages: string[] = [];
  const participants = new Set<string>();

  for (const line of lines) {
    const cleaned = line.replace(/%%.*$/, "").trim();
    if (!cleaned || /^sequenceDiagram\b/i.test(cleaned)) continue;
    const participant = cleaned.match(/^participant\s+([^\s]+)(?:\s+as\s+(.+))?/i);
    if (participant) {
      participants.add((participant[2] || participant[1]).trim());
      continue;
    }
    const message = cleaned.match(/^([^\s]+)\s*(-{1,2}>>\+?|-->>\+?|->>\+?)\s*([^:]+):\s*(.+)$/);
    if (message) {
      const from = message[1].trim();
      const to = message[3].trim();
      participants.add(from);
      participants.add(to);
      messages.push(`${from} -> ${to}: ${message[4].trim()}`);
    }
  }

  if (messages.length === 0) return undefined;
  return ["Mermaid sequence diagram (simple ASCII fallback)", `Participants: ${Array.from(participants).join(", ")}`, "", ...messages.map((msg) => `  ${msg}`)].join("\n");
}

function renderFallback(source: string): string {
  const lines = source.split("\n");
  const first = lines.find((line) => line.trim() && !line.trim().startsWith("%%"))?.trim() ?? "";
  if (/^(flowchart|graph)\b/i.test(first)) {
    const rendered = renderFallbackFlowchart(lines);
    if (rendered) return rendered;
  }
  if (/^sequenceDiagram\b/i.test(first)) {
    const rendered = renderFallbackSequence(lines);
    if (rendered) return rendered;
  }
  throw new Error("Mermaid rendering is unavailable and the fallback only supports simple flowchart/graph and sequenceDiagram inputs.");
}

function truncateForDisplay(value: string): { text: string; truncated: boolean } {
  const lines = value.split("\n");
  let text = lines.slice(0, MAX_RENDER_LINES).join("\n");
  let truncated = lines.length > MAX_RENDER_LINES;
  if (text.length > MAX_RENDERED_OUTPUT_LENGTH) {
    text = `${text.slice(0, MAX_RENDERED_OUTPUT_LENGTH)}\n…`;
    truncated = true;
  }
  return { text, truncated };
}

async function writeOutput(cwd: string, outputPath: string, ascii: string, overwrite: boolean | undefined): Promise<string> {
  const resolved = isAbsolute(outputPath) ? outputPath : resolve(cwd, outputPath);
  await mkdir(dirname(resolved), { recursive: true });
  await writeFile(resolved, ascii, { encoding: "utf8", flag: overwrite ? "w" : "wx" });
  return resolved;
}

export default function renderMermaidExtension(pi: ExtensionAPI) {
  const registerTool = pi.registerTool.bind(pi) as (tool: any) => void;
  registerTool({
    name: "render_mermaid",
    label: "render_mermaid",
    description: "Render Mermaid diagram source as terminal-friendly ASCII output.",
    promptSnippet: "Render Mermaid diagram source to ASCII for terminal display",
    promptGuidelines: [
      "Use render_mermaid when the user asks to render, preview, or validate Mermaid diagrams in the terminal.",
      "Prefer concise Mermaid diagrams. For ordinary explanations, include the Mermaid source and use render_mermaid only when a rendered preview helps.",
      "If render_mermaid reports fallback output, mention that full Mermaid rendering requires the beautiful-mermaid dependency to be installed.",
    ],
    parameters: renderMermaidSchema,
    async execute(
      _toolCallId: string,
      params: RenderMermaidInput,
      _signal: AbortSignal | undefined,
      onUpdate: ((partialResult: { content: { type: "text"; text: string }[]; details: RenderMermaidDetails }) => void) | undefined,
      ctx: { cwd: string },
    ) {
      const source = normalizeSource(params.mermaid);
      if (!source) throw new Error("mermaid must not be empty.");
      if (source.length > MAX_MERMAID_SOURCE_LENGTH) {
        throw new Error(`Mermaid source is too large (${source.length} chars; max ${MAX_MERMAID_SOURCE_LENGTH}).`);
      }

      onUpdate?.({ content: [{ type: "text", text: "Rendering Mermaid diagram..." }], details: { engine: "fallback", ascii: "", truncated: false } });

      const config = sanitizeConfig(params.config);
      const renderer = await loadBeautifulMermaidRenderer();
      const engine: RenderMermaidDetails["engine"] = renderer ? "beautiful-mermaid" : "fallback";
      const ascii = renderer ? renderer(source, config) : renderFallback(source);
      if (!ascii.trim()) throw new Error("Mermaid renderer returned empty output.");

      const savedPath = params.outputPath ? await writeOutput(ctx.cwd, params.outputPath, ascii, params.overwrite) : undefined;
      const display = truncateForDisplay(ascii);
      const text = [
        `Rendered Mermaid diagram with ${engine}.`,
        savedPath ? `Saved ASCII output to ${savedPath}.` : undefined,
        display.truncated ? "Output was truncated for chat display; use outputPath for the full rendering." : undefined,
        "",
        "```text",
        display.text,
        "```",
      ]
        .filter((line) => line !== undefined)
        .join("\n");

      return {
        content: [{ type: "text", text }],
        details: {
          engine,
          ascii,
          outputPath: savedPath,
          truncated: display.truncated,
        },
      };
    },
    renderCall(args: RenderMermaidInput | undefined, theme: any, context: any) {
      const text = (context.lastComponent as Text | undefined) ?? new Text("", 0, 0);
      const firstLine = typeof args?.mermaid === "string" ? normalizeSource(args.mermaid).split("\n")[0] || "diagram" : "diagram";
      text.setText(`${theme.fg("toolTitle", theme.bold("render_mermaid"))} ${theme.fg("muted", firstLine.slice(0, 120))}`);
      return text;
    },
    renderResult(result: unknown, _options: unknown, theme: any, context: any) {
      const text = (context.lastComponent as Text | undefined) ?? new Text("", 0, 0);
      const details = (result as { details?: RenderMermaidDetails }).details;
      if (!details) {
        text.setText(theme.fg("toolOutput", "Mermaid render finished."));
        return text;
      }

      const display = truncateForDisplay(details.ascii);
      text.setText(
        [
          theme.fg("success", `✓ Mermaid rendered with ${details.engine}`),
          details.outputPath ? theme.fg("accent", details.outputPath) : undefined,
          display.truncated ? theme.fg("dim", "Preview truncated; save with outputPath for full output.") : undefined,
          theme.fg("toolOutput", display.text),
        ]
          .filter(Boolean)
          .join("\n"),
      );
      return text;
    },
  });
}
