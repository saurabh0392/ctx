# ctx tool management: trimming the input tax

Status: draft for discussion
Date: 2026-07-02
Owner: Saurabh Sharan
Companion: `docs/revamp-education-and-savings.md` (the output side: See/Save/Trust). This is the input side, and it is meant to become as big as output trimming, the second half of the same product.

## The one paragraph

ctx already trims the tax on what comes back to the agent: tool output nobody reads. But there is a second tax, larger and more invisible, on what goes out on every request: the tool menu. Every MCP server you connect loads its full list of tools, names and descriptions and JSON schemas, into the context window on every single request, whether you call it or not. On this one machine, Linear is invoked with 14 tools but ships around 40; Figma, Notion, and Canva load their whole catalogs to be used a handful of ways. That is tens of thousands of tokens of fixed overhead paid on every turn, before the agent reads a single result. Output trimming reclaims variable room per result; tool management reclaims fixed room per request, which compounds far harder. ctx has the raw material (every request records what tools it carried and which it invoked) and most of the machinery (waste detection, a semantic per-session relevance model, auto-generated profiles). It is off and manual. The plan turns it into a first-class feature with the same discipline output trimming earned: see the tax, reclaim it safely and reversibly, and only auto-prune what your own usage proves is dead weight.

## The two taxes, one window

A context window pays two taxes ctx can see:

- **Output tax (existing work).** Tool *results*, the 6M+ characters of Read/Bash/MCP output. Variable, per call. Reclaimed by output trimming, reversible via `ctx_expand`, earned via the causal gate.
- **Input tax (this plan).** The tool *menu*, every connected server's full schema list, loaded on every request. Fixed, per request, paid before any work happens. Reclaimed by carrying only the tools a request actually needs.

They are symmetric, and ctx should manage both sides of the window with the same See/Save/Trust model. Output trimming proved the discipline; tool management applies it to the bigger, unmanaged half.

## Why this is a showstealer

- **It compounds harder than output trimming.** A trim saves once, on one result. Cutting a dead-weight server saves on *every request for the rest of the session*, and every session after. Fixed cost removed beats variable cost trimmed.
- **Nobody manages it.** Agent vendors ship the full menu because it is simplest and because more tools looks like more capability. No one shows you that you carry 40 Linear tools to use 4. The platforms hide the input tax exactly like they hide the output tax.
- **It is the neutral, local, cross-agent moat again.** The tool menu is the same problem in Claude Code, Cursor, and Codex. ctx already sits under all of them. This is the second reason to keep ctx open, not a footnote.
- **It closes the loop on the north star.** Tool-menu tokens reclaimed roll straight into net context reclaimed (WNAD). This is the lever that moves WNAD from "a few thousand tokens short" to comfortably net-ahead, every week, on fixed savings that do not depend on how much output a session happens to produce.

## The safety problem, and how we hold the line

Pruning a tool is not the same risk as trimming a result, and pretending otherwise would break the trust output trimming earned.

- A trimmed result is reversible (`ctx_expand`) and the tool still *runs in full*. Worst case is a cheap round trip.
- A pruned tool is *removed from the agent's menu*. If ctx guesses wrong, the agent cannot call it, and there is no verbatim original to recover. A server used once a month looks like pure waste over a 30-day window.

So the input side needs its own reversibility and its own earn-it gate, mirroring the output side:

