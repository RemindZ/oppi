export type OppiPluginCapability = "shell" | "network" | "native" | "filesystem" | "model" | (string & {});

export type OppiPluginPermissionDeclaration = {
  capability: OppiPluginCapability;
  reason?: string;
  optional?: boolean;
};

export type OppiPluginManifest = {
  name: string;
  version: string;
  description?: string;
  extensions?: string[];
  skills?: string[];
  prompts?: string[];
  themes?: string[];
  permissions?: OppiPluginPermissionDeclaration[];
  capabilities?: OppiPluginCapability[];
  license?: string;
};

export type OppiMarketplacePlugin = {
  name: string;
  source: string;
  version?: string;
  description?: string;
  license?: string;
  capabilities?: OppiPluginCapability[];
};

export type OppiMarketplaceCatalog = {
  name: string;
  description?: string;
  plugins: OppiMarketplacePlugin[];
};

export function defineOppiPlugin(manifest: OppiPluginManifest): OppiPluginManifest {
  return manifest;
}

export function defineOppiMarketplace(catalog: OppiMarketplaceCatalog): OppiMarketplaceCatalog {
  return catalog;
}
