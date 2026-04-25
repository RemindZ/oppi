type FeedbackType = "bug-report" | "feature-request";

type Env = {
  DEFAULT_REPO?: string;
  ALLOWED_REPOS?: string;
  GITHUB_APP_ID?: string;
  GITHUB_INSTALLATION_ID?: string;
  GITHUB_APP_PRIVATE_KEY?: string;
  /** Optional fallback for early setup. Prefer GitHub App credentials for production. */
  GITHUB_TOKEN?: string;
  INTAKE_SIGNING_SECRET?: string;
};

type IntakePayload = {
  repo?: string;
  type?: FeedbackType;
  title?: string;
  summary?: string;
  body?: string;
  labels?: string[];
};

const GITHUB_API = "https://api.github.com";
const MAX_PAYLOAD_BYTES = 64 * 1024;
const COMMON_LABELS = ["oppi-intake", "from-oppi", "needs-triage"];

function json(data: unknown, init: ResponseInit = {}): Response {
  return new Response(JSON.stringify(data, null, 2), {
    ...init,
    headers: {
      "content-type": "application/json; charset=utf-8",
      "access-control-allow-origin": "*",
      "access-control-allow-methods": "GET,POST,OPTIONS",
      "access-control-allow-headers": "content-type",
      ...init.headers,
    },
  });
}

function error(message: string, status = 400): Response {
  return json({ ok: false, error: message }, { status });
}

function base64Url(input: ArrayBuffer | Uint8Array | string): string {
  let bytes: Uint8Array;
  if (typeof input === "string") {
    bytes = new TextEncoder().encode(input);
  } else if (input instanceof Uint8Array) {
    bytes = input;
  } else {
    bytes = new Uint8Array(input);
  }

  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function pemToArrayBuffer(pem: string): ArrayBuffer {
  const normalized = pem.replace(/\\n/g, "\n");
  if (normalized.includes("BEGIN RSA PRIVATE KEY")) {
    throw new Error("GITHUB_APP_PRIVATE_KEY must be PKCS#8 PEM (BEGIN PRIVATE KEY). Convert RSA keys with: openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in github-app.pem -out github-app.pkcs8.pem");
  }
  const body = normalized
    .replace(/-----BEGIN PRIVATE KEY-----/g, "")
    .replace(/-----END PRIVATE KEY-----/g, "")
    .replace(/\s+/g, "");
  const binary = atob(body);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

async function signJwt(env: Env): Promise<string> {
  if (!env.GITHUB_APP_ID || !env.GITHUB_APP_PRIVATE_KEY) {
    throw new Error("worker is missing GitHub App configuration");
  }

  const now = Math.floor(Date.now() / 1000);
  const header = { alg: "RS256", typ: "JWT" };
  const payload = {
    iat: now - 60,
    exp: now + 9 * 60,
    iss: env.GITHUB_APP_ID,
  };

  const signingInput = `${base64Url(JSON.stringify(header))}.${base64Url(JSON.stringify(payload))}`;
  const key = await crypto.subtle.importKey(
    "pkcs8",
    pemToArrayBuffer(env.GITHUB_APP_PRIVATE_KEY),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", key, new TextEncoder().encode(signingInput));
  return `${signingInput}.${base64Url(signature)}`;
}

async function githubFetch(path: string, init: RequestInit & { token: string }): Promise<Response> {
  const { token, headers, ...rest } = init;
  return fetch(`${GITHUB_API}${path}`, {
    ...rest,
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      "user-agent": "oppi-intake-worker/0.0.0",
      "x-github-api-version": "2022-11-28",
      ...headers,
    },
  });
}

async function installationToken(env: Env): Promise<string> {
  if (env.GITHUB_TOKEN) return env.GITHUB_TOKEN;

  if (!env.GITHUB_INSTALLATION_ID) {
    throw new Error("worker is missing GITHUB_INSTALLATION_ID or fallback GITHUB_TOKEN");
  }

  const jwt = await signJwt(env);
  const response = await githubFetch(`/app/installations/${env.GITHUB_INSTALLATION_ID}/access_tokens`, {
    method: "POST",
    token: jwt,
    body: JSON.stringify({}),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`failed to create GitHub installation token (${response.status}): ${text}`);
  }

  const data = (await response.json()) as { token?: string };
  if (!data.token) throw new Error("GitHub installation token response was missing token");
  return data.token;
}

function allowedRepos(env: Env): Set<string> {
  return new Set(
    (env.ALLOWED_REPOS || env.DEFAULT_REPO || "")
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean),
  );
}

function normalizeRepo(env: Env, repo: string | undefined): string {
  const value = (repo || env.DEFAULT_REPO || "").trim();
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(value)) throw new Error("invalid repo; expected owner/name");
  const allowed = allowedRepos(env);
  if (allowed.size > 0 && !allowed.has(value)) throw new Error(`repo is not allowed: ${value}`);
  return value;
}

