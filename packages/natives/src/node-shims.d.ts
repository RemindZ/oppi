declare const process: {
  env: Record<string, string | undefined>;
  platform: string;
  arch: string;
  cwd(): string;
};

declare module "node:assert/strict" {
  const assert: any;
  export default assert;
}

declare module "node:test" {
  const test: any;
  export default test;
}

declare module "node:child_process" {
  export function spawnSync(command: string, args?: readonly string[], options?: any): any;
}

declare module "node:fs" {
  export function existsSync(path: string): boolean;
  export function readdirSync(path: string, options?: any): any[];
  export function readFileSync(path: string, options?: any): any;
  export function statSync(path: string): { isDirectory(): boolean; isFile(): boolean; size: number };
  export function mkdtempSync(prefix: string): string;
  export function mkdirSync(path: string, options?: any): void;
  export function writeFileSync(path: string, data: string, encoding?: BufferEncoding): void;
}

declare module "node:module" {
  export function createRequire(url: string): any;
}

declare module "node:os" {
  export function tmpdir(): string;
}

declare module "node:path" {
  export function dirname(path: string): string;
  export function join(...paths: string[]): string;
  export function resolve(...paths: string[]): string;
}

declare module "node:perf_hooks" {
  export const performance: { now(): number };
}

declare module "node:url" {
  export function fileURLToPath(url: string | URL): string;
}

type BufferEncoding = "utf8" | "utf-8" | string;
