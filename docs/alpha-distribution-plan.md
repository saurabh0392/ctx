# Getting ctx to alpha users without codebase access

Goal: a hand-picked alpha user on a fresh Mac runs one command, gets ctx installed with no security
warnings, wired into Claude Code, dashboard live. No source, no `gh`, no Rust, no org membership.

## Where we are today

- `scripts/install.sh` downloads the release binary with `gh` authenticated to the goshippo org, or a
  `GITHUB_TOKEN`. Both require repo access. That is the barrier to remove.
- `release.yml` builds three targets (arm64 mac, x86_64 mac, x86_64 linux), tars them, and publishes
  to a private GitHub Release. There is no Developer ID signing or notarization anywhere. The only
  signing is ad-hoc `codesign -s -`, which stops launchd from killing the binary but does not clear
  macOS Gatekeeper for a downloaded file. A user who downloads it hits the "unidentified developer"
  quarantine block.
- `ctx setup` already does the hard part: detects the host, writes `~/.ctx`, merges Claude Code hooks
  and MCP into `~/.claude/settings.json`, and starts the dashboard and ingest services. It has a
  `no_open` flag but no quiet mode. There is no self-update and no one-shot uninstall command.
- AWS is already wired for the report-intake feature (Lambda Function URL, S3, SSM SecureString). We
  can reuse that account and pattern for distribution.

## Security first

A `curl ... | sh` installer is a supply-chain trust vector, so the plan treats it as one:

- Serve everything over HTTPS (CloudFront TLS). Publish a SHA256 for every binary. The install script
  verifies the checksum before it runs or installs anything.
- Gate downloads behind a per-user alpha token, validated by a Lambda against an allowlist in SSM.
  The binary itself is never on a public URL. Tokens are revocable per user, so a leak is contained.
- Presigned download URLs are short-lived (5 minutes), so a copied URL expires fast.
- No secret ever ships in the binary or the script. The token lives only in the user's shell for the
  one install call.
- The quarantine-strip bridge below is a real trust compromise (it tells macOS to trust an unsigned
  binary). It is acceptable only for a small, known alpha and only until notarization lands. Call it
  out to users, and sunset it.

## Recommended architecture

### 1. Distribution: AWS S3 + Lambda token gate (reuse the report-intake account)

- Private S3 bucket holds `install.sh`, a `latest.json` manifest, and `ctx-<version>-<target>.tar.gz`
  plus `.sha256` for each target.
- A `ctx-install` Lambda Function URL validates the alpha token against `SSM /ctx/alpha-tokens`, then
  returns short-lived presigned GET URLs for the manifest and the caller's target tarball.
- `install.sh` is public and cacheable behind CloudFront (it holds no secrets). The binaries are not.
- One command for the user:

  ```bash
  curl -fsSL https://<cloudfront-domain>/install.sh | CTX_TOKEN=<their-token> sh
  ```

- What `install.sh` does, in order: detect OS and arch, call the Lambda with the token, get presigned
  URLs, download the tarball and its checksum, verify the checksum, install to `~/.local/bin` (no
  sudo), strip quarantine (interim, see below), run `ctx setup --quiet`, health-check the dashboard,
  print the URL.

Alternatives considered: a separate public dist repo (simplest, but the binary is then world-readable
and ungated), or a Homebrew tap (great UX and free updates, but still needs the binary hosted
somewhere and a tap repo). AWS wins here because it reuses infra we already run, keeps the binary
private and gated, and makes revocation trivial.

### 2. macOS trust: Developer ID signing and notarization

This is the single biggest lever for "just works on a new system." Without it, every user does an
`xattr` dance or right-click-open. With it, the binary opens silently.

- One-time: Apple Developer Program membership, then a "Developer ID Application" certificate.
- In `release.yml` on the macOS runners: `codesign --options runtime --timestamp --sign "Developer ID
  Application: <name>"`, then `xcrun notarytool submit --wait` with an App Store Connect API key in
  GitHub secrets, then `xcrun stapler staple`. The stapled ticket means it verifies even offline.
- Result: downloaded binary runs with zero Gatekeeper friction, and we drop the quarantine bridge.

Interim bridge until the certificate exists: `install.sh` runs `xattr -dr com.apple.quarantine` on the
installed binary. It works today, with the trust caveat above.

### 3. Make `ctx setup` opaque and easy

- Add `ctx setup --quiet`: a spinner, minimal output, and a single success line pointing at
  `http://127.0.0.1:8789`. `install.sh` calls it automatically, so install and setup are one step.
- Preflight: if Claude Code is not found, still install ctx and the dashboard, and print the one
  actionable line to add it. Never fail silently.
- Keep it idempotent and safe: back up `~/.claude/settings.json` before merging, never clobber a
  user's own rules. (Setup already does most of this.)
- Add `ctx update`: read `latest.json`, and if newer, re-download and swap the binary in place. Surface
  an "update available" banner on the dashboard so alpha users stay current without new instructions.
- Add `ctx teardown` (the skill exists; wire it as a subcommand): stop services, remove the ctx entries
  from settings, delete `~/.ctx`. Easy removal is part of trust.

