# CTX beta report intake

Capability-authenticated intake for explicit dashboard issue reports and aggregate beta check-ins.
The Lambda Function URL is public at the network layer, but every action requires a valid unexpired
`feedback` capability whose participant remains in the encrypted roster.

## Actions

- `presign`: up to three PNG/JPEG/WebP/GIF screenshots, 5 MB each.
- `submit`: creates a private-repository GitHub issue with capped text, reviewed diagnostic JSON, and
  seven-day signed screenshot links.
- `checkin`: validates the exact `ctx.beta-checkin.v1` allowlist and stores it privately in S3.

Unknown fields are rejected. Check-ins cannot contain prompts, output, commands, paths, repos, tool
names, costs, source, or arbitrary JSON. Screenshots are private and expire after 30 days; aggregate
check-ins expire after 365 days.

## Required SSM parameters

- `/ctx/report-intake/github-token`: fine-grained PAT with Issues read/write on the private repo.
- `/ctx/dist/alpha-tokens`: the shared beta invite roster.
- `/ctx/beta/capability-secret`: the same HMAC secret used by `ctx-dist`.

## Operate

```bash
cd services/report-intake
npm ci
npm run typecheck
npm run deploy -- -c githubRepo=<owner/private-feedback-repo>
```

There is intentionally no default repository: deployment must name a private feedback repo. The S3
bucket is retained if the stack is deleted accidentally; its 30-day screenshot and 365-day check-in
lifecycle rules continue to govern normal retention.

Create the repository labels `beta-report`, `coherence-regression`, `bug`, and `enhancement` before
testing issue submission.

To analyze check-ins, sync them to a temporary directory outside the repository and run:

```bash
aws s3 sync s3://<bucket>/checkins /tmp/ctx-checkins
node ../../scripts/beta-summary.mjs /tmp/ctx-checkins
```

The summary output is aggregate-only. Never commit raw check-ins, screenshots, capabilities, tokens,
or participant mappings.
