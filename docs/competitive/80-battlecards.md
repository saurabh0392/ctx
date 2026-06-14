# Battlecards

Last updated: 2026-06-13. One short card per top competitor for community and sales conversations. Each card: their pitch, our counter, traps to avoid, and proof points (with honesty labels). Keep the tone respectful. Confidence beats trash talk.

## RTK (Rust Token Killer)

- Their pitch: "One Rust binary, `brew install rtk`, cuts your agent's tokens 60-90% per command. 62K stars."
- Our counter: "RTK is genuinely good on shell commands. But its hook only fires on Bash, by its own docs. Your agent reads files with Read, searches with Grep, and calls MCP tools, and none of that goes through RTK. ctx trims exactly those, and only after it proves on your own sessions that trimming did not make the agent re-read or correct."
- Traps to avoid: do not argue Bash quality (you lose; RTK's filters are better). Do not claim a savings percentage you have not earned. Do not disparage RTK; it is well-built and popular.
- Proof points: RTK's own README states the Bash-only scope (verifiable). ctx covers native Read, Grep, Glob, MCP outputs (observed in product). ctx adds the edit-intent guard and per-user causal proof (architecture, shipped). The 343K-token live result is observed but early and single-machine; present it that way.

## Claude Code native (compaction, context editing)

- Their pitch: "It is built in, free, server-side, and improving. You do not install anything."
- Our counter: "Claude's compaction is good and free, and you should use it. But it grades its own homework. When it drops something the agent later needs, the failure is silent. ctx measures whether your context interventions, including Claude's own compaction, preceded corrections in your real work, and it does that across Cursor too, which Anthropic cannot see."
- Traps to avoid: do not claim ctx beats native compaction on quality (not measured yet). Do not position as a replacement for compaction; position as the proof and cross-agent layer on top.
- Proof points: Anthropic publishes no per-task quality delta for compaction (verifiable absence). ctx is cross-agent by design. The compaction-harm detector is the roadmap feature that makes this concrete (label: planned, E2.1).

## Cursor native (summarization, context ring)

- Their pitch: "Cursor summarizes long chats automatically and shows you exactly where your tokens go."
- Our counter: "The context ring is great, and it even shows you the MCP catalog cost. But summarization loses detail (Cursor's own users report re-explaining context), and your only levers are summarize or start a new chat. ctx trims individual tool outputs and filters individual MCP tool schemas, finer than a whole-server toggle, and it tells you whether a trim cost you a correction."
- Traps to avoid: do not claim a live Cursor hook (ctx uses transcript ingest today). Be honest that Cursor labels are lower-confidence.
- Proof points: Cursor's context ring itemizes MCP catalog cost (verifiable, Cursor 3.3). ctx's edit-intent guard now runs on Cursor (ADR 0011). Per-MCP-tool filtering vs Cursor's per-server toggle (product).

## Codex / Gemini CLI native

- Their pitch: "Auto-compaction is built in. Codex caps tool output; Gemini preserves the recent tail."
- Our counter: "Both use blunt levers: Codex's `tool_output_token_limit` is a flat cap that cuts by token count, not by what matters, and Gemini's fallback just truncates. ctx keeps the errors and the lines that matter, and only trims a tool after it earns it. Today ctx leads on Claude Code and Cursor; we add agents as real usage shows up."
- Traps to avoid: do not claim Codex or Gemini support (not built). Keep it about the mechanism contrast.
- Proof points: Codex's flat cap and no-off-switch are documented (verifiable, codex issues and config reference). Content-aware vs flat cap is the contrast.

## Kompact / Claw Compactor / LLMLingua (proxies and libraries)

- Their pitch: "Drop-in compression, 40-82%, benchmarked on BFCL or ROUGE."
- Our counter: "Benchmarks are not your workflow. A compressor can score better on ROUGE and still make the agent solve fewer problems (the ICAE/SWE-bench result). ctx measures the only thing that matters: did you have to fix the agent's work. And ctx does it with hooks alone, no MITM proxy in your request path at all."
- Traps to avoid: do not dismiss their transforms; Claw Compactor's content-aware stages are genuinely good and worth learning from. Do not overclaim ctx's transform sophistication.
- Proof points: ROUGE and BFCL are not task-completion (cited in strategy doc). ctx's label is production behavior. ctx's trust posture is lighter (hooks only, no proxy, no CA, never edits the request on the wire) than a mandatory full-API MITM.

## MCP gateways (search-first tool discovery)

- Their pitch: "Replace your whole tool catalog with 4 search tools. 90-97% fewer schema tokens."
- Our counter: "Search-first is great for huge catalogs, but it costs extra turns and latency on every first use of a tool, and Claude Code now does it natively. ctx strips the schemas you rarely use with no extra turns, as part of a layer that also trims your tool outputs and proves it was safe, which a schema router does not touch."
- Traps to avoid: do not claim ctx scales better for very large catalogs (it does not; search-first wins there). Position MCP filtering as a supporting feature.
- Proof points: search-first adds turns (the projects say so themselves). Claude Code absorbs it natively (StackOne, 2026-06-13). ctx's static stripping is turn-free.

## A note for whoever uses these cards

Every number with a label is the contract. "Observed, early, single-machine" is not optional fine print; it is the product's whole identity. The fastest way to lose a skeptical developer is to present the 343K figure as a general result. Lead with the structural gaps (which are verifiable from competitors' own docs), and let the early numbers be early.
