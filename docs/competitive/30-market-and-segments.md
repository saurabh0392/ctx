# Market and segments

Last updated: 2026-06-13. This is the PM view: the real pain, who feels it, what job they hire a tool to do, where the market is going, and a rough size sketch with every assumption labeled.

## The real buyer pain (three layers)

1. Token cost. Agentic coding burns tokens fast. A single `git diff`, a large file read, or a noisy test run can cost thousands of tokens, repeated dozens of times a session. RTK's own example session estimates about 118,000 tokens of command output in 30 minutes (vendor estimate). Cost is the pain people name first.

2. Context-window pollution and quality decay. This is the deeper pain. As context fills, the model loses focus, forgets early instructions, and re-reads files. Cursor's own staff and third-party guides describe quality degrading after roughly 20-30 exchanges (community-observed, not a controlled measurement). Anthropic frames compaction as being about focus, not just cost. The expensive failure is not the token bill; it is the agent making a mistake the human has to catch and fix.

3. Trust. The moment a tool silently drops something the agent needed, the developer stops trusting it and turns it off. Every compaction and compression tool faces this, and most have no answer beyond "it benchmarks well." This is the pain ctx is built around.

## Ideal customer profile

The ICP is a developer or team that runs AI coding agents heavily enough that token cost and context decay are daily, felt problems, and who cares whether the tool is honest. Concretely: people already running Claude Code or Cursor for hours a day, often with MCP servers connected, who have felt the agent get dumber in a long session.

## Personas (three)

1. The solo AI-heavy developer (primary, now).
   - Who: an individual power user of Claude Code or Cursor, possibly paying for API usage directly, technically comfortable installing a local tool.
   - Job-to-be-done: "Make my agent cheaper and keep it sharp in long sessions, without me having to babysit it or worry it is quietly cutting corners."
   - Why ctx: local, no account, trims the native tools they actually use, and proves it is not hurting. This is who RTK won first, and who ctx can win on the surface and proof gaps.
   - Buying motion: self-serve, open source, word of mouth, GitHub.

2. The platform / AI-tooling team (next).
   - Who: a small team inside a company responsible for the internal AI coding setup, MCP servers, and cost.
   - Job-to-be-done: "Cut our org's agent token spend and give us a defensible, honest view of whether context tooling is helping or quietly degrading our engineers' agents, across the different agents people use."
   - Why ctx: cross-agent normalization, the compaction-harm measurement, and a portable per-repo policy (roadmap E3.1). The proof story is what a team lead can defend to their own management.
   - Buying motion: champion-led, starts with the OSS tool, grows into a team artifact.

3. The enterprise procurement buyer (later, and only if the proof and trust story is real).
   - Who: a security- and cost-conscious buyer evaluating AI tooling across an org.
   - Job-to-be-done: "Reduce model spend at scale with something auditable, that does not exfiltrate code, and that we can prove is not degrading developer output."
   - Why ctx: local-first, no telemetry, no account, and an honest measurement layer they can audit. The gaps ctx must close for this buyer are governance (RBAC, audit), which the MCP gateways already offer, and a hosted or team-deployable form.
   - Buying motion: procurement, security review, pilot with measured results.

## Jobs-to-be-done (the JTBD ladder)

- Functional: "Spend fewer tokens per task." (RTK, platforms, proxies all serve this.)
- Functional, deeper: "Keep the agent accurate in long sessions." (Compaction and packers serve this partly; nobody proves it.)
- Emotional: "Let me trust the tool not to silently break my agent." (Only ctx's proof story serves this directly.)
- Social: "Let me show my team or my manager that our AI tooling is honest and working." (The measurement and portable-policy story serves this; nobody else does.)

ctx should sell up the ladder. The functional rung is crowded and commoditizing. The emotional and social rungs are open.

## Adoption trends (observed and dated)

- Token-saving tooling is having a moment. RTK went from about 28,000 stars (Medium blog, April 2026) to 62,127 (GitHub API, 2026-06-13). That is real, fast demand for "cut my agent's tokens."
- The platforms are racing to ship native context management: Claude Code compaction and context editing, Cursor's context ring (Cursor 3.3), Codex auto-compaction, Gemini CLI. Free context management is becoming table stakes.
- MCP tool sprawl is a recognized, named problem (Cursor's context ring breaks out the MCP catalog as a line item), and search-first discovery is consolidating fast, with Claude Code doing it natively.
- The research and product frontier moved from "compression ratio" to "tokens-to-complete-task" and outcome (strategy doc cites Factory.ai, Slipstream, ProcCtrlBench; these are internal-strategy citations, not re-verified in this pass). Outcome-over-ratio is becoming consensus, which is tailwind for ctx's proof framing.

## Rough market-size sketch (all numbers are estimates, labeled)

There is no clean public TAM for "AI coding agent context optimization," so this is a sketch of orders of magnitude, not a number to quote to investors as fact.

- Population (estimate): AI coding agents are used by a large and growing slice of professional developers. The addressable group for ctx is narrower: heavy daily users of Claude Code and Cursor who feel cost and context pain. Order of magnitude: hundreds of thousands to low millions of developers (estimate, derived from agent adoption being mainstream among professional developers; not from a cited market report).
- Willingness to pay (estimate): the token-saving category is currently almost entirely free and open source (RTK, the proxies, the gateways). Direct willingness to pay for a savings tool is unproven and probably low for individuals. Teams and enterprises will pay for governance, proof, and spend reduction at scale.
- Value basis (estimate, and the honest framing): the value is a fraction of model spend saved plus the avoided cost of agent mistakes. For a heavy user spending, say, hundreds of dollars a month on agent tokens, even a modest verified reduction plus fewer corrections is a real but not enormous individual ROI. The bigger value is at team and org scale.

Conclusion from the sizing sketch: this is more likely an open-source land-grab with a team and enterprise monetization layer than a high-ACV individual product. See 40-pricing-and-business-models.md.

## What this means for ctx

- Win the solo AI-heavy developer first, the same beachhead RTK took, but on the surfaces and proof RTK skips.
- Build toward the team persona with the measurement and portable-policy story, because that is where willingness to pay and a defensible moat both live.
- Do not price or position as if individuals will pay a lot for savings. They will not; the category is free. Sell trust and proof, and monetize teams.
