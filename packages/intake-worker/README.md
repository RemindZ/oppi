# @oppiai/intake-worker

Cloudflare Worker for OPPi feedback intake.

It accepts:

```text
POST /v1/intake/bug-report
POST /v1/intake/feature-request
GET  /health
```

and creates GitHub issues in allowlisted repositories using Worker-held GitHub credentials. The public OPPi endpoint intentionally does not require a bundled client token; any token shipped in the package would be public and should not be treated as a secret.

## Required Cloudflare secrets

Preferred production setup uses a GitHub App:

```bash
wrangler secret put GITHUB_APP_ID
wrangler secret put GITHUB_INSTALLATION_ID
wrangler secret put GITHUB_APP_PRIVATE_KEY
wrangler secret put INTAKE_SIGNING_SECRET
```

Early setup can use a fine-grained GitHub token instead of GitHub App credentials:

```bash
wrangler secret put GITHUB_TOKEN
wrangler secret put INTAKE_SIGNING_SECRET
```

Optional for private/internal deployments only:

```bash
wrangler secret put INTAKE_CLIENT_TOKEN
```

When `INTAKE_CLIENT_TOKEN` is set, clients must send the same value via `x-oppi-intake-token`. Do not set this for the public OPPi endpoint unless you also own client distribution, because a bundled token is extractable from the package.

`GITHUB_APP_PRIVATE_KEY` must be PKCS#8 PEM with `-----BEGIN PRIVATE KEY-----`. If GitHub gives you `-----BEGIN RSA PRIVATE KEY-----`, convert it first:

```bash
openssl pkcs8 -topk8 -inform PEM -outform PEM -nocrypt -in github-app.pem -out github-app.pkcs8.pem
```

The GitHub App only needs:

```text
Repository metadata: read
Issues: read/write
```

Install it only on the OPPi repository.

## Public vars

Configured in `wrangler.toml`:

```toml
DEFAULT_REPO = "RemindZ/oppi"
ALLOWED_REPOS = "RemindZ/oppi"
RATE_LIMIT_MAX_PER_HOUR = "8"
```

Rate limiting uses the `RATE_LIMIT` Workers KV binding configured in `wrangler.toml`.

## Deploy

```bash
cd packages/intake-worker
wrangler deploy
```