function normalizeType(pathname: string, bodyType: unknown): FeedbackType {
  const fromPath = pathname.endsWith("/bug-report")
    ? "bug-report"
    : pathname.endsWith("/feature-request")
      ? "feature-request"
      : undefined;
  const fromBody = bodyType === "bug-report" || bodyType === "feature-request" ? bodyType : undefined;
  const type = fromPath || fromBody;
  if (!type) throw new Error("invalid intake path; use /v1/intake/bug-report or /v1/intake/feature-request");
  if (fromPath && fromBody && fromPath !== fromBody) throw new Error("path type and body type do not match");
  return type;
}

function sanitizeText(value: unknown, max: number): string {
  const text = typeof value === "string" ? value : "";
  return text
    .replace(/(authorization\s*[:=]\s*bearer\s+)[^\s\n]+/gi, "$1<redacted>")
    .replace(/((?:api[_-]?key|token|password|secret|client[_-]?secret)\s*[:=]\s*)[^\s\n]+/gi, "$1<redacted>")
    .replace(/(gh[pousr]_[A-Za-z0-9_]+)/g, "<redacted-github-token>")
    .replace(/(sk-[A-Za-z0-9_-]{16,})/g, "<redacted-openai-key>")
    .replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g, "<redacted-private-key>")
    .slice(0, max)
    .trim();
}

async function signMarker(env: Env, repo: string, type: FeedbackType, title: string): Promise<string> {
  if (!env.INTAKE_SIGNING_SECRET) return "unsigned";
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(env.INTAKE_SIGNING_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${repo}\n${type}\n${title}`));
  return base64Url(signature);
}

async function createIssue(env: Env, payload: IntakePayload, requestType: FeedbackType): Promise<Response> {
  const repo = normalizeRepo(env, payload.repo);
  const [owner, name] = repo.split("/");
  const type = requestType;
  const title = sanitizeText(payload.title || payload.summary, 160) || (type === "bug-report" ? "[Bug] OPPi bug report" : "[Feature] OPPi feature request");
  const body = sanitizeText(payload.body, 60_000);
  if (!body) throw new Error("issue body is required");

  const typeLabels = type === "bug-report" ? ["bug"] : ["enhancement"];
  const requestedLabels = Array.isArray(payload.labels) ? payload.labels.filter((label): label is string => typeof label === "string") : [];
  const labels = Array.from(new Set([...COMMON_LABELS, ...typeLabels, ...requestedLabels].map((label) => label.trim()).filter(Boolean)));
  const signature = await signMarker(env, repo, type, title);
  const marker = `<!-- oppi-intake:v1 repo=${repo} type=${type} sig=${signature} -->`;
  const issueBody = `${marker}\n${body}`;

  const token = await installationToken(env);
  const response = await githubFetch(`/repos/${owner}/${name}/issues`, {
    method: "POST",
    token,
    body: JSON.stringify({ title, body: issueBody, labels }),
  });

  const text = await response.text();
  let data: Record<string, unknown> = {};
  try {
    data = text ? (JSON.parse(text) as Record<string, unknown>) : {};
  } catch {
    data = { raw: text };
  }

  if (!response.ok) {
    return json({ ok: false, error: "GitHub issue creation failed", status: response.status, github: data }, { status: 502 });
  }

  return json({ ok: true, repo, type, issueUrl: data.html_url, issueNumber: data.number });
}

async function readJsonPayload(request: Request): Promise<IntakePayload> {
  const length = Number(request.headers.get("content-length") || "0");
  if (Number.isFinite(length) && length > MAX_PAYLOAD_BYTES) throw new Error("payload is too large");
  const text = await request.text();
  if (text.length > MAX_PAYLOAD_BYTES) throw new Error("payload is too large");
  return JSON.parse(text) as IntakePayload;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    if (request.method === "OPTIONS") return json({ ok: true });

    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/health") {
      return json({ ok: true, service: "oppi-intake", defaultRepo: env.DEFAULT_REPO || null });
    }

    if (request.method !== "POST" || !url.pathname.startsWith("/v1/intake/")) {
      return error("not found", 404);
    }

    try {
      const payload = await readJsonPayload(request);
      const type = normalizeType(url.pathname, payload.type);
      return await createIssue(env, payload, type);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      const status = message.includes("not allowed") ? 403 : message.includes("too large") ? 413 : 400;
      return error(message, status);
    }
  },
};
