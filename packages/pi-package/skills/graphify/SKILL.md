---
name: graphify
description: Use Graphify codebase knowledge graphs for architecture, dependency, cross-module, repo-wide navigation, impact analysis, or "why is this connected?" questions. Prefer graph artifacts before broad raw search when .graphify/ or graphify-out/ exists.
license: MIT-compatible integration guidance; Graphify package is external and user-installed.
compatibility: Requires user-approved Graphify installation (usually npm package graphifyy exposing graphify) and generated .graphify/ or graphify-out/ artifacts.
---

# Graphify Codebase Graph Context

Use this skill when codebase structure matters: architecture maps, dependency paths, blast-radius review, cross-module navigation, ownership boundaries, god nodes, unexpected couplings, or questions about why parts of the repository are connected.

Graphify complements Hoppi:

- Hoppi stores user preferences, product decisions, and durable task memory.
- Graphify stores repository structure, graph relationships, communities, and generated graph artifacts.

## Safety and setup

Do not install or run Graphify automatically. If the CLI or graph artifacts are missing, propose a user-approved setup step instead.

Preferred setup, after user approval:

```bash
npm install -g graphifyy
graphify install
```

The npm package is currently `graphifyy`; the CLI command is `graphify`. Its install command prints a mutation preview before changing assistant instruction files, hooks, MCP, or plugin config.

## Status checks

Before relying on Graphify, inspect lightweight project state:

- `.graphify/GRAPH_REPORT.md`, `.graphify/wiki/index.md`, `.graphify/graph.json`
- legacy `graphify-out/GRAPH_REPORT.md`, `graphify-out/wiki/index.md`, `graphify-out/graph.json`
- `.graphify/needs_update`, `.graphify/scope.json`, `.graphify/branch.json`, `.graphify/worktree.json`
- `graphify.yaml` or `graphify.yml`

If `.graphify/needs_update` exists or relevant files changed since the graph was built, tell the user the graph may be stale before using it.

## Artifact preference

When graph artifacts exist, prefer them in this order before broad raw search:

1. `.graphify/wiki/index.md` or `graphify-out/wiki/index.md`
2. `.graphify/GRAPH_REPORT.md` or `graphify-out/GRAPH_REPORT.md`
3. `.graphify/graph.json` or `graphify-out/graph.json`

Use raw file search after graph context identifies likely modules, edges, or communities.

## Useful commands

Ask before commands that install packages, mutate config, rebuild large graphs, or use network/model resources.

```bash
graphify scope inspect . --scope auto
graphify detect .
graphify update .
graphify query "show the auth flow"
graphify path "Frontend" "Database"
graphify explain "DigestAuth"
graphify review-analysis
graphify portable-check .graphify
```

Use `--scope auto` or `scope inspect` first for normal code repos. Use `--all` only when the user wants a full recursive knowledge/document scan.

## Response pattern

When using Graphify, report:

- graph source and freshness (`.graphify`, `graphify-out`, stale/missing)
- key communities/nodes/edges that informed the answer
- which raw files you checked after graph context
- whether any conclusions are extracted, inferred, or uncertain
