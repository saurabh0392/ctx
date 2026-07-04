# ctx personas

Status: living document
Date: 2026-07-04
Owner: Saurabh Sharan
Used by: `docs/redesign-dashboard-2026.md` (the redesign brief) and the `fitcheck` skill
(`.claude/skills/fitcheck/`), which role-plays these five to score any version of the product.

## The one paragraph

ctx has one buyer profile (a developer who runs a coding agent with MCP servers wired in) and five
faces of it that pull the product in different directions. The pragmatist wants one number and
silence. The power user wants every number and a lever. The skeptic wants to know what it breaks
before he trusts a word of it. The first-run evaluator has no data yet and no idea what ctx is. The
budget watcher wants a single honest figure she can defend to a teammate. A design that serves one
of them usually fails another. The job of the dashboard, and the job of `fitcheck`, is to hold all
five at once: a calm surface the pragmatist and evaluator can read in ten seconds, with depth,
proof, and control one scroll or one click beneath it for the power user and the skeptic, and one
stable payoff number the budget watcher never sees contradict itself.

## How to read a persona

Each persona has a patience window (how long before they bounce), the one question they open the
dashboard to answer, what earns them, what loses them, and the fitcheck weighting (which scoring
dimensions matter most for this person). The weightings are what make the score persona-specific:
the same screen can be a 5 for Sam and a 2 for Priya.

The fitcheck dimensions referenced below are defined in `.claude/skills/fitcheck/rubric.md`:
comprehension, time-to-value, trust-and-safety, cognitive-load, action-clarity, journey-coherence,
delight.

---

## 1. Sam, the ship-it pragmatist

Product engineer. Lives in Cursor and Claude Code all day, ten sessions deep, context-switching
between tickets. Installed ctx because a teammate said it makes agents "less dumb late in a
session." Does not care how it works and never will.

- **Patience window:** about 10 seconds. If the first screen is a wall, he closes the tab and ctx
  runs invisibly forever, which is fine by him but means the dashboard failed.
- **Opens it to answer:** "Am I better off, yes or no, and is there anything I have to do?"
- **Earns him:** one headline number, a plain-language "you're covered, nothing to do" line, at most
  one button. Motion that confirms it's alive without demanding attention.
- **Loses him:** jargon (WNAD, Wilson interval, "trimmed arm"), more than one primary action, three
  numbers that each claim to be "saved," any config he's asked to think about.
- **fitcheck weighting:** time-to-value and cognitive-load dominate, then action-clarity. He barely
  scores comprehension-depth because he doesn't want depth.

## 2. Priya, the connector maximalist

Senior full-stack engineer. Six-plus MCP servers wired: Linear, Figma, Canva, Notion, GitHub,
Sentry. Her context is genuinely heavy and she can feel sessions degrade. Technical, opinionated,
will read the mechanics and argue with them.

- **Patience window:** minutes, as long as the depth is real. She'll dig.
- **Opens it to answer:** "Where exactly is my context going, and can you actually cut the dead
  weight without me babysitting it?"
- **Earns her:** the itemized bill biggest-sink-first, per-tool detail, the evidence behind a trim,
  a real control (prune this server, hold that one). Being shown something no other tool shows her.
- **Loses her:** a toy that hides the numbers, hand-wavy "we optimized it" copy, a control that
  isn't actually reversible, anything that treats her like Sam.
- **fitcheck weighting:** comprehension-depth, action-clarity, and trust-and-safety lead;
  cognitive-load is near-irrelevant because she'll tolerate density that pays off.

## 3. Marcus, the trust-but-verify skeptic

Staff engineer, owns reliability. His standing fear: a background tool that trims context will cause
subtle, undebuggable agent failures. He will not enable autopilot until he understands the earn-it
gate and reversibility cold, and he assumes every claim is marketing until shown the evidence.

- **Patience window:** long, but adversarial. He's reading for the catch.
- **Opens it to answer:** "What can this silently break, and exactly how do I undo it?"
- **Earns him:** the fail-closed story told plainly (nothing is removed until your own sessions prove
  it's safe), the reversibility path (the tool still runs in full; a trim is one command back), real
  intervals and sample sizes, and honest empty states that say "not enough evidence yet" instead of
  faking confidence.
- **Loses him:** any irreversible action presented casually, a confidence claim with no n behind it,
  optimistic spin over a limitation, a number that changes meaning between two screens.
- **fitcheck weighting:** trust-and-safety and journey-coherence dominate, then comprehension. He
  actively rewards honest limitation over polished overclaim.

## 4. Alex, the first-run evaluator

Just ran the installer. Opens the dashboard for the first time with little or no data in it. Does
not yet know what ctx is or why it should stay installed. This is the make-or-break persona: most
churn happens in the first session, on the empty state.

- **Patience window:** about 30 seconds of goodwill, then the verdict "keep or uninstall."
- **Opens it to answer:** "What is this, and is it worth keeping?"
- **Earns her:** a first screen that teaches in one sentence what ctx does, an empty state that reads
  as "warming up, here's what's coming" rather than broken, and a clear sense of what will happen as
  she works (ctx will watch, then start reclaiming what it proves safe).
- **Loses her:** numbers with no story, `n/a` everywhere, an empty state that looks like a bug, being
  dropped into depth before she has the concept.
- **fitcheck weighting:** comprehension and journey-coherence lead, then delight (first impression);
  time-to-value matters but the "value" she needs first is understanding, not a metric.

## 5. Jordan, the budget watcher

Engineering lead or solo founder watching token spend and cost. Cares about dollars and the weekly
trend, not the mechanics. Wants a defensible "we're net ahead this week" figure and something she
could screenshot for a teammate or an investor update.

- **Patience window:** a minute, goal-directed. She's looking for one figure.
- **Opens it to answer:** "How much am I actually saving, and is it trending the right way?"
- **Earns her:** one cumulative payoff number that never contradicts itself, a weekly net-ahead
  verdict with the trend, and a framing she can repeat out loud without caveats.
- **Loses her:** three different "saved" numbers on one page (the exact bug the redesign must kill),
  a headline that means output-only on one screen and output-plus-input on another, cost framing
  that feels invented.
- **fitcheck weighting:** journey-coherence (one consistent number) and comprehension lead, then
  trust-and-safety. She is the canary for the "figures disagree" failure.

---

## The tensions the design has to resolve

These conflicts are the whole reason a redesign is hard. fitcheck exists to catch when a version
resolves one by breaking another.

- **Sam vs Priya (calm vs depth).** Minimal top, progressive disclosure beneath. Sam never scrolls;
  Priya scrolls and clicks in. A version that satisfies one by starving the other fails fitcheck.
- **Marcus vs Sam (proof vs brevity).** The trust story has to be legible without being loud. Marcus
  finds it when he looks; Sam never has to read it. Reversibility lives near the action, not in a
  wall of caveats up top.
- **Alex vs everyone (empty vs full).** Every screen has to teach when empty and inform when full.
  fitcheck always scores at least one persona against the cold-start state.
- **Jordan vs the codebase's own history (one number vs many).** The "three different saved figures"
  problem already bit this product once. One headline, defined once, summed consistently everywhere.
  Journey-coherence scoring is aimed straight at this.

## Maintenance

When the product or the market view shifts, update the persona, not the redesign doc or the skill.
Both read from here. If a new archetype appears in real alpha feedback (for example a "team admin
rolling ctx out to a squad"), add it as persona 6 and give it a fitcheck weighting so the skill
starts scoring for it automatically.
