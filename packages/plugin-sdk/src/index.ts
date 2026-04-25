export type OppiPluginManifest = {
  extensions?: string[];
  skills?: string[];
  prompts?: string[];
  themes?: string[];
  features?: Record<string, unknown>;
};
