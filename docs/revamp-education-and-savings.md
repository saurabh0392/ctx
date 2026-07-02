# ctx revamp: the honest pivot to context education and savings

Status: draft for discussion
Date: 2026-07-02
Owner: Saurabh Sharan
Supersedes: the headline framing of `strategy-context-truth-layer.md` (the "truth layer" pitch). Keeps its competitive research and its kill criteria.
Companion: `roadmap-context-truth-layer.md` (the horizon epics survive as backlog; this doc re-sequences them).

## The one paragraph

ctx was built to prove, from your own sessions, that trimming context did not make your agent worse. After 25 days of running it on a real machine, the evidence for that claim is not there: across 2,731 joined decisions, the correction detector fired zero times. Not because trimming was flawless, but because the signal the whole thesis rested on almost never fires. Meanwhile the parts nobody led with quietly worked: ctx removed about 475K tokens of tool output on one machine, and its "what did the agent actually need whole" model reached a holdout AUC of 0.89. The revamp keeps what the data supports and drops what it does not. ctx becomes a context education and savings tool: the local, neutral thing that shows you where your agent's context goes and buys back the room it can safely reclaim. The proof-of-safety story steps down from headline to background check.

## Vision: the holy grail

Context is the scarcest and least understood resource in agentic coding. Agents drag it around, lose it in compaction, and spend it on tool output nobody reads. Today it is a black box: you cannot see where it goes, and you cannot tell whether anything cutting it is helping or hurting.

The grail: ctx is the context console every AI-coding developer keeps open. It shows exactly where your agent's context goes, reclaims the room it can prove is safe, and does it across every agent you use, locally, taking no side. The htop for context. Once a developer has seen their own Context Bill, flying blind feels wrong.

Three altitudes so the word "vision" means something:
- Mission (why): make context a legible, managed resource instead of an invisible tax.
- Vision (the grail): the default context console for agentic coding, trusted because it is local and neutral.
- Wedge (how we start): the honest pivot to See, Save, Trust.

We are close when a developer opens ctx to understand their own workflow, learns something no other tool could show them, and their agent runs leaner without running dumber.

## KPIs: how we know we're winning

One number on top, a small tree under it, a guardrail that can veto a "win," and a maturity ladder so progress is measured, not vibed.

### North star: Weekly Net-Ahead Developers (WNAD)

A developer-week is net-ahead when, that week, ctx reclaimed a meaningful share of that developer's context (target: at least 50K tokens or 25% of eligible, whichever is lower) AND the harm rate on earned tools stayed at or below that developer's own baseline. One number that only moves when adoption, savings, and safety all land for a real person. Corrections cannot be gamed into it; a harm regression cancels the week.

Baseline today: one machine, and even it is not cleanly net-ahead, because Read's harm rate is 26% (CTX-51 is the fix). Honest starting line: 0.

### Input KPIs, by pillar

**See (education)**
- Bill coverage: share of a session's tool-output tokens ctx can itemize in the Context Bill. Target 90%+.
- Insight engagement: weekly Context Bill opens per active developer, and insight-actions taken (pruned an MCP server, split a session, kept a tool trimmed). Education only counts when it changes behavior.
- Time to first insight: install to first populated bill. Target under 5 minutes.

**Save**
- Net context reclaimed per active developer per week (tokens kept out of the window, compounding). Reference point: ~475K over 25 days on one machine.
- Capture rate: reclaimed / eligible, per tool. Read is ~20% of its own pool today, the single biggest lever.
- Reversibility service rate (after CTX-51): re-expand requests served / requested = 100%, at low latency.

**Trust (guardrails: any of these can veto a net-ahead week)**
- Harm rate per earned tool: re-read rate, plus correction rate once the signal is real. Must stay at or below baseline. Reference: Bash 0.76% (earned), Read 26% (correctly held).
- Reversible-trim share: fraction of trims that can be re-expanded. Target 100% on Read.
- Cross-agent coverage: agents ctx sees per developer (Claude Code, Cursor, Codex).
- Local invariant: bytes leaving the machine = 0. Not a trend, a promise. 100% or the product is broken.

**Adoption (only if we take the product exit)**
- Weekly active machines, week-4 retention, tools earned per install, GitHub stars (RTK sits at 62K as a scale marker, not a target).

**Narrative (the portfolio exit)**
- The honest proof points stay true and cited (reclaimed tokens, model AUC, the pivot itself); if published, the post's reach and the conversations it starts.

### The maturity ladder (progress toward the grail)

Each level has one entry test. You are at a level when its test passes on real data, not when the code merges.

