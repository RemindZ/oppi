#!/usr/bin/env node
import { mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const refs = [
  ["pi", "https://github.com/badlogic/pi-mono.git"],
  ["oh-my-pi", "https://github.com/can1357/oh-my-pi.git"],
  ["codex-cli", "https://github.com/openai/codex.git"],
  ["claude-mem", "https://github.com/thedotmack/claude-mem.git"],
];

mkdirSync("reference", { recursive: true });
for (const [name, url] of refs) {
  const dir = join("reference", name);
  if (existsSync(dir)) {
    console.log(`${name}: already exists`);
    continue;
  }
  console.log(`${name}: cloning ${url}`);
  const result = spawnSync("git", ["clone", url, dir], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
