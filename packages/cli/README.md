# @oppi/cli

Thin `oppi` CLI wrapper for the OPPi Pi package.

Current version: **0.2.0**.

## Install

After the first public npm publish:

```bash
npm install -g @oppi/cli
oppi doctor
oppi
```

Until then, use the repository build:

```bash
git clone https://github.com/RemindZ/oppi.git
cd oppi
pnpm install
pnpm --filter @oppi/cli build
node packages/cli/dist/main.js doctor
```

## Development

```bash
pnpm --filter @oppi/cli check
pnpm --filter @oppi/cli build
pnpm --filter @oppi/cli test
```

After build:

```bash
node packages/cli/dist/main.js --version
node packages/cli/dist/main.js --help
node packages/cli/dist/main.js doctor
node packages/cli/dist/main.js -p "Reply ok"
pnpm --filter @oppi/cli start doctor
```

## Behavior

- resolves the Pi CLI from `@mariozechner/pi-coding-agent` or `OPPI_PI_CLI`
- resolves the OPPi Pi package from `@oppi/pi-package`, the monorepo layout, or `OPPI_PI_PACKAGE`
- launches Pi as `pi --no-extensions -e <oppi-pi-package> ...` by default
- uses `~/.oppi/agent` for Pi/OPPi settings and sessions by default
- honors `OPPI_AGENT_DIR` and `--agent-dir <dir>`
- passes ordinary Pi flags and messages through unchanged
- provides `oppi doctor [--json]`
- provides safe Hoppi bridge commands: `oppi mem status|setup|dashboard [--json]`

Use `--with-pi-extensions` to allow normal Pi extension discovery in addition to OPPi.
