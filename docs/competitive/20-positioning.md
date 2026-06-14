# Positioning: where ctx wins, where it loses, and the one position to take

Last updated: 2026-06-13. The strategic decision behind this doc is recorded in ADR 0013.

## The top-line answer (read this first)

Where ctx honestly wins: on the surfaces RTK and the platforms miss (native Read, Grep, Glob, and MCP tool outputs), and on a claim no competitor makes (per-user proof that trimming did not cost you corrections). ctx is the only thing in the scan that acts on context and then proves, from your own work, that it was safe.

Where ctx loses today: on Bash command output (RTK is better and far better known), on raw "free and default" (the platforms ship native compaction for nothing), and on distribution and maturity (RTK has 62,127 stars; ctx has one-machine evidence).

The one position to take: ctx is the context truth and safety layer for AI coding agents. It trims the native tool output and MCP noise the platform and RTK miss, on Claude Code and Cursor, and only after it proves, on your own work, that trimming did not cost you corrections.

## The wedge, stated plainly

Two structural openings, both verified in the briefs:

1. The surface gap. RTK's hook fires only on Bash (RTK's own docs). Native Read, Grep, Glob, and MCP tool results bypass it. On Cursor and default Claude Code, that is where the tokens go. ctx trims exactly there, after the tool runs, keeping errors and the lines that matter.

2. The proof gap. Every competitor either grades its own work (the platforms), grades a benchmark (Kompact on BFCL, Claw Compactor on ROUGE, LLMLingua on research sets), or grades an offline eval (Langfuse, Braintrust). None watches whether the human had to fix the agent. ctx does: it ties trims to the user's own re-reads and corrections and only trims a tool for real once it has earned it (the causal gate, ADR 0006 and 0012).

The platform compacts for the median user. ctx adapts to your repo and your work, and only acts when earned. That is the thesis, and it survives the competitive pass.

## One-line position

ctx is the context truth and safety layer for AI coding agents: it cuts the tool-output and MCP tokens your platform and RTK miss, and only after it proves on your own sessions that trimming did not cost you corrections.

## Three messaging pillars

1. The open lane. "RTK rewrites your shell commands. Your agent reads files with Read, searches with Grep, and calls MCP tools, and none of that goes through RTK. ctx trims those, after they run, keeping the errors and the lines that matter."

2. Earned, not assumed. "ctx does not trim a tool until it has shown, on your own sessions, that trimming did not make the agent re-read or correct. New tools earn it in a short, bounded burn-in. Clean tools keep trimming; harmful ones stop."

3. Yours, local, honest. "Everything stays in SQLite on your machine. No account, no telemetry. The dashboard shows observed savings and where the loop is still learning, never a placeholder."

## The honest claim ctx can make today

- "On the developer's own machine, ctx saved about 343,000 tokens with about 0% corrections and about 5% re-reads on the trimmed calls it could verify." Always labeled observed, early, and single-machine. This is a credible direction-of-travel, not a general result.
- "ctx trims native Read, Grep, Glob, and MCP outputs that RTK's Bash hook does not reach." This is verifiable from RTK's own documentation.
- "ctx only acts after a causal before-and-after check on your own data." This is the architecture, shipped (ADR 0012).

## The claims ctx must not make yet

- Do not claim a general savings percentage. The 343K figure is one machine. No "save 60-90%" headline; that is RTK's claim and ctx has not earned a number.
- Do not claim "proven safe" for tools still in burn-in. Only earned tools may carry proof language (ADR 0012).
- Do not claim ctx beats native compaction on quality. ctx has not measured that yet; the compaction-harm detector is the path to earning that claim (roadmap E2.1).
- Do not claim Codex or Gemini support. Not built yet.
- Do not claim ctx makes the agent smarter. It protects against degradation; it does not improve the model.

## Where ctx wins and loses, per competitor (summary)

| Against | ctx wins on | ctx loses on |
| --- | --- | --- |
| RTK | Native tool output, MCP, per-user proof, edit-intent guard, zero telemetry | Bash command quality, distribution, framing |
| Claude Code native | Cross-agent, proof, per-tool earning, measuring compaction harm | Free, default, first-party, server-side |
| Cursor native | Per-tool and per-MCP-tool granularity, proof, edit guard | No live hook on Cursor; platform can absorb |
| Codex / Gemini native | Content-aware vs flat cap, proof, cross-agent | No ctx surface there yet; free and default |
| Kompact / Claw / LLMLingua | Proof on real behavior, lighter trust, edit guard | Their transforms are more sophisticated than ctx's today |
| MCP gateways | Turn-free static stripping, part of a fuller layer | They scale better for huge catalogs; offer governance |
| Memory / packers / observability | Different problem; complementary | n/a (not competing) |

## What to do with this

- Lead every surface (README, dashboard, launch posts) with pillar 1 (the open lane) and pillar 2 (earned). Use the 343K number only with its label.
- Never open with a savings percentage. Open with the surface gap and the proof gap.
- Cross-link: positioning rests on ADR 0013; the brutal version of the threats is in 60-threats-and-moats.md; the go-to-market is in 70-gtm-implications.md.
