# 0024. Act on Cursor Shell output via a preToolUse input rewrite (`ctx run`)

- Status: accepted
- Date: 2026-06-14
- Deciders: Saurabh (with ctx CTO partner)

## Context

On Cursor, ctx could only ever *rewrite* MCP tool output, via `postToolUse`'s `updated_mcp_tool_output`
(ADR 0018 / 0021). Built-in Read, Shell, and Grep output cannot be rewritten by a `postToolUse` hook:
for non-MCP tools Cursor discards the hook's output. So Shell stayed observe-only on the output path,
and the dashboard could show Cursor Shell savings only as a hypothetical.

The CTX-39 spike confirmed a second lever Cursor exposes: a `preToolUse` hook can rewrite a tool's
*input* before it runs, via `updated_input`. For Shell that means rewriting the `command` string. This
is exactly how RTK acts on Cursor in production: it rewrites `git status` to `rtk git status`, the
wrapped command runs, and the compacted result comes back as Shell's own output. The spike also
measured ctx's existing compressors at 80–98% reduction on noisy shell output, and surfaced a data-loss
bug (CTX-40, `git diff --stat` erased to empty) that had to be fixed first.

Two facts shaped the design:

- **The rewrite is visible.** Cursor shows the user the rewritten command (RTK users see `rtk …`). True
  visual transparency is not achievable through this hook. The product decision (recorded on CTX-41) is
  that a visible, readable wrapper is acceptable, so we use a clean `ctx run <cmd>` form rather than
  pretending it is invisible.
- **`preToolUse` is not the shell approval gate.** Cursor does not enforce `permission: "ask"` at
  `preToolUse` (`beforeShellExecution` is the approval hook). So pairing `permission: "allow"` with
  `updated_input` does not bypass the user's command approval flow.

## Decision

Add a `ctx run <command>` subcommand and a Cursor `preToolUse` Shell hook that rewrites earned, safe
shell commands to run through it.

- **`ctx run` is the runtime engine.** It runs the command through the user's shell, and only when the
  shared earn-it gate (`agent::decide`, the same gate Claude uses) says apply *and* the compressor
  actually shortens the combined output does it return the compacted text. Otherwise it replicates the
  real stdout/stderr byte-for-byte. It always preserves the command's exit code. A real trim records a
  `compress_event` + analytics under `surface = "cursor"`, so savings stay honest. This is bulletproof
  passthrough: unless a clean, gated compaction happens, the command behaves exactly as if ctx were not
  there.
- **The `preToolUse` hook is a cheap front gate.** It rewrites a command to `ctx run <cmd>` only when
  both hold: (1) the command is on a default-deny allowlist of read-only, non-interactive inspection
  commands (git status/diff/log/show/…, ls, grep, rg, find, cat, tree, head, tail, wc, cargo), so
  capturing output can never break an editor, pager, REPL, or prompt; and (2) Shell has earned trimming
  for that command's kind (trial, activation, or burn-in). When it rewrites, it emits
  `{ "permission": "allow", "updated_input": { "command": "ctx run '<original>'" } }`, quoting the
  original as a single argument so pipes and redirects survive. The wrapper then re-checks the gate
  against the real output, so the hook front gate is never the final say.
- **No double counting.** Because a rewritten command's input becomes `ctx run …`, the `postToolUse`
  hook recognizes the `ctx run` prefix and stays out of the way, so a command the wrapper already
  compacted and recorded is not also recorded as an observe decision.
- **Shell is unified with Bash for classification.** Cursor's "Shell" now classifies by its command
  (git/grep/test) the same way Claude's "Bash" does, instead of falling through to the generic strategy.

## Alternatives considered

- **Keep Shell observe-only on Cursor.** Honest but leaves real, measured savings (80–98% on noisy
  commands) on the table on the agent the user actually runs. Rejected once the spike proved the input
  path works.
- **Inject a shell function/alias so the displayed command stays unchanged (true transparency).**
  Pollutes the user's shell rc, affects every terminal (not just the Cursor agent), and is exactly the
  global-environment coupling ctx removed in ADR 0015. Rejected; not worth it, and the user accepted a
  visible wrapper.
- **Wrap every shell command and let the wrapper decide.** Simpler hook, but wrapping interactive
  commands (editors, pagers, prompts) breaks them because the wrapper captures output. Rejected in favor
  of a default-deny allowlist.
- **Rewrite output without `permission`.** Whether `updated_input` is honored without `permission` is
  undocumented; RTK pairs them in production. We follow the proven pattern and document it for live
  re-verification.

## Consequences

- ctx can finally act on Cursor Shell output, not just MCP, behind the same gate it uses everywhere, so
  cross-surface savings become real instead of hypothetical.
- The wrapped command is visible to the user as `ctx run '<cmd>'`. Accepted tradeoff (CTX-41).
- The allowlist is conservative by design; it will need to grow as we confirm more commands are safe to
  wrap. Anything unrecognized is simply left untouched, so the failure mode is "no savings", never a
  broken command.
- Live-confirmed on a real Cursor build (2026-06-14): with Shell forced into trial, a `git log -n 400`
  Shell tool call was rewritten by the `preToolUse` hook to `ctx run`, Cursor applied `updated_input`,
  the wrapper ran and returned compacted output (58,483 -> 2,500 chars), recorded as a `Shell`
  compress_event under `surface = "cursor"`, and `postToolUse` correctly skipped the wrapped command.
  So `updated_input` with `permission: "allow"` does take effect for Shell. The gate keeps the hook
  inert until Shell earns trimming, so a fresh install still changes nothing until there is evidence.
- Supersedes the "Shell stays observe-only on Cursor no matter what" framing in ADR 0021 (already
  corrected there by the CTX-39 note): that limit is real for the `postToolUse` *output* path, not for
  the `preToolUse` *input* path.
