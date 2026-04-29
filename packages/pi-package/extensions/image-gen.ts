import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { StringEnum } from "@mariozechner/pi-ai";
import { Text } from "@mariozechner/pi-tui";
import { Type, type Static } from "typebox";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, extname, isAbsolute, resolve } from "node:path";
import { platform, release, arch } from "node:os";
import { readPromptVariantSurface } from "./prompt-variant";

const DEFAULT_IMAGE_MODEL = "gpt-image-2";
const DEFAULT_SIZE = "auto";
const DEFAULT_QUALITY = "medium";
const DEFAULT_OUTPUT_FORMAT = "png";
const DEFAULT_CODEX_MODEL = "gpt-5.2";
const DEFAULT_CODEX_BASE_URL = "https://chatgpt.com/backend-api/codex/responses";
const JWT_CLAIM_PATH = "https://api.openai.com/auth";
const MAX_IMAGE_BYTES = 50 * 1024 * 1024;
const CODEX_NATIVE_IMAGE_INSTRUCTIONS =
  "You are an image generation adapter. For the user's request, call the image_generation tool exactly once. Do not answer with text unless image generation is refused or impossible.";

const GPT_IMAGE_2_MIN_PIXELS = 655_360;
const GPT_IMAGE_2_MAX_PIXELS = 8_294_400;
const GPT_IMAGE_2_MAX_EDGE = 3840;
const GPT_IMAGE_2_MAX_RATIO = 3.0;

const outputFormatSchema = (description: string) => StringEnum(["png", "jpeg", "jpg", "webp"] as const, { description });
const qualitySchema = (description: string) => StringEnum(["low", "medium", "high", "auto"] as const, { description });
const backgroundSchema = (description: string) => StringEnum(["transparent", "opaque", "auto"] as const, { description });
const backendSchema = (description: string) => StringEnum(["auto", "codex_native", "image_api"] as const, { description });
const inputFidelitySchema = (description: string) => StringEnum(["low", "high"] as const, { description });
const moderationSchema = (description: string) => StringEnum(["low", "auto"] as const, { description });

const imageGenSchema = Type.Object(
  {
    prompt: Type.String({
      description:
        "Detailed image prompt. For edits, describe the desired transformation while preserving any required parts of the input image(s).",
    }),
    images: Type.Optional(
      Type.Array(Type.String(), {
        description:
          "Optional source image paths for editing or image-to-image generation. Paths are relative to cwd unless absolute. A leading @ is OK.",
      }),
    ),
    mask: Type.Optional(
      Type.String({
        description:
          "Optional PNG mask path for Image API edits. Fully transparent mask areas indicate where to edit. Requires backend=image_api/OpenAI API key.",
      }),
    ),
    outputPath: Type.Optional(
      Type.String({
        description:
          "Optional output file path for the first image. Defaults to output/imagegen/<timestamp>-<slug>.<format> under the current project.",
      }),
    ),
    n: Type.Optional(Type.Number({ description: "Number of images to generate, 1-10. Native Codex backend supports one image; n>1 requires image_api." })),
    backend: Type.Optional(
      backendSchema(
        "Execution backend. auto prefers Codex native image_generation when no low-level API controls are requested, then falls back to Image API. Use image_api to force GPT Image model selection.",
      ),
    ),
    model: Type.Optional(
      Type.String({
        description:
          "Image API model. Defaults to gpt-image-2. Use gpt-image-1.5 for true transparent background fallback after user confirmation.",
      }),
    ),
    size: Type.Optional(
      Type.String({
        description:
          "Image API size. gpt-image-2 accepts auto or WIDTHxHEIGHT with max edge <=3840, both edges multiples of 16, <=3:1 ratio, and 655,360-8,294,400 total pixels.",
      }),
    ),
    quality: Type.Optional(qualitySchema("Image API quality. Defaults to medium for gpt-image-2.")),
    outputFormat: Type.Optional(outputFormatSchema("Output image format. Defaults to png.")),
    background: Type.Optional(
      backgroundSchema(
        "Image API background. gpt-image-2 does not support transparent; use gpt-image-1.5 for true transparency after confirmation.",
      ),
    ),
    outputCompression: Type.Optional(Type.Number({ description: "Image API compression 0-100 for jpeg/webp outputs." })),
    inputFidelity: Type.Optional(
      inputFidelitySchema("Image API edit-only fidelity. Do not use with gpt-image-2; it is always high fidelity."),
    ),
    moderation: Type.Optional(moderationSchema("Image API moderation level: auto or low.")),
    overwrite: Type.Optional(Type.Boolean({ description: "Allow overwriting explicit outputPath files. Defaults to false." })),
  },
  { additionalProperties: false },
);

