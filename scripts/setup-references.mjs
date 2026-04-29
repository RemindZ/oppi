#!/usr/bin/env node
import { mkdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const refs = [
  ["pi-mono", "https://github.com/badlogic/pi-mono.git"],
  ["oh-my-pi", "https://github.com/can1357/oh-my-pi.git"],
  ["oh-my-pi-plugins", "https://github.com/RemindZ/oh-my-pi-plugins.git"],
  ["codex", "https://github.com/openai/codex.git"],
  ["caveman", "https://github.com/juliusbrussee/caveman.git"],
];

mkdirSync(".reference", { recursive: true });
for (const [name, url] of refs) {
  const dir = join(".reference", name);
  if (existsSync(dir)) {
    console.log(`${name}: already exists`);
    continue;
  }
  console.log(`${name}: cloning ${url}`);
  const result = spawnSync("git", ["clone", url, dir], { stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
