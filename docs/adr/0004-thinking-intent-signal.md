# 0004. Read trimming reads the agent's narration for edit-intent

- Status: accepted (prototype, measurement-first)
- Date: 2026-06-08 (updated 2026-06-09)
- Deciders: Saurabh Sharan, ctx CTO partner
- Ticket: CTX-11

## Update (2026-06-09): thinking is unreadable on Opus; pivoted to narration

The original decision read the agent's extended-**thinking** blocks. Live measurement killed that
source: on `claude-opus-4-8` via Claude Code, thinking is persisted **signature-only**. The
`thinking` text field is empty and only an encrypted `signature` is stored. Across 8 recent
sessions, 1294 of 1294 thinking blocks had no readable text, so the signal recorded nothing useful
(15/15 reads came back with no readable reasoning). The plumbing was correct end to end
(`transcript_path` is delivered, the file is found, the parser and window are fine); the data
simply is not there.

The readable intent on that surface lives in assistant **text** blocks (narration), which are
stored in plaintext, are on disk at hook time, and in a sample carried an edit verb in ~17% of
blocks with real file/symbol names ("Now update the references to `MATCH_SPEED`"). So the signal
pivoted from thinking to narration: it now reads assistant `text` blocks, and still uses readable
`thinking` text when a model/surface happens to store it, so it degrades gracefully. The rest of
this ADR is updated to reflect that; the structure (read tail, detect basename + edit verb, protect
only) is unchanged. The trade-off: narration is noisier than reasoning (the agent narrates about
many files), which is acceptable because the signal is purely protective.

## Context

ADR 0001 protects working reads with a static path classifier and explicitly rejected, for
Phase 1, "predict edit-intent from the model's stated plan," on the grounds that the PostToolUse
hook is synchronous and "cannot see the future turn."

That reasoning is half right. We cannot see the *next* turn, but we can see the agent's stated
reason for the call we are deciding on. Claude Code persists the agent's extended-thinking blocks
in the session transcript (`~/.claude/projects/**/*.jsonl`), and the PostToolUse hook payload
carries `transcript_path`. The thinking that *precedes* a tool call is the agent's intent for
that call ("I need to read FullConversation.tsx so I can edit its render method"). That is exactly
the signal trimming lacks: the static guard infers intent from the file path; the thinking states
it directly.

We already ingest these transcripts; `conversations.rs` even has dedup logic that steps over
thinking blocks. We just discard the thinking text today (`extract_output_text` keeps only the
first `text` block). The signal is sitting in files we already read.

Honest constraints:

- **Claude Code only.** The Cursor adapter (`surface/cursor.rs`) does not persist raw reasoning
  in a form we read. This must not become a dependency of the core model.
- **Privacy.** Thinking is the most sensitive content in a session. We extract a boolean signal,
  not raw chain-of-thought, and reading is gated behind the same posture as the rest of the hook.
- **Stability.** The thinking format and its availability are not a stable contract; some API
  paths return it signature-encrypted. The signal is advisory, never load-bearing.
- **Coarse detection.** Matching a filename mention plus an edit verb is a heuristic with false
  positives.

## Decision

Add a measurement-first intent signal for Read, gated by `compress_intent_log` (default on).

At decision time the Claude Code transport reads the tail of the transcript named by
`transcript_path`, lifts the most recent readable narration (assistant `text` blocks, plus
readable `thinking` text when present), and attaches it to the canonical `ToolResult`
(`recent_intent_text`). For a `read`-kind decision, `agent::decide` computes
`compress::intent::IntentSignal`: whether readable narration exists, names the file (by basename),
and uses an edit verb. When all three hold (`edit_intent_for_path()`), the read is protected even
if the static guard would have let it trim.

The signal is **purely additive and protective**: it can only turn an `apply=true` into
`apply=false`. It never causes more trimming, so a false positive costs a little context, never
correctness. This is the safe direction to ship a prototype in.

Every read decision records the outcome in shadow features as `intent: Option<IntentSignal>`:
`Some(..)` when the signal ran, `None` when it did not apply. The struct records the three
components separately (`has_text`, `mentions_path`, `has_edit_verb`) rather than a single collapsed
verdict. This is deliberate: a collapsed boolean conflates "no readable narration was available"
(Cursor surface, signature-only thinking, no transcript) with "narration present but no
edit-intent," which makes prevalence unanswerable. With the components split, `has_text` measures
coverage independently of the intent rate. This is the point of the prototype: before we rely on it
for harder calls (line-level trim targeting, allowing trims the static guard blocks), we measure
how often the signal is even available, how often intent is present, and how often it agrees with
the static classifier.

### What this prototype deliberately does not do

- It does not use the signal to *enable* trimming the static guard protects.
- It does not do line-level trim targeting from named symbols.
- It does not read narration for non-Read kinds.
- It does not touch the Cursor or Codex surfaces.

Those are follow-ups, justified only after the recorded prevalence shows the signal is worth it.

## Alternatives considered

- **Stay with the static path classifier only (ADR 0001).** Rejected as the endpoint, kept as the
  floor: the classifier cannot protect a reference-path read the agent has declared it will edit,
  and it cannot target lines. Thinking is the missing intent channel.
- **Thread thinking into `compute_shadow_decision`.** Rejected: that function is pure over its
  inputs and does no IO. Reading the transcript lives in the transport, and the controller
  annotates the feature, keeping the IO boundary clean.
- **Persist raw thinking for offline training now.** Deferred: higher privacy cost, and we should
  prove the live signal earns its keep first. Offline use of post-result reaction-thinking (for
  cleaner correction labels) is a separate future ticket.
- **Use thinking to trim more aggressively immediately.** Rejected: that is the unsafe direction
  and would re-risk the CTX-8 harm before we have evidence the detector is reliable.

## Consequences

- A reference-path read the agent declares it will edit is now protected. The CTX-8 class of harm
  gets a second, intent-based backstop on top of the path heuristic.
- `compress_intent_log` defaults on. Turning it off restores pure ADR-0001 behavior and records
  nothing.
- New per-decision feature `intent` (with `has_text` / `mentions_path` / `has_edit_verb`) lets us
  answer "how often is narration even available, how often is intent present, and does it agree with
  the static guard?" from existing shadow data, with no new pipeline.
- Coverage is uneven by surface (Claude Code only). The core retention model must keep working
  without this signal; it is an enhancement, not a requirement.
- Follow-ups, gated on observed prevalence: line-level trim targeting from named symbols;
  allowing intent to *permit* trims the static guard blocks (only with strong precision evidence);
  offline use of post-result narration as a correction/re-read label for `learn.rs`.