type ImageGenInput = Static<typeof imageGenSchema>;

type ImageGenDetails = {
  backend: "codex_native" | "image_api";
  endpoint?: string;
  model?: string;
  prompt: string;
  revisedPrompt?: string;
  outputPaths: string[];
  outputFormat: string;
  size?: string;
  quality?: string;
  usage?: unknown;
};

type GeneratedImage = {
  b64: string;
  revisedPrompt?: string;
};

function stripAt(value: string): string {
  return value.startsWith("@") ? value.slice(1) : value;
}

function resolveCwd(cwd: string, value: string): string {
  const cleaned = stripAt(value.trim());
  return isAbsolute(cleaned) ? cleaned : resolve(cwd, cleaned);
}

function slugify(value: string): string {
  const slug = value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 60);
  return slug || "image";
}

function normalizeOutputFormat(format?: string): "png" | "jpeg" | "webp" {
  const value = (format || DEFAULT_OUTPUT_FORMAT).toLowerCase();
  if (value === "jpg") return "jpeg";
  if (value === "png" || value === "jpeg" || value === "webp") return value;
  throw new Error("outputFormat must be png, jpeg, jpg, or webp.");
}

function mimeForFormat(format: string): string {
  return format === "jpg" || format === "jpeg" ? "image/jpeg" : `image/${format}`;
}

function mimeForPath(path: string): string {
  const ext = extname(path).toLowerCase();
  if (ext === ".jpg" || ext === ".jpeg") return "image/jpeg";
  if (ext === ".webp") return "image/webp";
  if (ext === ".gif") return "image/gif";
  return "image/png";
}

function parseSize(size: string): { width: number; height: number } | undefined {
  const match = /^([1-9][0-9]*)x([1-9][0-9]*)$/.exec(size);
  if (!match) return undefined;
  return { width: Number(match[1]), height: Number(match[2]) };
}

