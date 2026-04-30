import { spawnSync } from "node:child_process";
import {
  antigravityOAuthProvider,
  geminiCliOAuthProvider,
  githubCopilotOAuthProvider,
  openaiCodexOAuthProvider,
  type OAuthCredentials,
  type OAuthLoginCallbacks,
  type OAuthProviderInterface,
  type OAuthPrompt,
} from "@mariozechner/pi-ai/oauth";
import { AuthStorage, type ExtensionAPI } from "@mariozechner/pi-coding-agent";

const DISABLED_OAUTH_PROVIDERS = new Set(["anthropic"]);
const AUTH_STORAGE_PATCHED_KEY = Symbol.for("oppi.oauthSafety.authStoragePatched");

const WRAPPED_PROVIDERS: OAuthProviderInterface[] = [
  githubCopilotOAuthProvider,
  geminiCliOAuthProvider,
  antigravityOAuthProvider,
  openaiCodexOAuthProvider,
];

function sanitizeManualOAuthInput(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return trimmed;

  // OAuth URLs and codes cannot contain raw whitespace. Terminal selection across
  // wrapped long URLs can insert spaces/newlines, which then breaks state checks.
  const looksLikeOAuthRedirect = /^https?:\/\//i.test(trimmed)
    || /(?:^|[?&#])(?:code|state)=/.test(trimmed)
    || /[\r\n\t\u00a0]/.test(input);

  return looksLikeOAuthRedirect ? trimmed.replace(/[\s\u00a0]+/g, "") : trimmed;
}

function runClipboardCommand(command: string, args: string[], text: string): boolean {
  try {
    const result = spawnSync(command, args, {
      input: text,
      encoding: "utf8",
      stdio: ["pipe", "ignore", "ignore"],
      windowsHide: true,
    });
    return result.status === 0;
  } catch {
    return false;
  }
}

function copyTextToClipboard(text: string): boolean {
  if (process.platform === "win32") {
    return runClipboardCommand("powershell.exe", ["-NoProfile", "-Command", "$input | Set-Clipboard"], text)
      || runClipboardCommand("clip.exe", [], text);
  }

  if (process.platform === "darwin") {
    return runClipboardCommand("pbcopy", [], text);
  }

  return runClipboardCommand("wl-copy", [], text)
    || runClipboardCommand("xclip", ["-selection", "clipboard"], text)
    || runClipboardCommand("xsel", ["--clipboard", "--input"], text);
}

function appendInstruction(existing: string | undefined, addition: string): string {
  return existing ? `${existing} ${addition}` : addition;
}

function wrapCallbacks(callbacks: OAuthLoginCallbacks): OAuthLoginCallbacks {
  return {
    ...callbacks,
    onAuth: (info) => {
      const copied = copyTextToClipboard(info.url);
      const copyNote = copied
        ? "OPPi copied the exact login URL to your clipboard. If no browser opens, paste from clipboard instead of copying the wrapped terminal text."
        : "If no browser opens, be careful copying the displayed URL: remove any spaces or newlines inserted by terminal wrapping.";
      callbacks.onAuth({
        ...info,
        instructions: appendInstruction(info.instructions, copyNote),
      });
    },
    onPrompt: async (prompt: OAuthPrompt) => sanitizeManualOAuthInput(await callbacks.onPrompt(prompt)),
    onManualCodeInput: callbacks.onManualCodeInput
      ? async () => sanitizeManualOAuthInput(await callbacks.onManualCodeInput!())
      : undefined,
  };
}

function wrapProvider(provider: OAuthProviderInterface): Omit<OAuthProviderInterface, "id"> {
  return {
    name: provider.name,
    usesCallbackServer: provider.usesCallbackServer,
    async login(callbacks: OAuthLoginCallbacks): Promise<OAuthCredentials> {
      return provider.login(wrapCallbacks(callbacks));
    },
    async refreshToken(credentials: OAuthCredentials): Promise<OAuthCredentials> {
      return provider.refreshToken(credentials);
    },
    getApiKey(credentials: OAuthCredentials): string {
      return provider.getApiKey(credentials);
    },
    modifyModels: provider.modifyModels
      ? (models, credentials) => provider.modifyModels!(models, credentials)
      : undefined,
  } as Omit<OAuthProviderInterface, "id">;
}

function installAuthStoragePolicyPatch(): void {
  const globalStore = globalThis as Record<symbol, boolean | undefined>;
  if (globalStore[AUTH_STORAGE_PATCHED_KEY]) return;
  globalStore[AUTH_STORAGE_PATCHED_KEY] = true;

  const proto = AuthStorage.prototype as AuthStorage & Record<string, any>;
  const originalGetOAuthProviders = proto.getOAuthProviders;
  const originalLogin = proto.login;

  proto.getOAuthProviders = function getOAuthProvidersWithoutUnsupportedNativeSignIns(this: AuthStorage) {
    return originalGetOAuthProviders.call(this).filter((provider: OAuthProviderInterface) => !DISABLED_OAUTH_PROVIDERS.has(provider.id));
  };

  proto.login = async function loginWithOppiPolicy(this: AuthStorage, providerId: string, callbacks: OAuthLoginCallbacks) {
    if (DISABLED_OAUTH_PROVIDERS.has(providerId)) {
      throw new Error("Native Anthropic subscription sign-in is disabled in OPPi. Use an Anthropic API key, or OPPi's throughput bridge once it is available in a later stage.");
    }
    return originalLogin.call(this, providerId, callbacks);
  };
}

export default function oauthSafety(pi: ExtensionAPI) {
  installAuthStoragePolicyPatch();

  for (const provider of WRAPPED_PROVIDERS) {
    // registerProvider's public type omits usesCallbackServer, but the OAuth
    // selector consumes it at runtime. Preserve it for callback-server providers.
    pi.registerProvider(provider.id, { oauth: wrapProvider(provider) as any });
  }
}
