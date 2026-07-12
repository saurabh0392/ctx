# ctx Windows port: session handoff

Paste this into a fresh Claude Code session on the `feat/windows` branch to start the Windows port
cold. It carries everything a prior session verified, so you do not need that context.

## Mission

Bring ctx to first-class Windows support (`x86_64-pc-windows-msvc`). Not a shim: real `cfg(windows)`
implementations. Target parity with macOS and Linux for Claude Code (CLI and IDE): output trimming,
MCP menu pruning, the dashboard, the ctx MCP tools, ingest, and one-command install. Windows has no
launchd, no systemd, and no unix domain sockets, so three subsystems need a Windows path.

## Ground truth (verified, do not re-litigate)

- Zero real Windows code paths today, except `src/config.rs` already has two `#[cfg(target_os =
  "windows")]` arms (~line 220 and ~line 378) for paths. The path layer is partially started.
- You cannot cross-compile or cross-check from macOS or Linux for the msvc target: a transitive native
  crypto dep (`ring`) needs the Windows toolchain. Build and test MUST run on a real Windows machine or
  a `windows-latest` CI runner. Do not trust a cross-check verdict from another OS.
- No unix-only crates in the dependency tree (no direct `nix`, `libc`, `daemonize`). The unix surface
  is concentrated in three files plus some path literals, so this is a bounded port, not a rewrite.
- The test suite is 350 tests and must run serial (it mutates process-global env `CTX_HOME`/`HOME`):
  `cargo test -- --test-threads=1`. CI already does this on other OSes.

## The three hard blockers (fix in this order)

1. IPC. `src/socket.rs` opens a tokio unix domain socket (`UnixListener`/`UnixStream`) at
   `ctx_dir()/ctx.sock` for local read-only state queries, consumed by `src/dashboard.rs`. Unix sockets
   do not exist on Windows. Replace with a `cfg`-gated transport that keeps the same newline-delimited
   JSON protocol: a named pipe (`\\.\pipe\ctx-<user>`) or a `127.0.0.1` TCP socket on an ephemeral port
   written to a file under `ctx_dir()`. Gate with `cfg(unix)` / `cfg(windows)`.

2. Service and autostart. `src/daemon.rs` and `src/setup.rs` install launchd agents (macOS) and systemd
   units (Linux) for the dashboard (port 8789) and the 5-minute ingest. Windows has neither. Add a
   `cfg(windows)` path: a Scheduled Task (`schtasks`) or a Startup-folder launcher that starts the
   dashboard and ingest and survives reboot. `ctx setup --uninstall` must remove it (mirror the
   teardown skill).

3. Paths and settings. About 29 hardcoded unix path literals and about 30 `cfg(unix/target_os)` gates,
   most without a Windows arm. Use `dirs::home_dir()` (cross-platform) and `%APPDATA%` /
   `%LOCALAPPDATA%` where right. Confirm where Claude Code on Windows reads `settings.json` (hooks) and
   its MCP config (`~/.claude` vs `%APPDATA%`), and finish the `config.rs` Windows arms. `src/host.rs`
   detects the host with macOS/Linux branches and a fallback: add a real Windows branch.

## Also required

- `scripts/install.ps1`: a PowerShell mirror of `scripts/install.sh`. Same flow: read `CTX_TOKEN`,
  POST `{token, target}` to the endpoint, get the presigned URL and sha256, download, verify the
  checksum, install to a dir on PATH (`%LOCALAPPDATA%\ctx` or similar), then `ctx setup --yes`. The
  distribution Lambda already serves `GET /install.ps1` with `__CTX_ENDPOINT__` templating (see
  `services/ctx-dist/lambda/handler.ts`), and `scripts/dist-publish.sh` uploads `install.ps1` when it
  exists. The Windows release target is `x86_64-pc-windows-msvc`.
- CI: add a `windows-latest` job to `.github/workflows/ci.yml` (build plus serial tests) and a Windows
  target to the `release.yml` build matrix so releases publish a Windows binary to the dist bucket.
- Trust: an unsigned `.exe` triggers SmartScreen. Interim bridge is the user clicking "More info, then
  Run anyway." The real fix is an Authenticode certificate (OV or EV), a later step. Note it, do not
  block on it.

## Milestones (one commit each, Windows tests green before moving on)

- M1 Compile. Get `cargo build` and the test suite green for `x86_64-pc-windows-msvc` on a Windows
  runner. Minimally `cfg` out or stub the unix-only bits so it builds. This will likely surface more
  than the three blockers above; let the compiler drive.
- M2 IPC. Land the `cfg`-gated transport in `socket.rs` and `dashboard.rs`. The dashboard reads ctx
  state on Windows.
- M3 Service. `daemon.rs`/`setup.rs` Windows autostart plus uninstall. `ctx setup --yes` brings the
  dashboard up at http://127.0.0.1:8789 on Windows and it survives a reboot.
- M4 Paths and wiring. Hooks and the MCP server register into Claude Code on Windows. Drive one real
  tool result end to end: it gets trimmed with the `ctx_expand` marker, and `ctx_expand` recovers it.
- M5 Installer. `install.ps1` written and published. A one-command install works on a clean Windows box.
- M6 CI. Windows job in `ci.yml` and Windows target in `release.yml`, both green.

## Definition of done

On a clean Windows 11 machine: the install one-liner installs ctx, the dashboard is live at
http://127.0.0.1:8789, hooks and the MCP server are registered in Claude Code, a real tool result is
trimmed and `ctx_expand` recovers it verbatim, and `ctx setup --uninstall` removes everything. The full
suite is green on the Windows CI job.

## Conventions (match the repo)

- Conventional commits (`feat`/`fix`/`refactor`/`chore`). Footer:
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- No em dashes and none of the humanizer avoid-list words in any prose you write.
- Do not merge or deploy without asking. Branch protection is not enforced (private free repo).
- The alpha distribution (`services/ctx-dist`) is live in AWS us-east-1 and gates downloads by token.

## Key files

- `src/socket.rs` (IPC), `src/dashboard.rs` (consumer)
- `src/daemon.rs`, `src/setup.rs` (service model, install and uninstall)
- `src/config.rs` (paths, Windows arms started ~220 and ~378), `src/host.rs` (host detection)
- `scripts/install.sh` (bash installer to mirror), `scripts/dist-publish.sh`,
  `services/ctx-dist/lambda/handler.ts` (serves `install.ps1`)
- `.github/workflows/ci.yml`, `.github/workflows/release.yml`

## First move

On `feat/windows`, add a `windows-latest` job to `ci.yml` as a non-blocking (allow-fail) check first,
so every push shows the real compiler errors from a Windows runner. Fix M1 from those errors, then
tighten the job to blocking once it is green. Let the Windows compiler, not a cross-check, be the
source of truth.
