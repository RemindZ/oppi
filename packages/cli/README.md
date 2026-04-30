# @oppiai/cli

Thin `oppi` CLI wrapper for the OPPi Pi package.

Current version: **0.2.7**.

## Install

```bash
npm install -g @oppiai/cli
oppi doctor
oppi
```

Source install for local development:

```bash
git clone https://github.com/RemindZ/oppi.git
cd oppi
pnpm install
pnpm --filter @oppiai/cli build
node packages/cli/dist/main.js doctor
```

## Development

```bash
pnpm --filter @oppiai/cli check
pnpm --filter @oppiai/cli build
pnpm --filter @oppiai/cli test
```

After build:

```bash
node packages/cli/dist/main.js --version
node packages/cli/dist/main.js --help
node packages/cli/dist/main.js doctor
node packages/cli/dist/main.js -p "Reply ok"
pnpm --filter @oppiai/cli start doctor
```

## Behavior

- resolves the Pi CLI from `@mariozechner/pi-coding-agent` or `OPPI_PI_CLI`
- resolves the OPPi Pi package from `@oppiai/pi-package`, the monorepo layout, or `OPPI_PI_PACKAGE`
- launches Pi as `pi --no-extensions -e <oppi-pi-package> ...` by default
- uses `~/.oppi/agent` for Pi/OPPi settings and sessions by default
- honors `OPPI_AGENT_DIR` and `--agent-dir <dir>`
- checks npm at most daily before interactive launches and prints a one-line update notice when a newer `@oppiai/cli` is available (`OPPI_UPDATE_CHECK=0` disables it)
- passes ordinary Pi flags and messages through unchanged
- provides `oppi doctor [--json]`
- provides safe Hoppi bridge commands: `oppi mem status|install|setup|dashboard [--json]`
- installs optional Hoppi backend explicitly with `oppi mem install`; OPPi never installs `@oppiai/hoppi-memory` silently
- manages Stage 3 plugins with `oppi plugin list|add|install|enable|disable|remove|doctor`
- manages marketplace catalogs with `oppi marketplace list|add|remove`
- loads enabled OPPi plugins as extra Pi package sources (`-e <source>`) after the built-in OPPi package

Use `--with-pi-extensions` to allow normal Pi extension discovery in addition to OPPi.

## Plugins

Plugins are disabled by default when added. Enabling a plugin requires explicit trust with `--yes` because Pi packages can execute extension code.

```bash
oppi plugin add ./my-pi-package --local
oppi plugin doctor my-pi-package
oppi plugin enable my-pi-package --yes
oppi plugin list
```

Global plugin state lives in `~/.oppi/plugin-lock.json` (or `OPPI_HOME/plugin-lock.json`). Project plugins use `.oppi/plugins.json` when `--local` is passed.

Marketplace catalogs are JSON files or URLs shaped like:

```json
{
  "name": "local-dev",
  "plugins": [
    { "name": "demo", "source": "./plugins/demo", "description": "Demo Pi package" }
  ]
}
```

```bash
oppi marketplace add ./catalog.json
oppi plugin add demo --enable --yes
```

Claude-store compatibility is intentionally conservative for now. If a catalog entry looks Claude-specific (for example MCP server config, hooks, agents, or slash commands) but does not expose a Pi/OPPi package source, `oppi plugin add <name>` fails with a compatibility report and a copy/paste `oppi "Port ..."` handoff prompt so the agent can adapt it into a local Pi-compatible plugin instead of loading it blindly.