function validateImageApiParams(params: ImageGenInput, outputFormat: string): void {
  const model = params.model || DEFAULT_IMAGE_MODEL;
  const size = params.size || DEFAULT_SIZE;
  const quality = params.quality || DEFAULT_QUALITY;
  const n = params.n ?? 1;

  if (!model.startsWith("gpt-image-") && model !== "chatgpt-image-latest") {
    throw new Error("model must be a GPT Image model such as gpt-image-2, gpt-image-1.5, gpt-image-1, or gpt-image-1-mini.");
  }
  if (!Number.isInteger(n) || n < 1 || n > 10) throw new Error("n must be an integer between 1 and 10.");
  if (!["low", "medium", "high", "auto"].includes(quality)) throw new Error("quality must be low, medium, high, or auto.");
  if (params.background && !["transparent", "opaque", "auto"].includes(params.background)) {
    throw new Error("background must be transparent, opaque, or auto.");
  }
  if (params.background === "transparent" && !["png", "webp"].includes(outputFormat)) {
    throw new Error("transparent background requires outputFormat png or webp.");
  }
  if (params.outputCompression !== undefined && (params.outputCompression < 0 || params.outputCompression > 100)) {
    throw new Error("outputCompression must be between 0 and 100.");
  }

  if (model === "gpt-image-2") {
    if (params.background === "transparent") {
      throw new Error(
        "gpt-image-2 does not support background=transparent. Ask the user before switching to gpt-image-1.5 with background=transparent and outputFormat=png/webp.",
      );
    }
    if (params.inputFidelity !== undefined) {
      throw new Error("inputFidelity is not supported with gpt-image-2 because image inputs always use high fidelity.");
    }
    if (size !== "auto") {
      const parsed = parseSize(size);
      if (!parsed) throw new Error("gpt-image-2 size must be auto or WIDTHxHEIGHT, for example 1536x1024.");
      const { width, height } = parsed;
      const maxEdge = Math.max(width, height);
      const minEdge = Math.min(width, height);
      const pixels = width * height;
      if (maxEdge > GPT_IMAGE_2_MAX_EDGE) throw new Error("gpt-image-2 size max edge must be <= 3840px.");
      if (width % 16 !== 0 || height % 16 !== 0) throw new Error("gpt-image-2 size width and height must be multiples of 16px.");
      if (maxEdge / minEdge > GPT_IMAGE_2_MAX_RATIO) throw new Error("gpt-image-2 size ratio must not exceed 3:1.");
      if (pixels < GPT_IMAGE_2_MIN_PIXELS || pixels > GPT_IMAGE_2_MAX_PIXELS) {
        throw new Error("gpt-image-2 size total pixels must be between 655,360 and 8,294,400.");
      }
    }
  } else {
    const allowed = new Set(["1024x1024", "1536x1024", "1024x1536", "auto"]);
    if (!allowed.has(size)) {
      throw new Error("size must be one of 1024x1024, 1536x1024, 1024x1536, or auto for this GPT Image model.");
    }
  }
}

function hasLowLevelApiControls(params: ImageGenInput): boolean {
  return Boolean(
    params.model !== undefined ||
      params.size !== undefined ||
      params.quality !== undefined ||
      params.background !== undefined ||
      params.outputCompression !== undefined ||
      params.inputFidelity !== undefined ||
      params.moderation !== undefined ||
      params.mask !== undefined ||
      (params.n !== undefined && params.n !== 1),
  );
}

function buildOutputPaths(cwd: string, params: ImageGenInput, outputFormat: string, count: number): string[] {
  const explicit = params.outputPath ? resolveCwd(cwd, params.outputPath) : undefined;
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const base = explicit ?? resolve(cwd, "output", "imagegen", `${timestamp}-${slugify(params.prompt)}.${outputFormat}`);
  const ext = extname(base) || `.${outputFormat}`;
  const withExt = extname(base) ? base : `${base}.${outputFormat}`;

  if (count === 1) return [withExt];
  const stem = withExt.slice(0, -extname(withExt).length);
  const suffix = extname(withExt);
  return Array.from({ length: count }, (_, i) => `${stem}-${i + 1}${suffix}`);
}

async function ensureWritable(paths: string[], overwrite: boolean | undefined): Promise<void> {
  for (const path of paths) {
    try {
      await stat(path);
      if (!overwrite) throw new Error(`Output already exists: ${path}. Set overwrite=true or choose another outputPath.`);
    } catch (error) {
      if (error instanceof Error && error.message.startsWith("Output already exists:")) throw error;
    }
  }
}

async function readInputImage(path: string): Promise<{ path: string; data: Buffer; mimeType: string }> {
  const info = await stat(path);
  if (!info.isFile()) throw new Error(`Input image is not a file: ${path}`);
  if (info.size > MAX_IMAGE_BYTES) throw new Error(`Input image exceeds 50MB limit: ${path}`);
  return { path, data: await readFile(path), mimeType: mimeForPath(path) };
}

async function resolveOpenAIApiKey(ctx: any): Promise<string | undefined> {
  const fromRegistry = await ctx.modelRegistry.getApiKeyForProvider("openai").catch(() => undefined);
  if (fromRegistry && !fromRegistry.includes(".") && fromRegistry.length > 10) return fromRegistry;
  return process.env.OPENAI_API_KEY;
}

