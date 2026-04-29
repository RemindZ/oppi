import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, normalize, sep } from "node:path";
import { pathToFileURL } from "node:url";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const PATCHED_KEY = Symbol.for("oppi.dockedUi.patched");
const DOCK_WIDGET_KEY = "oppi.docked-command-panel";

type InteractiveModeLike = {
  ui: any;
  editor: any;
  editorContainer: any;
  keybindings: any;
  extensionWidgetsAbove: Map<string, any>;
  extensionSelector?: any;
  extensionInput?: any;
  extensionEditor?: any;
  renderWidgets: () => void;
  showExtensionCustom: (factory: any, options?: any) => Promise<any>;
};

function resolvePiMainPath(): string {
  const require = createRequire(import.meta.url);
  try {
    return require.resolve("@mariozechner/pi-coding-agent");
  } catch {
    // Package extensions live outside Pi's global node_modules during local dogfooding.
    // Recover Pi's install root from the running CLI path or common npm global paths.
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

  throw new Error("Cannot resolve @mariozechner/pi-coding-agent internals for OPPi docked UI patch");
}

async function importPiInternal(relativePath: string): Promise<any> {
  const mainPath = resolvePiMainPath();
  return import(pathToFileURL(join(dirname(mainPath), relativePath)).href);
}

function removeDockedComponent(mode: InteractiveModeLike, dispose = true): void {
  const existing = mode.extensionWidgetsAbove?.get(DOCK_WIDGET_KEY);
  if (dispose) {
    try { existing?.dispose?.(); } catch { /* ignore */ }
  }
  mode.extensionWidgetsAbove?.delete(DOCK_WIDGET_KEY);
  mode.renderWidgets?.();
}

function showDockedComponent(mode: InteractiveModeLike, component: any): void {
  removeDockedComponent(mode);
  mode.extensionWidgetsAbove.set(DOCK_WIDGET_KEY, component);
  mode.renderWidgets();
  mode.ui.setFocus(component);
  mode.ui.requestRender();
}

function restoreEditorFocus(mode: InteractiveModeLike): void {
  mode.ui.setFocus(mode.editor);
  mode.ui.requestRender();
}

async function installDockedUiPatch(): Promise<void> {
  const globalStore = globalThis as Record<symbol, boolean | undefined>;
  if (globalStore[PATCHED_KEY]) return;
  globalStore[PATCHED_KEY] = true;

  const [interactive, selectorModule, inputModule, editorModule, themeModule] = await Promise.all([
    importPiInternal("modes/interactive/interactive-mode.js"),
    importPiInternal("modes/interactive/components/extension-selector.js"),
    importPiInternal("modes/interactive/components/extension-input.js"),
    importPiInternal("modes/interactive/components/extension-editor.js"),
    importPiInternal("modes/interactive/theme/theme.js"),
  ]);

  const InteractiveMode = interactive.InteractiveMode;
  const ExtensionSelectorComponent = selectorModule.ExtensionSelectorComponent;
  const ExtensionInputComponent = inputModule.ExtensionInputComponent;
  const ExtensionEditorComponent = editorModule.ExtensionEditorComponent;
  const theme = themeModule.theme;
  const proto = InteractiveMode?.prototype;
  if (!proto || proto.__oppiDockedUiPatched) return;

  const originalShowCustom = proto.showExtensionCustom;

  proto.showExtensionSelector = function showExtensionSelectorDocked(title: string, options: string[], opts?: any) {
    const mode = this as InteractiveModeLike;
    return new Promise<string | undefined>((resolve) => {
      if (opts?.signal?.aborted) {
        resolve(undefined);
        return;
      }
      const onAbort = () => {
        mode.hideExtensionSelector?.();
        resolve(undefined);
      };
      opts?.signal?.addEventListener("abort", onAbort, { once: true });
      mode.extensionSelector = new ExtensionSelectorComponent(title, options, (option: string) => {
        opts?.signal?.removeEventListener("abort", onAbort);
        mode.hideExtensionSelector?.();
        resolve(option);
      }, () => {
        opts?.signal?.removeEventListener("abort", onAbort);
        mode.hideExtensionSelector?.();
        resolve(undefined);
      }, { tui: mode.ui, timeout: opts?.timeout });
      showDockedComponent(mode, mode.extensionSelector);
    });
  };

  proto.hideExtensionSelector = function hideExtensionSelectorDocked() {
    const mode = this as InteractiveModeLike;
    try { mode.extensionSelector?.dispose?.(); } catch { /* ignore */ }
    mode.extensionSelector = undefined;
    removeDockedComponent(mode, false);
    restoreEditorFocus(mode);
  };

  proto.showExtensionInput = function showExtensionInputDocked(title: string, placeholder?: string, opts?: any) {
    const mode = this as InteractiveModeLike;
    return new Promise<string | undefined>((resolve) => {
      if (opts?.signal?.aborted) {
        resolve(undefined);
        return;
      }
      const onAbort = () => {
        mode.hideExtensionInput?.();
        resolve(undefined);
      };
      opts?.signal?.addEventListener("abort", onAbort, { once: true });
      mode.extensionInput = new ExtensionInputComponent(title, placeholder, (value: string) => {
        opts?.signal?.removeEventListener("abort", onAbort);
        mode.hideExtensionInput?.();
        resolve(value);
      }, () => {
        opts?.signal?.removeEventListener("abort", onAbort);
        mode.hideExtensionInput?.();
        resolve(undefined);
      }, { tui: mode.ui, timeout: opts?.timeout });
      showDockedComponent(mode, mode.extensionInput);
    });
  };

  proto.hideExtensionInput = function hideExtensionInputDocked() {
    const mode = this as InteractiveModeLike;
    try { mode.extensionInput?.dispose?.(); } catch { /* ignore */ }
    mode.extensionInput = undefined;
    removeDockedComponent(mode, false);
    restoreEditorFocus(mode);
  };

  proto.showExtensionEditor = function showExtensionEditorDocked(title: string, prefill?: string) {
    const mode = this as InteractiveModeLike;
    return new Promise<string | undefined>((resolve) => {
      mode.extensionEditor = new ExtensionEditorComponent(mode.ui, mode.keybindings, title, prefill, (value: string) => {
        mode.hideExtensionEditor?.();
        resolve(value);
      }, () => {
        mode.hideExtensionEditor?.();
        resolve(undefined);
      });
      showDockedComponent(mode, mode.extensionEditor);
    });
  };

  proto.hideExtensionEditor = function hideExtensionEditorDocked() {
    const mode = this as InteractiveModeLike;
    mode.extensionEditor = undefined;
    removeDockedComponent(mode);
    restoreEditorFocus(mode);
  };

  proto.showExtensionCustom = function showExtensionCustomDocked(factory: any, options?: any) {
    const mode = this as InteractiveModeLike;
    if (options?.overlay) return originalShowCustom.call(mode, factory, options);

    return new Promise((resolve, reject) => {
      let component: any;
      let closed = false;
      const close = (result: any) => {
        if (closed) return;
        closed = true;
        removeDockedComponent(mode, false);
        restoreEditorFocus(mode);
        resolve(result);
        try { component?.dispose?.(); } catch { /* ignore */ }
      };

      Promise.resolve(factory(mode.ui, theme, mode.keybindings, close))
        .then((created) => {
          if (closed) return;
          component = created;
          showDockedComponent(mode, component);
        })
        .catch((error) => {
          if (!closed) {
            removeDockedComponent(mode);
            restoreEditorFocus(mode);
          }
          reject(error);
        });
    });
  };

  proto.__oppiDockedUiPatched = true;
}

export default async function dockedUiExtension(_pi: ExtensionAPI) {
  await installDockedUiPatch();
}