## Rollout

- Phase 0, this week: stand up the S3 bucket, the token-gate Lambda, and CloudFront. Rework
  `install.sh` to be gh-free (token, presign, checksum, `~/.local/bin`, auto-setup, quarantine bridge).
  Add a release step that uploads the tarballs, checksums, and `latest.json` to S3. Ship to the first
  three to five friendly users. One command, no repo access.
- Phase 1, before scaling: Apple Developer ID signing and notarization in `release.yml`. Remove the
  quarantine bridge. Now it is frictionless and honestly trustworthy.
- Phase 2: `ctx setup --quiet`, `ctx update`, `ctx teardown`, and a short first-run message (what ctx
  does, that it is local, how to report) with a link to the demo video.
- Phase 3: operate the alpha. Issue and revoke per-user tokens from an SSM roster. No telemetry by
  design; feedback comes through the Report button already in the dashboard, which files GitHub issues.

## Decisions (locked)

- Channel: AWS S3 plus a token-gate Lambda, fronted by CloudFront. This deliberately decouples
  distribution from GitHub entirely, so the user-facing install never names a repo or an org. Note:
  the real repo is `saurabh0392/ctx` (private). The `goshippo/ctx` references in the README,
  `install.sh`, and `release.yml` are stale and wrong (they point at a repo that is not ours) and
  should be removed as part of this work, since they would break the current gh-based install anyway.
- Trust: no Apple account yet, so Phase 0 ships with the quarantine bridge on macOS and the equivalent
  "Run anyway" path on Windows. Real signing (Apple notarization, Windows Authenticode) comes later.
- Gating: per-user revocable tokens, allowlisted in SSM.
- Platforms: Apple Silicon Mac, Intel Mac, Linux x86_64, and Windows.

## The Windows reality

Mac and Linux are a packaging job, the binaries already build in `release.yml`. Windows is a port, not
a repackage. Today there is no Windows code at all: `host.rs` only branches on macOS and Linux, every
one of the 53 service hooks is launchd or systemd, there is no `.exe` handling, and `src/socket.rs`
uses unix domain sockets. So Windows needs real work before it can ship:

- Confirm the crate even compiles for `x86_64-pc-windows-msvc`, and replace anything unix-only (the
  unix socket in `socket.rs`, any hardcoded `/` paths, permissions calls).
- A background-service model: launchd and systemd do not exist on Windows. Use a Scheduled Task or a
  startup launcher for the dashboard and ingest, behind a new `#[cfg(windows)]` branch in the host
  and daemon code.
- Path and integration handling: `~/.ctx` and `~/.claude` become `%USERPROFILE%` paths (the `dirs`
  crate handles `home_dir`, but the settings-merge and hook paths need auditing), and we need to
  confirm how Claude Code on Windows loads hooks and MCP config.
- A PowerShell installer (`irm https://.../install.ps1 | iex`) that mirrors `install.sh`.
- Trust: unsigned `.exe` files trigger SmartScreen. The interim bridge is the user clicking "More info,
  then Run anyway." The real fix is an Authenticode certificate (OV or EV), separate from Apple.

Estimate: Windows is days to a couple of weeks of engineering, mostly the service model and the unix
assumptions. It should not block the Mac and Linux alpha.

## Status

Phase 0 is deployed and verified end to end. The `CtxDist` stack (`services/ctx-dist`) is live in
us-east-1: a private artifact bucket, a token-gate Lambda serving `install.sh` and presigned downloads,
and the `/ctx/dist/alpha-tokens` SSM allowlist. `install.sh` is rewritten to be org-agnostic and
token-based. A smoke test confirmed a valid token returns a presigned URL, the download checksum
matches, the binary runs, and a bad token gets 403. Left for a follow-up: add the three CI secrets so
`release.yml` auto-publishes all targets, and run one real install on a clean machine.

## Rollout (revised)

- Phase 0, this week, Mac and Linux only: S3 bucket, token-gate Lambda, CloudFront. Rework `install.sh`
  to be gh-free and org-agnostic (token, presign, checksum, `~/.local/bin`, auto `ctx setup`, quarantine
  bridge). Add a release step that uploads tarballs, checksums, and `latest.json` to S3. Ship to the
  first few users. One command, no repo access.
- Phase 1: `ctx setup --quiet`, `ctx update` (self-update from `latest.json`), `ctx teardown`, and a
  short first-run message linking the demo video.
- Phase 2, Windows port: compile for Windows, add the `#[cfg(windows)]` service and path handling,
  write `install.ps1`, ship with the "Run anyway" bridge.
- Phase 3, real trust: Apple Developer ID signing and notarization for macOS, Authenticode for Windows.
  Drop both bridges.
- Ongoing: issue and revoke per-user tokens from the SSM roster. No telemetry; feedback flows through
  the dashboard Report button into GitHub issues.

## Open items to confirm

- Install URL: a custom domain like `get.ctx.tools`, or the default CloudFront domain for now.
- Where the alpha token roster lives and who mints tokens (SSM parameter plus a small `ctx-issue-token`
  helper, or by hand for the first few users).