async function resolveCodexToken(ctx: any): Promise<string | undefined> {
  return ctx.modelRegistry.getApiKeyForProvider("openai-codex").catch(() => undefined);
}

function extractAccountId(token: string): string {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) throw new Error("Invalid token");
    const payload = JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
    const accountId = payload?.[JWT_CLAIM_PATH]?.chatgpt_account_id;
    if (!accountId) throw new Error("No account ID in token");
    return accountId;
  } catch {
    throw new Error("Failed to extract ChatGPT account ID from openai-codex OAuth token.");
  }
}

function buildCodexHeaders(token: string): Headers {
  const accountId = extractAccountId(token);
  const headers = new Headers();
  headers.set("Authorization", `Bearer ${token}`);
  headers.set("chatgpt-account-id", accountId);
  headers.set("originator", "pi");
  headers.set("User-Agent", `pi (${platform()} ${release()}; ${arch()})`);
  headers.set("OpenAI-Beta", "responses=experimental");
  headers.set("accept", "text/event-stream");
  headers.set("content-type", "application/json");
  return headers;
}

async function* parseSSE(response: Response): AsyncGenerator<any> {
  if (!response.body) return;
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let index = buffer.indexOf("\n\n");
      while (index !== -1) {
        const chunk = buffer.slice(0, index);
        buffer = buffer.slice(index + 2);
        const data = chunk
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trim())
          .join("\n")
          .trim();
        if (data && data !== "[DONE]") {
          try {
            yield JSON.parse(data);
          } catch {}
        }
        index = buffer.indexOf("\n\n");
      }
    }
  } finally {
    try {
      await reader.cancel();
    } catch {}
    try {
      reader.releaseLock();
    } catch {}
  }
}

async function imageUrlToBase64(url: string, signal?: AbortSignal): Promise<string> {
  const response = await fetch(url, { signal });
  if (!response.ok) throw new Error(`Failed to fetch generated image URL: ${response.status} ${response.statusText}`);
  return Buffer.from(await response.arrayBuffer()).toString("base64");
}

async function runCodexNative(params: ImageGenInput, ctx: any, signal?: AbortSignal): Promise<{ images: GeneratedImage[]; details: Partial<ImageGenDetails> }> {
  if (params.mask) throw new Error("mask requires backend=image_api; Codex native image_generation does not expose mask controls.");
  if (params.n !== undefined && params.n !== 1) throw new Error("n>1 requires backend=image_api; Codex native image_generation returns one image per call.");

  const token = await resolveCodexToken(ctx);
  if (!token) throw new Error("No openai-codex OAuth token available. Run /login and choose ChatGPT/Codex, or set OPENAI_API_KEY for backend=image_api.");

  const outputFormat = normalizeOutputFormat(params.outputFormat);
  const inputImages = await Promise.all((params.images ?? []).map((p) => readInputImage(resolveCwd(ctx.cwd, p))));
  const imagePrompt = inputImages.length > 0
    ? `Create an edited/generated image from the attached source image(s). Prompt: ${params.prompt}`
    : `Create an image. Prompt: ${params.prompt}`;

  const content: any[] = [{ type: "input_text", text: imagePrompt }];
  for (const image of inputImages) {
    content.push({ type: "input_image", detail: "high", image_url: `data:${image.mimeType};base64,${image.data.toString("base64")}` });
  }

  const model = ctx.model?.provider === "openai-codex" ? ctx.model.id : DEFAULT_CODEX_MODEL;
  const body = {
    model,
    store: false,
    stream: true,
    instructions: readPromptVariantSurface("image-gen-codex-native-adapter-instructions.md").text || CODEX_NATIVE_IMAGE_INSTRUCTIONS,
    input: [{ role: "user", content }],
    tools: [{ type: "image_generation", output_format: outputFormat }],
    tool_choice: "auto",
    parallel_tool_calls: false,
  };

  const response = await fetch(DEFAULT_CODEX_BASE_URL, {
    method: "POST",
    headers: buildCodexHeaders(token),
    body: JSON.stringify(body),
    signal,
  });

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(`Codex native image_generation failed: HTTP ${response.status} ${response.statusText}${text ? `: ${text}` : ""}`);
  }

  const images: GeneratedImage[] = [];
  let responseError: string | undefined;
  for await (const event of parseSSE(response)) {
    if (event.type === "error") {
      responseError = event.message || event.code || JSON.stringify(event);
      break;
    }
    if (event.type === "response.failed") {
      responseError = event.response?.error?.message || JSON.stringify(event.response?.error ?? event);
      break;
    }
    if (event.type === "response.output_item.done" && event.item?.type === "image_generation_call") {
      const item = event.item;
      // Codex can preserve image_generation_call history with status "generating" even
      // when the result field already contains the completed base64 payload. Treat a
      // non-empty result as authoritative and only error on non-completed statuses
      // when no image data is present.
      if (typeof item.result === "string" && item.result.length > 0) {
        images.push({ b64: item.result, revisedPrompt: item.revised_prompt });
      } else if (item.status && item.status !== "completed") {
        responseError = `image_generation_call status: ${item.status}`;
        break;
      }
    }
  }

  if (responseError) throw new Error(`Codex native image_generation failed: ${responseError}`);
  if (images.length === 0) throw new Error("Codex native image_generation completed without an image result.");
  return { images, details: { backend: "codex_native", model, endpoint: DEFAULT_CODEX_BASE_URL, revisedPrompt: images[0]?.revisedPrompt } };
}