- L0 Instrumented: every compressible tool result is recorded. Test: decisions logged across Bash, Read, Grep, Glob, MCP. Status: done.
- L1 Legible: a developer can see where their context goes. Test: the Context Bill renders real per-tool sinks in the first screen (CTX-49, CTX-50).
- L2 Safely reclaiming: savings without harm, reversible. Test: Read capture rate rising and effective harm at or below baseline with reversibility on (CTX-51, CTX-52).
- L3 Multi-agent truth: one honest console across agents. Test: two live agents render side by side from one machine (CTX-53, CTX-54, CTX-55).
- L4 Trusted default: developers keep it open and it changes behavior. Test: WNAD above 0 sustained across more than one machine, insight-actions per active developer above 0 (CTX-56).
- L5 The console: category-defining. Test: developers describe ctx as where they go to understand their context, and WNAD grows without hand-holding.

Today: solidly L0, reaching for L1. Phase 1 gets us to L2, Phase 2 to L3, Phase 3 opens L4. L5 is the grail and is not something you build, it is something the first four levels earn.

## What the data actually says

Everything below is from the live `~/.ctx/ctx.db` and `retention-model.json` on 2026-07-02, one developer, 100 sessions, 20 sessions of compression decisions spanning 2026-06-07 to 2026-07-02.

**Working, and worth protecting.**
- Real savings. `compress_events` shows about 1.9M characters removed, roughly 475K tokens. Read alone is 1.48M characters. This is applied output, not a projection.
- A real model. `needed_whole` predictor, version 8, holdout AUC 0.89 on 1,474 rows, `kind_only_auc` 0.885. The the-gaffer repo already has 102 positive labels and reads `ready: true`. As a "do not trim what the agent needs in full" model, this is legitimately good.
- Rare intellectual honesty. Earned-only savings, "none yet" instead of a fake number, a compaction view that tells the user "this is not proof," no telemetry, fail-closed activation, 35 ADRs with real kill criteria. This discipline is a trust asset and a differentiator. Keep it.

**Hollow, and driving the wrong story.**
- Zero corrections. Across 2,731 joined decisions, `outcome_correction` is 0 everywhere. The gate is `explicit_complaint && applied && lines_drop > 0`, and CTX-48 through CTX-50 tightened it further to kill false positives. The steering you do constantly ("didnt revert back", "which one for b?", "clean enough?") almost never lands inside a post-trim window with complaint wording. The "we prove trimming did not cost you corrections" pitch is writing a check the data does not cash. What actually carries safety is the re-read signal and the `needed_whole` model, not corrections.

**Stuck, right where the value is.**
- Read is the prize and the problem. 6.15M characters in shadow, 1.48M already trimmed, but a 26% re-read rate, so the gate correctly refuses to earn it. Bash (0.76% re-read) sails through. The tool with the most savings is the one trimming hurts most. During this very analysis, ctx trimmed a 222-line model file down to one line when I tried to read it, and I had to read it again another way. That re-read is the exact harm the model counts. The failure is real and reproducible.

**Missing entirely.**
- No education. The dashboard is an audit tool for a skeptic: it shows what ctx did to each tool. It never teaches the one thing a developer does not understand, which is where their context goes and why the agent gets duller over a long session. The raw material is already in `compress_decisions`: Linear `list_projects` cost 112,732 chars and collapsed to 553, Figma `use_figma` is 98% trimmable, Read is the number one sink. That is an itemized context bill, and no one (not RTK, not Anthropic, not Cursor) shows it. The platforms actively hide it.

**The real moat, understated in the current pitch.**
- Neutral, local, cross-agent. ctx is the only thing that sits under Claude Code and Cursor (and later Codex) on your machine and tells you the truth about all of them at once. No agent vendor will ever grade its rivals, or itself, honestly. That is more durable than any correction gate. It should be the moat we lead the strategy on, not a Horizon 2 footnote.

## The reposition

From: "the context truth and safety layer" (measurement plus governance plus proof, sold on a signal that is not arriving).

To: "the context education and savings tool" (see where it goes, save what is safe, trust it because it is local and neutral).

Three verbs, in order of what a new user meets first:

1. **See.** Make context legible. The context bill, per tool, per MCP server, per repo, per session. Works on day one with zero labels. This is the new front door and the education product.
2. **Save.** Reclaim the room you safely can, and say so honestly. Reversible trims so aggressive Read compression stops being a risk. Savings framed as context reclaimed, which compounds, not as a tiny dollar figure.
3. **Trust.** Local, no telemetry, neutral across agents. The background check (did a trim ever correlate with harm) stays, but as a quiet safety mechanism, not the headline.

