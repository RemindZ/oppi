#!/usr/bin/env node
import process from "node:process";

const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
const repo = process.env.OPPI_FEEDBACK_REPO || "RemindZ/oppi";

if (!token) {
  console.error("Set GITHUB_TOKEN or GH_TOKEN with repo issues/metadata access.");
  process.exit(1);
}

const labels = [
  ["oppi-intake", "06b6d4", "Created through OPPi feedback intake."],
  ["from-oppi", "67e8f9", "Submitted by OPPi tooling."],
  ["needs-triage", "facc15", "Needs maintainer triage."],
  ["bug", "ef4444", "Something is not working."],
  ["enhancement", "a78bfa", "New feature or improvement."],
];

async function github(path, init = {}) {
  const response = await fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      "user-agent": "oppi-feedback-github-setup",
      "x-github-api-version": "2022-11-28",
      ...init.headers,
    },
  });
  const text = await response.text();
  let body;
  try { body = text ? JSON.parse(text) : undefined; } catch { body = text; }
  return { response, body };
}

for (const [name, color, description] of labels) {
  const encoded = encodeURIComponent(name);
  const current = await github(`/repos/${repo}/labels/${encoded}`);
  const payload = JSON.stringify({ name, color, description });
  if (current.response.status === 200) {
    const updated = await github(`/repos/${repo}/labels/${encoded}`, { method: "PATCH", body: payload });
    console.log(`${updated.response.ok ? "updated" : "failed"} ${name}: ${updated.response.status}`);
  } else if (current.response.status === 404) {
    const created = await github(`/repos/${repo}/labels`, { method: "POST", body: payload });
    console.log(`${created.response.ok ? "created" : "failed"} ${name}: ${created.response.status}`);
  } else {
    console.error(`failed checking ${name}: ${current.response.status}`, current.body);
    process.exit(1);
  }
}