function imageApiBaseUrl(): string {
  return (process.env.OPENAI_BASE_URL || "https://api.openai.com/v1").replace(/\/+$/, "");
}

function appendFormValue(form: FormData, key: string, value: unknown): void {
  if (value === undefined || value === null) return;
  form.append(key, String(value));
}

async function parseImageApiResponse(response: Response, signal?: AbortSignal): Promise<{ images: GeneratedImage[]; usage?: unknown }> {
  const text = await response.text();
  let json: any;
  try {
    json = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(`Image API returned non-JSON response: ${text.slice(0, 500)}`);
  }
  if (!response.ok) {
    const message = json?.error?.message || json?.message || text || `${response.status} ${response.statusText}`;
    throw new Error(`Image API failed: ${message}`);
  }

  const data = Array.isArray(json.data) ? json.data : [];
  const images: GeneratedImage[] = [];
  for (const item of data) {
    if (typeof item.b64_json === "string") {
      images.push({ b64: item.b64_json, revisedPrompt: item.revised_prompt });
    } else if (typeof item.url === "string") {
      images.push({ b64: await imageUrlToBase64(item.url, signal), revisedPrompt: item.revised_prompt });
    }
  }
  if (images.length === 0) throw new Error(`Image API returned no image data: ${text.slice(0, 500)}`);
  return { images, usage: json.usage };
}