The safety-proof machinery is not deleted. It moves from the marquee to the engine room, where it belongs until it has signal.

## The product, revamped

What a developer gets, concretely:

- A **Context Bill** they actually want to open: "this week your agent read 6.1M characters of tool output; your top three sinks were Read, Linear list calls, and Figma; here is what ctx reclaimed and what is still on the table." Built from data ctx already stores.
- **Safe, reversible savings** on the big pools. Read gets trimmed hard, and when the agent needs the detail back it asks and gets the verbatim original from a rewind store. The 26% re-read stops being harm and becomes a cheap round trip.
- **One honest view across agents.** Claude Code and Cursor side by side, with Codex when it lands, and plain "not seen yet" where a surface is empty.
- **A context health read** on native compaction, presented as education ("your agent compacted 9 times this week; here is what tends to follow"), never as a causal proof claim.

## The dashboard: from audit readout to context console

Today the dashboard is an audit tool for a skeptic. It shows what ctx did to each tool, in a "watching, nothing changed yet" default that reads as broken to a new user. The revamp makes it the thing a developer opens on purpose: the htop for context.

The metaphor is a console, not a report. Three coupled changes:

- **Home is the Context Bill.** The first screen answers "where did my context go this week, and what did ctx buy back," not "here are three reasons to trust us." Trust is shown by being right, not by being claimed. (CTX-49, CTX-50)
- **The IA is See / Save / Trust at the top level.** See: the bill, sinks ranked, per repo and per session, plus a context-health trend so a developer can tell if their context is getting leaner or heavier week over week. Save: reclaimed plus what is still on the table, per tool, reversible. Trust: the cross-agent surfaces, the harm-versus-baseline read, the local invariant, compaction health. (CTX-52, CTX-53, CTX-55)
- **Time to first insight under five minutes.** On first ingest the dashboard lights up with a real bill, never an empty "watching" state. Drilling into a sink opens the actual trimmed-versus-original for a decision, which the rewind store makes possible. (CTX-57)

The dashboard is also where the KPIs live. Reclaimed, capture rate, harm against baseline, agents seen: the developer's own scoreboard, the same numbers that roll up to WNAD. There is prior design exploration to converge (`docs/prototypes/dashboard-revamp.html`, `learning-home.html`, `tools-page.html`); the revamp points them all at the console.

## The plan

Three phases. Phase 1 is the wedge and can ship on today's data. Phase 2 is the differentiation. Phase 3 keeps both the portfolio and the product exits open. Each epic names what it touches and the gate that closes it, matching the house roadmap style.

### Phase 1: the wedge (education, plus the savings we already earn)

**E-A. Reposition the surfaces.** Rewrite the README lead, the dashboard Home story, and the copy from "truth/safety layer" to "see, save, trust." Demote the corrections-proof language from every headline. Keep the honest empty states.
- Touches: `README.md`, `src/dashboard.html` (Home beats), `docs/` positioning.
- Gate: no user-facing surface leads with "we prove trimming did not cause corrections"; Home leads with the context bill.

**E-B. The Context Bill (the education front door).** A view that answers "where did my context go" from existing `compress_decisions` data: per-tool and per-MCP-server char counts, biggest sinks ranked, per-repo and per-session breakdowns, and "reclaimed vs still on the table." No labels required.
- Touches: new `/api/context/bill`, a new dashboard view, `src/db.rs` aggregation over `compress_decisions`.
- Gate: on a machine with history, the bill renders real per-tool sink rankings and a reclaimed-vs-available number, in the first screen a new user sees.

**E-C. Reversible Read compression (rewind store).** When ctx trims, store the original hash-addressed so the agent re-expands on demand. This turns Read's 26% re-read from harm into a cheap round trip and unlocks the single largest savings pool. Already scoped as SAU-147; this promotes it to the critical path.
- Touches: new `src/compress/rewind.rs`, apply path in `src/compress/hook_io.rs`, retrieval tool in `src/mcp.rs`.
- Gate: a trimmed Read block round-trips to its verbatim original; retrieval works from the agent side; Read's effective harm rate drops with reversibility on.

**E-D. Savings reframe: context reclaimed, honest potential.** Keep ADR 0006's earned-only truth, but pair it with the bill's "still on the table" so day one shows potential, clearly labeled, instead of a dead "none yet." Lead with tokens of room reclaimed and its compounding, dollars secondary and labeled.
- Touches: `/api/context/proof`, Home savings card, the new bill view.
- Gate: day-one dashboard shows a labeled potential figure alongside the honest earned figure; no blank dead-end.

