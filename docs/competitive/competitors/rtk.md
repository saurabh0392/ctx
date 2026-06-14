# RTK (Rust Token Killer)

- Category: tool-output / command compressor
- Classification: direct competitor (primary)
- One-liner: a single Rust binary that intercepts shell commands before they run and returns command-aware compact output, cutting tokens 60-90% per command.
- URL / repo / docs: https://github.com/rtk-ai/rtk, https://www.rtk-ai.app
- Maturity signals: 62,127 GitHub stars (observed, GitHub API, 2026-06-13). License Apache-2.0. Latest version 0.28.2 (vendor README, accessed 2026-06-13). Last repo push 2026-06-12 (observed, GitHub API). On Homebrew (`brew install rtk`). Active Discord. Founder Patrick Szymkowiak plus two core contributors (vendor README). Funding: none disclosed (open source project).
- License / pricing: Apache-2.0, free. No paid tier observed.

## What it does and how (mechanism)

RTK sits in the request path as a PreToolUse hook. It rewrites the command before execution. When the agent asks to run `git status`, the hook calls `rtk rewrite` and the command becomes `rtk git status`. The agent then runs the rewritten command, RTK runs the real command in full, filters the output, and only the compact output reaches the model. Overhead is claimed under 10ms (vendor).

The leverage is roughly 100+ hand-written, command-aware filters across git, test runners (jest, vitest, pytest, cargo, go), linters, package managers, AWS, docker, kubectl, and more. Four named strategies per command type: smart filtering (drop noise), grouping (aggregate similar items), truncation (keep relevant context), and deduplication (collapse repeats with counts). When a command fails, a tee layer saves the full unfiltered output to disk so the model can read it without re-running. Tee defaults to on, mode "failures".

Surfaces: 14 agents (vendor README). Claude Code, Cursor, Gemini CLI use real command-rewriting hooks. Copilot, OpenCode, Pi, Hermes use plugin or extension rewrites. Codex, Windsurf, Cline, Roo Code, Kilo Code, Antigravity get prompt-level instructions only (a rules file telling the agent to prefer `rtk`), which is guidance, not interception.

Critical scope fact, stated in RTK's own README: the hook only fires on Bash (terminal) tool calls. Claude Code built-in tools `Read`, `Grep`, `Glob`, and MCP tool results do not pass through the hook and are not rewritten. To get RTK on those, the user or agent must call `rtk read` / `rtk grep` / `rtk find` explicitly, or route work through the shell.

## Claimed results vs verifiable results

- Claimed (vendor): 60-90% token reduction per supported command; a 30-minute Claude Code session table totalling about 80% reduction (118,000 tokens to 23,900). The README labels these "Estimates based on medium-sized TypeScript/Rust projects. Actual savings vary." Source: RTK README, accessed 2026-06-13. Label: vendor claim, self-labeled as estimate.
- Verifiable: the 62,127 star count and Apache-2.0 license are observed via GitHub API (2026-06-13). The mechanism (PreToolUse rewrite, Bash-only scope, tee recovery) is documented in the repo and is verifiable by reading the source. Label: observed for adoption and mechanism.
- Not verifiable in this pass: independent, third-party measurement of end-task quality after RTK filtering. RTK does not publish a downstream task-accuracy benchmark (for example, did filtered output cause the agent to re-run commands or make mistakes). The savings numbers are per-command output reduction, not net tokens-to-complete-task. Logged in 90-open-questions.md.

## Strengths

- Distribution and framing. `brew install rtk`, one binary, a memorable name, and a clean "kill tokens" story. 62K stars is real momentum.
- Command-aware filters beat generic trimming on shell output. A hand-written git or pytest filter understands structure ctx's heuristics do not.
- PreToolUse rewrite means the noise never enters context at all, and never hits the user's transcript either. Cleaner than trimming after the fact.
- Tee recovery is a sane safety net: full output on failure, retrievable without a re-run.
- Breadth: 100+ commands, 14 agents, cross-platform.

## Weaknesses and blind spots

- Bash-only interception. Native `Read`, `Grep`, `Glob`, and MCP tool results bypass the hook entirely. On agents that prefer native file tools (Cursor, Claude Code default behavior), a large share of token spend is untouched unless the user changes how they work.
- No per-user proof. RTK trusts its curated filters. It does not measure, on your sessions, whether a filter made the agent re-read or correct. If a filter drops a line your workflow needed, RTK has no feedback loop to catch it. The defense is tee recovery, which only fires on command failure, not on silent quality loss.
- Guidance-only on several agents (Codex, Cline, Roo, Windsurf). A rules file asking the agent to "use rtk" is unreliable; the agent often ignores it.
- Opt-in telemetry exists. Off by default, consent required, aggregate only (vendor README). Lower trust cost than most, but not the zero-telemetry posture ctx holds.

## Overlap with ctx

High on the shell/Bash surface. Both reduce tool-output tokens locally with no account. Both keep errors and the lines that matter. Both have a full-output recovery story (RTK tee on failure; ctx full output stays in the transcript, plus the planned rewind store).

The overlap stops at the surface boundary. RTK's value is concentrated on terminal commands. ctx's PostToolUse trimming covers native `Read`, `Grep`, `Glob`, and MCP results that RTK structurally cannot reach without the user re-routing work through the shell.

## Where ctx is better / where it is worse

Better:
- Coverage of native tools and MCP outputs, which is where Cursor and default Claude Code spend tokens. This is the open lane.
- Per-user causal proof. ctx only trims a tool "for real" after it has shown, on the user's own sessions, that trimming did not raise re-reads or corrections. RTK ships curated filters and trusts them.
- Edit-intent guard: ctx will not trim a read the agent signaled it will edit. RTK has no equivalent because it does not trim native reads at all.
- Zero telemetry, no account, local SQLite.

Worse:
- Distribution, mindshare, and momentum. RTK has 62K stars and the "token killer" framing. ctx has neither yet.
- Command-aware quality on shell output. RTK's 100+ hand-written filters produce tighter, structure-aware shell output than ctx's heuristic trim. On Bash specifically, RTK is likely better per command.
- Simplicity of the story. "Install one binary, save 80%" is easier to sell than "a causal safety gate that earns trimming per tool".

## Threat level to ctx: high

It owns the category framing ctx is adjacent to, has real distribution, and is expanding agent coverage (it already ships a Cursor hook). If RTK adds a credible native-Read and MCP-output story, it closes ctx's open lane. The mitigation is that RTK's architecture (PreToolUse command rewrite) does not naturally extend to native tool results, and it has shown no interest in per-user proof, which is ctx's distinct claim.

## What ctx should learn or steal

- Command-aware filters for shell output. ctx's Bash trimming should adopt structure-aware filters for the common commands (git, test runners, linters) rather than generic heuristics. This is a feature gap, not a strategy conflict.
- The distribution playbook: `brew install`, one-line setup, a single sharp number, a recovery command (`rtk gain`). ctx's setup and dashboard should be as frictionless.
- Tee-on-failure is a good pattern. ctx already keeps full output in the transcript; making recovery one step (the planned rewind store) matches and arguably beats it.
- Do not fight RTK on Bash framing. Position on the surfaces and the proof it skips. See 20-positioning.md and ADR 0013.