async function runImageApi(params: ImageGenInput, ctx: any, signal?: AbortSignal): Promise<{ images: GeneratedImage[]; details: Partial<ImageGenDetails> }> {
  const apiKey = await resolveOpenAIApiKey(ctx);
  if (!apiKey) throw new Error("No OpenAI API key available. Set OPENAI_API_KEY or log in with an OpenAI API key for backend=image_api.");

  const outputFormat = normalizeOutputFormat(params.outputFormat);
  validateImageApiParams(params, outputFormat);

  const model = params.model || DEFAULT_IMAGE_MODEL;
  const size = params.size || DEFAULT_SIZE;
  const quality = params.quality || DEFAULT_QUALITY;
  const n = params.n ?? 1;
  const endpoint = params.images?.length ? "/images/edits" : "/images/generations";
  const common: Record<string, unknown> = {
    model,
    prompt: params.prompt,
    n,
    size,
    quality,
    background: params.background,
    output_format: outputFormat,
    output_compression: params.outputCompression,
    moderation: params.moderation,
  };

  if (endpoint === "/images/generations") {
    const payload = Object.fromEntries(Object.entries(common).filter(([, v]) => v !== undefined && v !== null));
    const response = await fetch(`${imageApiBaseUrl()}${endpoint}`, {
      method: "POST",
      headers: { Authorization: `Bearer ${apiKey}`, "Content-Type": "application/json" },
      body: JSON.stringify(payload),
      signal,
    });
    const parsed = await parseImageApiResponse(response, signal);
    return { images: parsed.images, details: { backend: "image_api", endpoint, model, size, quality, usage: parsed.usage } };
  }

  const inputImages = await Promise.all((params.images ?? []).map((p) => readInputImage(resolveCwd(ctx.cwd, p))));
  if (inputImages.length === 0) throw new Error("images must contain at least one source image for edits.");
  if (inputImages.length > 16) throw new Error("Image API edits support up to 16 input images.");

  const form = new FormData();
  for (const [key, value] of Object.entries(common)) appendFormValue(form, key, value);
  appendFormValue(form, "input_fidelity", params.inputFidelity);

  if (inputImages.length === 1) {
    const image = inputImages[0];
    form.append("image", new Blob([image.data], { type: image.mimeType }), basename(image.path));
  } else {
    for (const image of inputImages) {
      form.append("image[]", new Blob([image.data], { type: image.mimeType }), basename(image.path));
    }
  }

  if (params.mask) {
    const maskPath = resolveCwd(ctx.cwd, params.mask);
    const mask = await readInputImage(maskPath);
    if (extname(mask.path).toLowerCase() !== ".png") throw new Error("mask must be a PNG file with an alpha channel.");
    form.append("mask", new Blob([mask.data], { type: "image/png" }), basename(mask.path));
  }

  const response = await fetch(`${imageApiBaseUrl()}${endpoint}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${apiKey}` },
    body: form,
    signal,
  });
  const parsed = await parseImageApiResponse(response, signal);
  return { images: parsed.images, details: { backend: "image_api", endpoint, model, size, quality, usage: parsed.usage } };
}

async function saveImages(cwd: string, params: ImageGenInput, outputFormat: string, images: GeneratedImage[]): Promise<string[]> {
  const paths = buildOutputPaths(cwd, params, outputFormat, images.length);
  await ensureWritable(paths, params.overwrite);
  for (let i = 0; i < images.length; i++) {
    const path = paths[i];
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, Buffer.from(images[i].b64, "base64"));
  }
  return paths;
}

