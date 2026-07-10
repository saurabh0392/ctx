# ctx-dist

Token-gated distribution for ctx. Alpha users install with one command and no repo access:

```bash
curl -fsSL <endpoint>/install.sh | CTX_TOKEN=<their-token> sh
```

## How it works

- A private S3 bucket holds the binaries, checksums, `manifest/latest.json`, and `install.sh`. Nothing
  is public.
- A Lambda (Function URL) serves `install.sh` openly (it holds no secrets) and, on `POST {token,
  target}`, validates the token against an SSM allowlist and returns a 5-minute presigned download URL
  plus the sha256. A leaked URL is one binary for a few minutes, not the bucket.
- `install.sh` verifies the checksum, installs to `~/.local/bin`, clears the macOS quarantine (interim
  trust bridge until Developer ID notarization), and runs `ctx setup`.

## Live resources (us-east-1, account 553239736682)

- Endpoint: set at deploy, see the `CtxDist.InstallUrl` output.
- Bucket: `CtxDist.BucketName` output.
- Token allowlist: SSM SecureString `/ctx/dist/alpha-tokens`.

## Operate

```bash
# deploy / update the stack
cd services/ctx-dist && npm install && npx cdk deploy

# mint a token for a user (printed once, give it to them)
./scripts/dist-token.sh add "alice@example.com"
./scripts/dist-token.sh list
./scripts/dist-token.sh revoke <token>

# publish a build by hand (CI does all targets on release)
BUCKET=<bucket> ./scripts/dist-publish.sh aarch64-apple-darwin target/release/ctx 0.4.0
```

## CI publishing

`release.yml` publishes every target to the bucket and assembles the manifest on each tagged release.
It is gated on three repo secrets, and skips cleanly when they are absent (only the GitHub Release is
produced):

- `CTX_DIST_BUCKET`, `CTX_DIST_AWS_ACCESS_KEY_ID`, `CTX_DIST_AWS_SECRET_ACCESS_KEY`

## Not done yet (see docs/alpha-distribution-plan.md)

- Real trust: Apple Developer ID notarization (macOS) and Authenticode (Windows). Until then the
  installer uses the quarantine and "Run anyway" bridges.
- Windows: `install.ps1` and the first-class Windows build.