- **Reversibility = soft mode + auto-restore.** Default to shrinking the *presented* menu, not disconnecting servers. A tool the agent reaches for is brought back for the session (the existing `ctx filter expand`), the input-side equivalent of `ctx_expand`. Nothing is truly gone.
- **Relevance, not blanket removal = the learned model.** ctx already ships `semantic_tool_mix`: it embeds the session and carries the top-K relevant tools (Figma for a design turn, Linear for a planning turn). This is the continuously-learning vector model, present but switched off. Relevance per session beats "unused in 30 days."
- **Harm signal = the tool miss.** The input-side re-read: the agent tries to call a tool that was hidden or pruned. Deterministic to detect (an invocation of a tool not in the request's carried set). This is the metric an auto-prune must not raise above baseline.
- **Earn-it gate.** A server is auto-pruned only when the developer's own usage proves it is dead weight AND hiding it did not raise the tool-miss rate, the exact shape of the causal gate that earns output trimming. Never silently remove a capability the way ctx never silently loses a result.

## The maturity ladder

Parallel to the output ladder (L0-L5), so progress is measured on data, not vibes.

- **T0 Instrumented.** Every request records the tools it carried and which it invoked. Test: loaded-vs-invoked recoverable per server. Status: done (`requests`, `tool_invocations`).
- **T1 Legible.** The developer can see the input tax. Test: a Tool Menu Bill itemizes per-server tokens loaded vs invoked, dead weight ranked, in the first screen. The input-side Context Bill.
- **T2 Safely reclaiming.** Test: soft-mode semantic tool mix on, tool-miss rate at or below baseline with reversibility (session expand) working. The learned model shrinks the menu without capability loss.
- **T3 Earned auto-prune.** Test: per-server earn-it gate fires; ctx auto-prunes a server proven dead weight for this developer, tool-miss stays at or below baseline, and a reach re-adds it.
- **T4 Managed default.** Test: tool-menu tokens reclaimed per request sustained across more than one machine, and it shows up in WNAD as fixed savings.

Today: solidly T0, with a running start on T2 (the model exists, off). Phase 1 gets to T1, Phase 2 to T2, Phase 3 opens T3.

## KPIs

- **North star contribution:** tool-menu tokens reclaimed per request, which folds into net context reclaimed (WNAD). Fixed savings, so it compounds every turn.
- **Input capture rate:** dead-weight tokens removed / dead-weight tokens carried, per server.
- **Tool-miss rate (guardrail):** invocations of a tool not carried that request. Must stay at or below the developer's baseline, or the prune is not earned. Any regression vetoes it, exactly like the output harm gate.
- **Reversibility service rate:** session re-expands served / requested = 100%, at low latency.
- **Coverage:** servers and tools under management per developer, per agent.
- **Local invariant:** bytes leaving the machine = 0. Unchanged.

## What already exists (the running start)

This is not a greenfield build. Present today, mostly off or manual:

- **Waste detection.** `zero_usage_servers(days)` finds servers loaded but never invoked in the window. `ctx_waste` surfaces them.
- **The learned relevance model.** `semantic_tools.rs`: `semantic_tool_mix` embeds the session with the shipped MiniLM model and carries the top-K relevant tools, in Soft filter mode. Implemented, gated off.
- **Auto-profile generation.** `upsert_personal_from_usage` builds a lean profile from tools actually invoked (>=3 calls in 30 days).
- **The prune mechanism.** Filter modes (soft shrinks the menu, strict disconnects), profile deny rules, `ctx filter expand` for session restore.
- **The instrumentation.** `requests` records `tools_sent_count`, `kept_servers`, `removed_servers`, `mcp_tools_invoked`, `tools_sent_by_server`; `tool_invocations` records every call.

The work is wiring, measurement, and the earn-it discipline, not inventing the mechanism.

## The plan

Four phases, mirroring the output-side sequence. Each epic names what it touches and the gate that closes it.

### Phase 1: See the input tax

**M-A. The Tool Menu Bill.** The input-side Context Bill. Per server: tokens of tool schemas carried per request, calls actually made, dead-weight ratio, ranked biggest tax first. Built from `requests` + `tool_invocations`, no new tracking.
- Touches: new `/api/context/tool-bill`, a dashboard view, `src/db.rs` aggregation.
- Gate: the bill shows real per-server carried-vs-invoked tokens and names the biggest dead weight, on this machine's history.

**M-B. One-click reversible prune.** Turn `zero_usage_servers` into a proactive recommendation: "Linear carries 40 tools every request; you have invoked 4 in 21 days. Prune it, save ~N tokens/request. Reversible." One click applies a deny rule; a reach re-adds it. Counts as a real insight-action.
- Touches: dashboard insight, `src/filter_control.rs`, `src/profiles.rs`.
- Gate: a dead-weight server is pruned in one click, the saving shows per request, and a session expand restores it.

### Phase 2: Save safely, with the learned model

**M-C. Turn on the semantic tool mix (soft, reversible).** Enable the per-session relevance model in Soft mode so the carried menu shrinks to what the turn needs, servers stay connected, hidden tools re-expand per session. Ship sane defaults (top-K, min similarity) tuned on real sessions.
- Touches: `src/semantic_tools.rs`, config defaults, `src/filter_hook.rs`.
- Gate: on real sessions the carried tool count drops materially with the tool-miss rate at or below baseline.

**M-D. The tool-miss harm signal.** Detect and record the input-side re-read: an invocation of a tool not carried that request. The metric every prune is judged against.
- Touches: `src/db.rs` (join over `requests.tools_sent_by_server` vs `tool_invocations`), the hook path.
- Gate: tool-miss rate computed per server on real data, with a documented baseline.

### Phase 3: Earn the auto-prune

**M-E. Per-server earn-it gate.** The causal gate for tools: auto-prune a server only when usage proves it dead weight AND hiding it did not raise tool-miss above baseline, with the same confidence discipline as output activation. Reversible, fail-closed.
- Touches: `src/compress/activation.rs` sibling for tools, `src/db.rs`, the filter path.
- Gate: a server auto-prunes on real data at a documented tool-miss precision, and a reach re-adds it.

**M-F. Fold input savings into WNAD and the scoreboard.** Tool-menu tokens reclaimed count as net context reclaimed. The net-ahead scoreboard shows both taxes: output trimmed and input pruned.
- Touches: `src/db.rs` (`weekly_net_ahead`), the Home scoreboard.
- Gate: the scoreboard's reclaimed figure includes input savings, labeled, and WNAD moves on fixed savings.

### Phase 4: The unified context console

**M-G. One view over both taxes.** The dashboard headline becomes "ctx manages both sides of your context window": the output bill and the input bill side by side, one reclaimed number, one harm read. The showstealer, made legible.
- Touches: the dashboard IA, Home.
- Gate: a developer opens ctx and sees both taxes, what was reclaimed on each, and the single net-ahead verdict.

## What we are not doing

- Silent auto-prune without an earned gate. It removes capability with no recovery signal; it is the one thing that would break the trust output trimming earned.
- A cloud model or per-request LLM call to pick tools. The local embedding model is the ceiling; no bytes leave the machine.
- Chasing "more tools is more capable." The whole point is that a smaller, relevant menu is a *better* agent, not a weaker one, because it is not drowning in a catalog it never uses.
