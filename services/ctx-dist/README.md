# CTX beta distribution

Private S3 artifacts behind a public installer and capability-authenticated download endpoint.

## Flow

1. An operator mints a high-entropy one-time invite in the encrypted SSM roster.
2. `install.sh` posts the invite plus target.
3. The service derives a 16-hex participant ID, confirms the roster entry, and returns:
   - a five-minute presigned artifact URL;
   - the SHA-256 from `manifest/latest.json`; and
   - a signed 90-day `download-feedback` capability.
4. The installer passes the scoped capability to `ctx setup --beta`; the invite is never stored.
5. `ctx update` uses the capability. Every request rechecks expiry, scope, HMAC, and roster presence.

Removing a roster entry revokes both the invite and all capabilities derived from it.

## Required SSM parameters

- `/ctx/dist/alpha-tokens`: SecureString roster, one `token = label` entry per participant.
- `/ctx/beta/capability-secret`: SecureString HMAC key, at least 32 random characters, shared with report intake.

Create the capability secret once:

```bash
openssl rand -hex 32 | aws ssm put-parameter \
  --name /ctx/beta/capability-secret --type SecureString --value file:///dev/stdin
```

## Operate

```bash
cd services/ctx-dist
npm ci
npm run typecheck
npm run deploy -- -c feedbackEndpoint=<https-report-intake-url>

../../scripts/dist-token.sh add "team-a/lead"
../../scripts/dist-token.sh list                       # participant IDs + labels, never tokens
../../scripts/dist-token.sh revoke <participant-id>    # invalidates invite and capabilities
```

CI publishes target archives plus `manifest/latest.json`; manual publishing remains available:

```bash
BUCKET=<bucket> ../../scripts/dist-publish.sh aarch64-apple-darwin ../../target/release/ctx 0.5.0
```

Artifacts remain private and versioned. The macOS binary is not Developer ID signed/notarized in
this wave; Windows is experimental. Manual publishing records a version per target, so a host-only
smoke release does not invalidate the last known-good artifacts for other platforms.