export default function imageGenerationExtension(pi: ExtensionAPI) {
  try {
    pi.registerTool({
    name: "image_gen",
    label: "image_gen",
    description:
      "Generate or edit images dynamically. Auto mode prefers the native Codex image_generation backend when available, otherwise uses the OpenAI Images API. Saves outputs to files and returns the image(s).",
    promptSnippet: "Generate or edit images using Codex native image_generation or OpenAI GPT Image models",
    promptGuidelines: [
      "Use image_gen when the user asks to create, generate, draw, render, or edit an image. Do not claim you cannot make images if image_gen is available.",
      "For ordinary image requests, call image_gen with a rich prompt and omit backend/model so it can choose the best available backend dynamically.",
      "image_gen Image API fallback defaults to gpt-image-2, size auto, quality medium, and png output.",
      "If the user explicitly asks for API/model controls or multiple images, use image_gen with backend=image_api and the requested controls; this requires an OpenAI API key.",
      "gpt-image-2 does not support true transparent backgrounds or input_fidelity. Ask before switching to gpt-image-1.5 for true transparency.",
      "When the user asks you to pick a subject yourself, choose a concrete subject and call image_gen rather than asking a follow-up question.",
    ],
    parameters: imageGenSchema,
    async execute(_toolCallId, params: ImageGenInput, signal, onUpdate, ctx) {
      const outputFormat = normalizeOutputFormat(params.outputFormat);
      onUpdate?.({ content: [{ type: "text", text: "Preparing image generation request..." }] });

      const forceBackend = params.backend ?? "auto";
      const advancedControls = hasLowLevelApiControls(params);
      let result: { images: GeneratedImage[]; details: Partial<ImageGenDetails> } | undefined;
      const errors: string[] = [];

      if (forceBackend === "codex_native" || (forceBackend === "auto" && !advancedControls)) {
        try {
          onUpdate?.({ content: [{ type: "text", text: "Calling Codex native image_generation..." }] });
          result = await runCodexNative(params, ctx, signal);
        } catch (error) {
          errors.push(error instanceof Error ? error.message : String(error));
          if (forceBackend === "codex_native") throw error;
        }
      }

      if (!result) {
        try {
          onUpdate?.({ content: [{ type: "text", text: "Calling OpenAI Images API..." }] });
          result = await runImageApi(params, ctx, signal);
        } catch (error) {
          errors.push(error instanceof Error ? error.message : String(error));
          throw new Error(`image_gen failed. ${errors.join(" | ")}`);
        }
      }

      const outputPaths = await saveImages(ctx.cwd, params, outputFormat, result.images);
      const details: ImageGenDetails = {
        backend: result.details.backend as "codex_native" | "image_api",
        endpoint: result.details.endpoint,
        model: result.details.model,
        prompt: params.prompt,
        revisedPrompt: result.details.revisedPrompt ?? result.images.find((img) => img.revisedPrompt)?.revisedPrompt,
        outputPaths,
        outputFormat,
        size: result.details.size,
        quality: result.details.quality,
        usage: result.details.usage,
      };

      const text = [
        `${details.backend === "codex_native" ? "Generated via Codex native image_generation" : "Generated via OpenAI Images API"}.`,
        `Saved ${outputPaths.length} image(s):`,
        ...outputPaths.map((path) => `- ${path}`),
        details.revisedPrompt ? `Revised prompt: ${details.revisedPrompt}` : undefined,
      ]
        .filter(Boolean)
        .join("\n");

      return {
        content: [
          { type: "text", text },
          ...result.images.map((image) => ({ type: "image" as const, mimeType: mimeForFormat(outputFormat), data: image.b64 })),
        ],
        details,
      };
    },
    renderCall(args, theme, context) {
      const text = (context.lastComponent as Text | undefined) ?? new Text("", 0, 0);
      const prompt = typeof args?.prompt === "string" ? args.prompt : "...";
      const backend = typeof args?.backend === "string" ? args.backend : "auto";
      const images = Array.isArray(args?.images) ? ` + ${args.images.length} input image(s)` : "";
      text.setText(`${theme.fg("toolTitle", theme.bold("image_gen"))} ${theme.fg("accent", backend)} ${theme.fg("muted", prompt.slice(0, 120))}${theme.fg("dim", images)}`);
      return text;
    },
    renderResult(result, _options, theme, context) {
      const text = (context.lastComponent as Text | undefined) ?? new Text("", 0, 0);
      const details = (result as { details?: ImageGenDetails }).details;
      if (!details) {
        text.setText(theme.fg("toolOutput", "Image generation finished."));
      } else {
        text.setText(
          [
            theme.fg("success", `✓ ${details.backend} wrote ${details.outputPaths.length} image(s)`),
            ...details.outputPaths.map((path) => theme.fg("accent", path)),
            !context.showImages ? theme.fg("dim", "Enable terminal images to preview inline.") : undefined,
          ]
            .filter(Boolean)
            .join("\n"),
        );
      }
      return text;
    },
    });
  } catch (error) {
    if (!String(error instanceof Error ? error.message : error).includes('Tool "image_gen" conflicts')) {
      throw error;
    }
  }
}
