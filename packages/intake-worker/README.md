# @oppi/intake-worker

Cloudflare Worker for OPPi feedback intake.

It accepts:

```text
POST /v1/intake/bug-report
POST /v1/intake/feature-request
GET  /health
```

and creates GitHub issues in allowlisted repositories using a GitHub App installation token.

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
```

## Deploy

```bash
cd packages/intake-worker
wrangler deploy
```