### Phase 2: differentiate on the honest moat

**E-E. Cross-agent one-truth surface.** Make the neutral local view the centerpiece: Claude Code and Cursor together, Codex adapter when justified, honest "not seen yet" states. This is the durable moat.
- Touches: `src/surface/`, Surfaces view, `/api/context`.
- Gate: two live agents render side by side from one machine's data with honest empty states.

**E-F. Fix or retire the correction signal.** Either wire the coaching detector's steer signals (which already catch your real corrections) into the compression join so corrections actually fire with defensible precision, or formally retire the corrections-proof headline and lean on re-reads plus `needed_whole`. Decide with a hand-labeled precision check, not a hope.
- Touches: `src/outcome_signals.rs`, `src/conversations.rs`, `src/coach.rs`, `src/db.rs` join.
- Gate: either the correction label fires on real sessions at documented precision, or the headline is retired in copy and the model target is stated plainly.

**E-G. Compaction health as education, not proof.** Reframe the compaction view from a self-disclaiming proof attempt into a plain-language context-health read: how often the agent compacted, what tends to follow, what it costs. Education framing sidesteps the causal-proof problem that makes the current tab undercut itself.
- Touches: compaction view, `src/conversations.rs` (compaction events), `/api/context`.
- Gate: the view reads as a useful health signal a developer would act on, with no self-negating "this is not proof" as its main message.

### Phase 3: keep both exits open

**E-H. Two exits: narrative and product optionality.** For the portfolio exit, produce the public writeup and the honest single-machine metrics behind it. For the product exit, ship a shareable per-repo context report (a Context Bill someone can send a teammate) and a lightweight interest signal (waitlist or star ask), without committing to a go-to-market.
- Touches: `docs/`, an export path for the bill, a public-facing summary.
- Gate: the writeup is publishable, and a per-repo context report exports and opens on another machine.

## Outcome signal detection: do we need a model?

Short answer: for rereads and aborts, no. For corrections, a small local model helps, but it is not the missing piece, and it must not run in the hook. Separate the two jobs, because they are different problems.

**Behavioral signals** (reread, abort, immediate re-edit, error-then-retry) are events, not text judgments. A reread is "the agent read a path or ran a command it just touched." An abort is a flag on the tool result. An immediate re-edit is a path match inside a window. These are deterministic joins and already partly work (361 rereads joined today). A classifier here adds noise, not signal. Build them as joins, not a model. This is the CTX-32 workstream.

**Turn intent** (is this user turn a correction, an approval, a new instruction, a steer?) is the genuine language problem, and the lexical rules in `outcome_signals.rs` are too brittle for it: they miss "didnt revert back" and "clean enough?" and every paraphrase. Here a model earns its place, on a ladder from cheapest to heaviest:

