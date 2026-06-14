# 0011. Intent signal on Cursor, and reasoning capture from the proxy stream

- Status: Part A accepted and shipped. Parts B and C reverted after live measurement (see Update).
- Date: 2026-06-12
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-20 (epic), CTX-21 (Cursor narration), CTX-22 (proxy reasoning + settings control)
- Extends: ADR 0004 (intent signal), ADR 0001 (read edit-intent guard)

## Update (2026-06-12): proxy reasoning capture reverted

Part B rested on one assumption: that extended thinking is readable on the Anthropic wire even
though Claude Code persists it signature-only on disk. We built the capture path, turned it on, and
measured a real Claude Code session. The assumption was wrong.

The streamed response carries `thinking_delta` events with an **empty** `thinking` field and only
an `estimated_tokens` count, followed by an encrypted `signature_delta`. Across every thinking
delta in the captured sample, zero carried readable text. Anthropic encrypts extended thinking end
to end; the proxy sees the same signature-only blob the disk gets. The only readable content in the
stream is the narration (`text_delta`), which is already on disk as `text` blocks and which Part A
and ADR 0004 already use.

So the proxy bought nothing over disk narration, while adding a system-level MITM CA, the trust
cost, and dead code. We reverted Parts B and C: removed the `proxy_capture` module, the
`reasoning_capture` table, the `proxy_capture_reasoning` config flag, the SSE tee in the proxy, the
`accept-encoding: identity` forcing, the Settings reasoning-capture card and its endpoint, and the
decision-time fallback. Part A (Cursor narration) stays, shipped and useful.

Lesson recorded for honesty: the wire-vs-disk claim below was never verified before building on it.
It should have been a one-session experiment first. The original decision text is kept verbatim
below so the reversal is legible.

## Context

ADR 0004 shipped the read edit-intent guard on Claude Code only, off the agent's **narration**
(assistant `text` blocks), after live measurement showed extended-thinking is persisted
signature-only on disk and carries no readable text. It named two follow-ups it explicitly did
not do:

1. The guard does not run on the **Cursor** surface, so a working read the agent has declared it
   will edit is unprotected there. ctx trims it like any other read.
2. The agent's **reasoning** is the strongest intent channel, but it is not on disk in a readable
   form, so we fell back to narration, which is noisier.

Two facts change what is now possible:

- **Cursor persists narration.** Its agent transcript
  (`~/.cursor/projects/<enc>/agent-transcripts/<uuid>/<uuid>.jsonl`) carries assistant `text` and
  `tool_use` blocks. We already parse this file for outcome joins (`surface/cursor.rs`). The same
  `text` blocks that drive intent on Claude Code are sitting there unused at decision time.
- **The reasoning is readable on the wire, just not on disk.** When extended thinking is on, the
  Anthropic streamed response carries readable `thinking_delta` text before the `tool_use` it
  leads into. Claude Code writes only the signature to disk, but ctx's MITM proxy terminates that
  exact stream and can read the text in flight. The proxy is the one place the reasoning exists in
  the clear on the user's machine.

Honest constraints carried forward from ADR 0004:

- The reasoning channel is **Claude Code only**. Cursor sends model calls to its own backend, not
  to `api.anthropic.com`, so our Anthropic-scoped MITM never sees Cursor reasoning. Cursor gets
  narration, not reasoning.
- Reasoning is the most sensitive content in a session. Capturing it raises the trust bar, so it
  must be **off by default, user-visible, and reversibly toggled**.
- The signal stays **purely protective**: it can only turn a trim off, never on. A false positive
  costs a little context, never correctness.

## Decision

### Part A: narration intent on Cursor (CTX-21)

