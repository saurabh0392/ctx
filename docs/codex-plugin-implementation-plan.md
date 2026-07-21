# CTX Codex plugin implementation plan

Status: implemented; private-beta validation pending
Date: 2026-07-18
Target: next CTX beta wave
Decision owner: Saurabh
Extension: `docs/model-gateway-implementation-plan.md`
Codex surface: `docs/codex-model-gateway-implementation-plan.md`

## Outcome

Ship CTX as an installable Codex plugin that can observe Codex work live, record compaction and
outcome signals, expose CTX recovery/status tools, and trim only the Codex tool paths whose output
CTX can actually control.

The product must describe three capability levels accurately:

| Status | What it means |
| --- | --- |
| Observing | CTX sees tool activity and outcomes but changes no output. |
| Partially active | CTX can shorten specific output paths, currently safe wrapped shell commands. Other paths remain observation-only. |
| Active | CTX can replace every supported tool result before Codex consumes it. This is not currently possible through Codex's documented `PostToolUse` contract. |

The first commercial release target is **partially active**, not full parity with Claude Code.

This remains the contract for standard, plugin-only mode. The umbrella model-gateway plan and its
Codex surface plan propose an opt-in Responses route that can make additional local tool results
model-visible after its authentication, fidelity, security, and evidence gates pass.

## Why this is feasible now

Verified locally with Codex CLI 0.144.5 on 2026-07-18:

- The `plugins` and `hooks` features are stable.
- Plugins can bundle lifecycle hooks and MCP configuration.
- Codex exposes `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PreCompact`,
  `PostCompact`, `SubagentStop`, and `Stop` hooks.
- `PreToolUse` can observe, block, or rewrite local function-tool calls.
- `PostToolUse` can observe local tool results. `updatedMCPToolOutput` is rejected and
  `suppressOutput` is parsed but not implemented. A blocking decision can substitute textual
  feedback, but it is error-shaped rather than a clean tool-native replacement and is not enabled.
- Hosted tools such as web search do not pass through the local tool-hook path.
- Users must review and trust non-managed plugin hooks before they run.