1. **Broaden the lexicon first.** The steer and correction cues already in `coach.rs` and the Cursor guard catch your real language. Wire them in. Rules, no model, moves corrections off zero this week.
2. **Embedding kNN next, and this is the right first model.** ctx already ships all-MiniLM-L6-v2 (384-d, ONNX). Hand-label 50 to 100 turns, embed each new turn with its preceding assistant turn for context, and classify by nearest neighbors or a similarity threshold. Few labels, local, no new dependency, and it catches the paraphrases the lexicon cannot.
3. **A logistic classifier once labels exist,** fusing the embedding with the structural features we already compute (turn length against the user's own percentile, time since the tool result, whether an applied trim preceded it, path overlap). This graduates the signal into the same honest, inspectable family as `needed_whole`.
4. **LLM-as-judge only offline, only to manufacture labels.** It can label a training set at ingest, then we distill into the local classifier. It never runs in the hook, and it stays local or explicitly consented, because "no cloud, no LLM in the hook" is a trust pillar, not a default we relax quietly.

Attribution versus causation, because this is where the design decision lives. Naming the tool that likely caused a correction is doable, and we should do it. The JSONL has the full transcript, and `compress_decisions` records exactly what each trim dropped, so an offline pass can ask "does this correction reference what we cut?" Two mechanisms give it teeth: content overlap (embed the correction turn against the dropped lines; a high match means the complaint is about what got trimmed) and, once reversibility ships, the re-expand event itself (the agent asking for exactly the dropped block back is the closest-to-causal signal we get without an A/B). Aggregated over many instances that is a reliable per-tool risk read: if trimming a tool keeps coinciding with corrections that reference the dropped content, trimming it is risky for you.

What stays out of reach is single-case causation. You only ever observe the timeline where you did trim; the branch where the same session ran with the full output is in no log, so for any one correction you cannot prove the trim, rather than the agent's own mistake, was at fault. That is fine, because we dropped the proof thesis. Education and savings want the honestly-labeled suspect, not a courtroom verdict, and aggregate attribution delivers exactly that.

One clarification on "the model has the whole corpus." The model steering live trims does not. It is a small feature-based logistic (`needed_whole`, 0.89 AUC) that reads drop ratios and kinds, never the transcript, and runs in a no-LLM hook. Naming the culprit is a separate offline job over the JSONL that we have not built yet. The information for attribution is there; today's model just is not the thing reading it. Both the correction classifier and this attribution pass are tracked in CTX-54.

## What we are killing or demoting

- The "we prove trimming did not cost you corrections" headline. Demoted until the signal fires (E-F). It is our weakest evidence and we were leading with it.
- The compression-ratio and ROUGE game. Never ours to win; the competitive doc already settled this.
- Randomized withholding for a clean control arm (ADR 0009, already shelved in ADR 0012). Stays shelved. It cost real savings for data that never accumulated.
- The learned model as a user-facing "it adapts to you" claim. It stays shadow and honest until it beats the heuristic on real data. It is an engine part, not a pitch.

## The two exits (why "keep both open" is a real strategy, not a hedge)

The portfolio exit and the product exit want almost the same next six weeks, which is why we do not have to choose yet.

- **Portfolio exit.** The story is the honest pivot: built a proof tool, the data killed the core thesis, rebuilt around what worked. That story is only credible if the rebuild is real and the metrics are honest. Phase 1 is the rebuild. The writeup lands whether or not the product finds users.
- **Product exit.** The wedge is education plus safe savings on the surfaces RTK and the platforms miss, held together by the neutral local view. Phase 1 ships that wedge. If people use the Context Bill and keep it open, there is a product; if they do not, the portfolio exit is unharmed.

Both exits are served by shipping the same See, Save, Trust product honestly. The fork only matters later, at "do we invest in distribution," and we decide it with usage data we do not have yet.

## Draft LinkedIn post (the honest pivot)

> I spent months building an AI tool to compress my coding agent's context. Then my own data told me the main idea was wrong.
>
> The pitch was clean: don't just cut tokens, prove from your real sessions that cutting them didn't make the agent worse. Measure the corrections you had to make, and only trim what your own work shows is safe.
>
> I ran it on myself for 25 days. 2,731 decisions. Corrections it detected: zero.
>
> Not because the trimming was perfect. Because the signal I'd bet the whole product on almost never fires. I had built a proof around evidence that wasn't there.
>
> What actually worked was quieter. It removed about 475K tokens of dead weight on one machine. Its model for "what did the agent actually need to keep" was genuinely good. And the thing I never built turned out to be the thing that would have helped me most: just showing me where my context goes. Nobody does that. The platforms hide it.
>
> So I'm rebuilding it around what the data supports, not what sounded impressive: see where your agent's context goes, reclaim the room you safely can, and trust it because it runs locally and takes no side between the agents you use.
>
> Three things I'm taking with me:
> 1. Instrument the thing you're claiming before you claim it. My headline metric had never once fired.
> 2. The honest empty state ("none yet") was right, but honesty that undersells a real result is its own kind of dishonesty.
> 3. Your users don't want your cleverest mechanism. They want to understand their own problem. Educate first.
>
> Killing your own best idea on the evidence is not a setback. It's the job.

Tune the voice before posting. The numbers are all real and defensible as single-machine, early results; label them that way.

## Linear map

Tracked as the project **ctx revamp: education + savings** on the ctx team (`linear.app/saurabh0392/project/ctx-revamp-education-savings-f0ff83ba6640`). Epics:

| Epic | Linear | Phase | Priority | Notes |
| --- | --- | --- | --- | --- |
| Reposition surfaces to see/save/trust | CTX-49 | 1 | High | README + dashboard + copy |
| Context Bill (education front door) | CTX-50 | 1 | High | net-new, from existing data |
| Reversible Read compression | CTX-51 | 1 | Urgent | promotes SAU-147 to critical path |
| Savings reframe: context reclaimed | CTX-52 | 1 | High | extends ADR 0006 |
| Cross-agent one-truth surface | CTX-53 | 2 | High | the durable moat |
| Fix or retire the correction signal | CTX-54 | 2 | Medium | decide with a precision check; relates to CTX-32 |
| Compaction health as education | CTX-55 | 2 | Medium | reframe, not new proof |
| Two exits: narrative + product optionality | CTX-56 | 3 | Medium | writeup + shareable report |