Generalize the intent reader so it parses both transcript shapes: Claude Code's `{"type":
"assistant", ...}` rows and Cursor's `{"role": "assistant", "message": {...}}` rows. At Cursor
decision time, resolve the transcript path from the hook payload (`transcript_path` when present,
otherwise derive it from the session UUID and cwd the same way `surface/cursor.rs` discovers
sessions), read the tail, and lift the most recent narration. From there the existing
`IntentSignal` logic (basename mention + edit verb -> protect) is unchanged and surface-agnostic.

This is additive and protective, gated by the existing `compress_intent_log` (default on). It adds
no new trust cost: Cursor already writes these transcripts and we already read them for outcomes.

### Part B: reasoning capture from the proxy stream (CTX-22)

Add an opt-in capability, off by default, behind a new config flag `proxy_capture_reasoning`
(false) and only meaningful when the legacy MITM proxy is installed.

When enabled and the proxy is forwarding an Anthropic `text/event-stream` response, the proxy tees
the stream: it passes every chunk to the client untouched (fail-open, never blocks or delays the
response) and, on a copy, reassembles the SSE to extract (a) the concatenated `thinking_delta`
text and (b) the `tool_use` block(s) that reasoning leads into. It writes one row per captured
reasoning span to a new local table keyed by the **tool-input fingerprint**
(`surface::fingerprint_tool_input`), with a timestamp, capped in size and count.

At decision time, when `recent_intent_text_for_payload` finds no readable narration **and**
capture is enabled, the controller looks up the most recent reasoning row whose fingerprint matches
the current tool input within a short window and uses it as `recent_intent_text`. The fingerprint
join is the same mechanism the transcript ingest already uses, so reasoning and narration flow
through the identical `IntentSignal` path. Reasoning is preferred when present; narration is the
fallback. Capture failure is silent and falls back to narration.

### Part C: the proxy is the user's to understand and turn off (CTX-22)

The proxy stops being invisible plumbing. Settings gains a plain-language section:

- What the proxy is: an optional local MITM that lets ctx read the model's reasoning to protect
  your reads better. It only intercepts traffic to `api.anthropic.com`; all other traffic is
  untouched. Everything stays on your machine.
- Its live status: installed or not, capture on or off.
- One control to turn it on (installs the CA + proxy, enables capture) and one to turn it off
  (uninstalls, restores `settings.json`, disables capture). These call the existing
  `ctx proxy install` / `proxy::uninstall` paths.

Default install does not touch the proxy. A user must opt in from Settings, and can opt out at any
time with a single action.

## What this deliberately does not do

- It does not turn the proxy on by default, and does not enable capture by default.
- It does not capture or store reasoning for any surface other than Claude Code.
- It does not let the signal *enable* trims the static guard blocks. Still protect-only.
- It does not persist reasoning for offline training; capture is for the live protective signal and
  is size/count capped. Offline training use is a separate future decision.
- It does not MITM Cursor's backend. Cursor gets narration only.

## Alternatives considered

- **Cursor narration only, skip the proxy.** Rejected as the endpoint: narration is noisier than
  reasoning, and the reasoning is right there on the Claude Code wire. But this is the safe first
  step and ships independently (Part A has no trust cost).
- **Read reasoning from disk on Claude Code.** Rejected: measured signature-only, the text is not
  there (ADR 0004).
- **Proxy on by default.** Rejected: a system-level MITM CA and capture of raw reasoning is the
  heaviest trust ask in the product. It must be opt-in and clearly reversible.
- **Store full reasoning for training now.** Deferred: prove the live protective signal first; the
  privacy cost of a reasoning corpus needs its own decision.

## Consequences

- A working read the agent declares it will edit is now protected on Cursor too, not just Claude
  Code. The CTX-8 class of harm gets its intent backstop on the second surface.
- Users who opt into the proxy get the stronger reasoning signal on Claude Code, with capture
  visible and reversible in Settings.
- The retention model still must work with neither signal present; both remain enhancements, never
  requirements. Coverage stays uneven by surface and by whether the proxy is on.
- New surface area to keep honest: the Settings copy must state plainly that the proxy reads model
  reasoning and intercepts Anthropic traffic, and the off switch must fully restore prior state.