Sources: [Codex hooks](https://learn.chatgpt.com/docs/hooks),
[Codex plugin structure](https://learn.chatgpt.com/docs/build-plugins#plugin-structure).

This gives CTX a strong observation surface and the same shell-command rewrite strategy already
proven in Cursor. It does not provide a clean, tool-native way CTX can safely ship to rewrite
built-in Read/Grep or arbitrary MCP results after they execute.

## Product contract

### What v1 will do

- Install through a CTX marketplace entry and standard `codex plugin` commands.
- Register CTX hooks without directly editing `~/.codex/hooks.json`.
- Register the existing `ctx mcp` server through the plugin.
- Observe local Codex tool calls from the moment the plugin is trusted.
- Record Codex session, turn, tool, repo, correction, re-touch, and compaction provenance.
- Rewrite eligible, non-interactive shell commands to `ctx run <command>` before execution.
- Preserve the command's exit code and byte-for-byte output whenever CTX does not apply a trim.
- Show Codex in the dashboard with an explicit capability label.
- Remove the plugin, hooks, MCP registration, and CTX-owned configuration cleanly.

### What v1 will not claim

- It will not claim that all Codex output is trimmable.
- It will not claim control over hosted tools.
- It will not add a compressed copy through `PostToolUse`; that would leave the original in context
  and consume more tokens.
- It will not parse Codex transcripts as its primary live contract. The official hook documentation
  warns that transcript format is not stable.
- It will not let evidence collected on Claude or Cursor automatically authorize a Codex trim.
- It will not patch Codex, proxy the model API, or depend on undocumented internal state.

## Proposed plugin package

```text
plugins/ctx/
├── .codex-plugin/
│   └── plugin.json
├── .mcp.json
├── hooks/
│   ├── hooks.json
│   ├── run-ctx.sh
│   └── run-ctx.ps1
├── skills/
│   └── ctx/
│       └── SKILL.md
└── assets/
    ├── icon.png
    └── logo.png

.agents/plugins/
└── marketplace.json
```

The plugin does not bundle native CTX binaries. The normal CTX installer owns the correct binary
for the machine; the plugin owns Codex discovery and lifecycle wiring. The dispatch scripts resolve
the installed binary, fail open when it is missing, and give `SessionStart` one concise remediation
message instead of breaking a tool call.

The bundled skill is deliberately small. It teaches Codex when to use `ctx_expand`, `ctx_status`,
and the local Context Bill. It must not ask Codex to route ordinary work through CTX manually.
Behavioral enforcement belongs in hooks, not prompt instructions.

## Hook-to-CTX contract

| Codex event | CTX entry point | Behavior | Mode |
| --- | --- | --- | --- |
| `SessionStart` | `ctx hook codex-session-start` | Record a heartbeat and report a missing/incompatible CTX binary once. | Observe |
| `UserPromptSubmit` | `ctx hook codex-user-prompt-submit` | Record the turn and correction/steer signals using the shared lexical guard. | Observe |
| `PreToolUse` | `ctx hook codex-pre-tool-use` | Normalize tool input; rewrite allowlisted Bash commands to `ctx run` only when the Codex-specific gate allows it. | Observe/act |
| `PostToolUse` | `ctx hook codex-post-tool-use` | Normalize result, calculate the shadow transform, and record the decision. Never imply the primary result was changed. | Observe |
| `PreCompact` | `ctx hook codex-pre-compact` | Record the impending native compaction and its trigger. | Observe |
| `PostCompact` | `ctx hook codex-post-compact` | Record completion and connect it to later correction/steer signals. | Observe |
| `Stop` / `SubagentStop` | `ctx hook codex-stop` | Close pending outcome windows and record abort/stop state. | Observe |

Every hook must:

- accept one JSON object on stdin and emit valid JSON on stdout;
- return success with an inert response on malformed or unknown payloads;
- avoid prompts, network calls, and long-lived locks;
- stamp `surface = "codex"` and preserve `session_id` plus `turn_id`;
- deduplicate retries with a stable event key;
- skip `ctx run ...` results already recorded by the wrapper;
- log diagnostics locally without putting user content in hook output.

## Architecture changes

### 1. Codex transport

Add `src/codex_hook.rs` with a `CodexTransport` implementation of the existing
`AgentTransport` boundary. Normalize Codex tool names before classification:

- `Bash` and unified exec -> `Shell` for surface reporting, with the existing command classifier;
- `apply_patch`, `Edit`, and `Write` -> the shared edit family;
- MCP names -> stable MCP server/tool identity;
- unsupported or hosted tools -> observation-only with an explicit capability reason.

Add Codex hook variants in `src/cli.rs` and dispatch them in `src/lib.rs`. Keep parsing functions
pure and fixture-driven, as with `src/cursor_hook.rs`.

### 2. Surface-specific activation evidence

This is a blocker before Codex data is admitted to the live gate.

Today `causal_tool_outcomes` groups by `tool_name` across all surfaces. That means Claude evidence
could authorize a Codex shell trim even though the transport and output shape differ. Change the
activation identity to:

```text
surface + normalized tool + transform version
```

The compressor and learned features may remain shared. Permission to act must be earned separately
on each surface. Legacy rows with no surface continue to map to `claude-code`.

Required changes:

- add surface-aware filters/keys to causal outcome and activation queries;
- persist a transform version with each decision or derive a stable versioned gate key;
- invalidate/restart evidence when a material transform changes;
- add migration and mixed-surface regression tests;
- expose the surface in proof APIs so dashboard copy cannot merge unlike experiments.

### 3. Codex compaction storage

Generalize the Cursor-specific compaction event path into a native compaction record keyed by
surface, session, turn, trigger, and timestamp. Keep pre/post events distinct and idempotent.

The dashboard may say Codex compaction is observed only after a real hook event lands. Merely having
the plugin installed is not evidence that a compaction occurred.

### 4. Historical ingestion

Live hooks are the v1 source of truth. A historical `src/surface/codex.rs` transcript adapter is a
separate, version-gated increment because Codex explicitly does not promise a stable transcript
format.

If built, it must:

- detect supported transcript versions/shapes;
- reject unknown shapes without partial writes;
- retain sanitized golden fixtures for every supported version;
- report `history unsupported` rather than zero sessions when parsing cannot be trusted;
- never upgrade transcript-only observations to live-action evidence.

## Work plan

### C0 — Contract spike and fixtures

Capture sanitized payload fixtures from the installed Codex build for every target event. Prove:

1. plugin install, enable, disable, upgrade, and removal;
2. hook trust behavior through `/hooks`;
3. exact `PreToolUse` rewrite response for a harmless `printf` command;
4. `PostToolUse` payload shapes for Bash, unified exec, `apply_patch`, MCP, and another local tool;
5. pre/post compaction payloads;
6. duplicate behavior for unified-exec polling and subagents;
7. that attempted `PostToolUse` output replacement is ignored, and therefore must not ship.

Deliverables:

- sanitized JSON fixtures under `tests/fixtures/codex/`;
- `docs/adr/0037-codex-plugin-surface.md` recording the verified contract;
- a capability matrix marked `verified`, `unsupported`, or `unknown`—never assumed.

Exit gate: no implementation proceeds on an unverified payload or rewrite field.

### C1 — Plugin skeleton and local marketplace

- Add the plugin structure and manifest.
- Bundle hooks and existing CTX MCP server registration.
- Add a repo-local marketplace for development.
- Validate with `codex plugin marketplace add`, `codex plugin add`, and `codex plugin list --json`.
- Verify plugin hooks remain inert until explicitly trusted.

Exit gate: a clean Codex profile can install, discover, enable, and remove CTX without hand-editing
configuration files.

### C2 — Live observation

- Implement the Codex transport and hook entry points.
- Normalize tool results into the existing controller.
- Add idempotency and DB busy-timeout handling because matching hooks may run concurrently.
- Record surface-specific shadow decisions and compaction events.
- Join prompt/stop events to correction, re-touch, retry, abort, and session-steer signals.

Exit gate: a real Codex session produces exactly one decision per supported local tool call, appears
as `Codex — observing`, and never changes the tool result.

### C3 — Safe shell action

- Reuse the conservative `ctx run` allowlist and quoting rules.
- Make the wrapper accept an explicit `--surface codex` provenance value.
- Apply only after the Codex-specific evidence gate permits the transform.
- Preserve stdout, stderr, and exit status when no shorter result is produced.
- Skip interactive commands, unresolved executable paths, nested wrappers, and hosted tools.

Exit gate: a live Codex shell call is visibly rewritten, produces a shorter result only when earned,
can be restored through `ctx_expand`, and is recorded once under `surface = codex`.

### C4 — Setup, doctor, and uninstall

- Detect the Codex CLI/app without treating mere installation as plugin activation.
- Add `ctx setup` steps for marketplace registration and plugin installation.
- Respect the unavoidable Codex hook-review step and explain `/hooks` in one sentence.
- Add doctor checks for Codex version, plugin installed/enabled state, hook event heartbeat, MCP
  availability, and binary compatibility.
- Make uninstall remove only CTX-owned plugin/marketplace state and preserve unrelated Codex config.
- Cover macOS, Linux, and Windows dispatch paths.

Exit gate: fresh install and uninstall are idempotent, and `ctx doctor --json` distinguishes
`installed`, `awaiting_hook_trust`, `observing`, and `partially_active`.

### C5 — Dashboard capability model

Replace the current binary implication that an agent either exists or is fully trimmable. Each
surface card should report capabilities directly:

```text
Codex                         PARTIALLY ACTIVE
Shell output                  CTX can shorten after it passes the safety check
Built-in Read / search        Observed only
MCP results                   Observed only
Compaction                    Visible
Last activity                 2 min ago
```

The API should return structured fields rather than forcing the browser to infer capability:

```json
{
  "surface": "codex",
  "seen": true,
  "integration": "plugin",
  "status": "partially_active",
  "can_observe": true,
  "can_trim_shell": true,
  "can_trim_builtin": false,
  "can_trim_mcp": false,
  "can_observe_compaction": true,
  "limitations": ["Clean PostToolUse result replacement is not enabled"]
}
```

Exit gate: no Codex card or marketing copy uses “active,” “trimmable,” or “supported” without showing
which output paths CTX controls.

### C6 — Beta and release

- Dogfood on the project itself, but exclude CTX self-development rows from activation evidence.
- Run a private beta across CLI, IDE extension, and desktop Codex surfaces.
- Confirm no hook regressions across Codex upgrades before widening distribution.
- Version the plugin independently from the CTX binary and declare compatible CTX/Codex ranges.
- Publish through the CTX marketplace only after install, trust, update, and rollback are proven.

Exit gate: five beta users complete installation without manual config editing; at least three real
Codex sessions per supported surface are recorded; hook p95 overhead stays below 50 ms excluding the
wrapped command; no duplicate decisions; no user content leaves the machine.

## Test matrix

### Automated

- Unit fixtures for every hook event and malformed-input case.
- Golden stdout tests: every handler always returns a valid Codex hook response.
- Surface isolation: Claude/Cursor evidence cannot activate Codex.
- Deduplication across unified-exec completion/polling and `ctx run`.
- Shell quoting, pipes, redirects, non-zero exits, stderr, Unicode, and large output.
- Concurrent hook writes and locked-database failure-open behavior.
- Plugin manifest, hook-path, MCP-path, and marketplace schema validation.
- Setup/uninstall preservation tests for unrelated Codex configuration.
- Dashboard API and copy snapshots for every capability state.

### Live

- Codex CLI, IDE, and desktop app.
- macOS Apple Silicon first; macOS Intel, Linux x86_64, and Windows after native artifacts exist.
- Startup, resume, compact, subagent, interrupted command, MCP call, and long-running unified exec.
- Plugin update with a changed hook hash, including the re-trust experience.
- CTX binary missing, too old, or temporarily unavailable.

## Privacy and performance requirements

- Raw prompts and tool results remain local.
- Hook logs contain event ids, byte counts, timings, decisions, and error classes—not content.
- The plugin makes no background network request.
- Beta check-ins remain previewed, explicit, aggregate-only, and capability-authenticated.
- Observation hooks target p95 under 50 ms and fail open.
- Hook failures cannot block a Codex tool or stop a session unless the user explicitly enables a
  future enforcement feature.

## Rollout metrics

Measure locally and include only reviewed aggregates in optional beta check-ins:

- install -> enabled -> trusted conversion;
- event capture rate by hook type;
- duplicate/drop rate;
- hook latency and failure rate;
- Codex sessions and tool calls observed;
- shell runs eligible, wrapped, shortened, passed through, and expanded;
- correction/re-touch deltas by surface and transform version;
- plugin uninstall and rollback success.

The success metric is not raw tokens removed. It is: **more Codex context reclaimed without a
measurable increase in users going back for missing information.**

## Release cutline

| Capability | v1 requirement |
| --- | --- |
| Plugin install/trust/update/remove | Required |
| Live Codex observation | Required |
| Compaction observation | Required |
| CTX MCP recovery/status tools | Required |
| Safe shell rewriting | Required for “partially active” |
| Built-in Read/Grep output replacement | Not shipped; only error-shaped feedback substitution is verified |
| Arbitrary MCP output replacement | Not shipped; structured replacement is rejected |
| Historical transcript ingestion | Optional follow-up |
| Public marketplace submission | After private-beta proof |

Estimated implementation effort after C0: 8–12 engineering days, followed by at least one week of
dogfood/beta observation. Any Codex hook-contract mismatch discovered in C0 changes the cutline before
code is committed.

## Companion dashboard correction: replace “harm bar” with a decision users can understand

The screenshot that prompted this plan is not a copy-only problem. It exposes conflicting sources of
truth.

### What “harm bar” means in the current implementation

For both corrections and re-reads/re-edits, CTX calculates:

```text
rate after trimming - rate when left whole
```

After at least 30 scored runs on each side, CTX enables a tool only when the upper end of its 95%
interval is no greater than **+10 percentage points**. In plain language: CTX must be reasonably sure
that trimming does not cause more than ten additional look-backs or fixes per hundred runs. When the
estimate is uncertain, output stays whole.

That is a **safety check**, not a user-facing “harm bar.” The statistical term belongs in an advanced
methodology disclosure, not the primary product story.

### Defects visible in the screenshot

1. The top `−10.1 pts` result comes from the all-runs tally: 30 trimmed versus 306 left whole.
2. The “clean test” is a separate randomized subset: 30 trimmed versus only 13 controls, with a
   different observed gap.
3. The UI says to trust the randomized test most, but the live activation gate uses the all-runs
   `causal_tool_outcomes` query.
4. The randomized API currently waits for 20 scored runs per arm, while this card passes the separate
   30-run activation threshold into its copy and tells the user it needs 30.
5. The control card says `13 scored / 13 trimmed` even though those runs were deliberately left alone.
6. “Confidence range,” “point estimate,” “fails closed,” “upper bound,” and “harm bar” make the user
   reverse-engineer the system instead of answering whether CTX is changing output.

### Required decision-integrity fix

Use one comparison for both activation and explanation.

Recommended choice:

- randomized treatment/control rows drive the automatic trim decision;
- all-runs counts remain available as a clearly labeled activity history, never “proof”;
- one shared threshold object supplies API logic, dashboard progress, CLI status, and copy;
- if the randomized sample is too slow, change the allocation deliberately—do not silently substitute
  observational rows;
- every decision API returns `status`, `reason_code`, `runs_needed`, and the exact data source used.

### Replacement primary card

```text
Read                                      WAITING FOR MORE DATA

CTX is keeping Read output unchanged.

Early result: after trimming, the agent re-read 23% of the time,
compared with 46% when CTX left it unchanged. That looks promising,
but only 13 comparable unchanged runs have finished.

7 more comparable runs needed before CTX decides.

[How CTX decides]
```

The exact “7” assumes a single 20-per-arm threshold. If the product chooses 30, it must say “17.” The
API—not the browser copy—must calculate this number.

The expanded methodology may say:

```text
Safety check
CTX compares randomly selected trimmed and unchanged runs. It activates trimming only
when there is enough data to rule out a meaningful increase in re-reads or corrections.
Current safety limit: no more than 10 extra events per 100 runs, using a 95% interval.
```

### Dashboard language rules

- Lead with `Trimming`, `Testing`, `Waiting for data`, or `Kept whole`.
- Say what CTX is doing before explaining why.
- Replace “harm bar” with “safety check.”
- Replace “baseline/control/treatment arm” with “left unchanged/trimmed runs” outside methodology.
- Replace “confidence range crosses the bar” with a concrete missing-data or uncertainty sentence.
- Never show two competing verdicts on the same card.
- Never label an unchanged run as trimmed.
- Put intervals, randomization, margins, and statistical caveats behind `How CTX decides`.
- User-test the collapsed card with the question: “Can someone answer in five seconds whether CTX is
  changing this output and what happens next?”

## Definition of done

The Codex plugin wave is complete only when:

1. A new user installs CTX and the Codex plugin without editing JSON/TOML manually.
2. Codex hook trust is explicit and clearly explained.
3. A real Codex session appears in the dashboard with accurate per-path capabilities.
4. Supported shell output is shortened only through an earned, Codex-specific gate and is reversible.
5. Unsupported tool paths say `Observed only`, not `trimmable` or `zero savings`.
6. Claude, Cursor, and Codex evidence remain isolated at the action gate.
7. Setup, update, doctor, and uninstall work across the supported release targets.
8. The dashboard uses one source of truth and can explain every status without “harm bar” jargon.
