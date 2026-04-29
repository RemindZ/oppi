# @oppiai/cli

Thin `oppi` CLI wrapper for the OPPi Pi package.

Current version: **0.2.0**.

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
- passes ordinary Pi flags and messages through unchanged
- provides `oppi doctor [--json]`
- provides safe Hoppi bridge commands: `oppi mem status|setup|dashboard [--json]`

Use `--with-pi-extensions` to allow normal Pi extension discovery in addition to OPPi.
