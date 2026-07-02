# I killed my AI tool's headline feature because my own data said it never worked

Status: publishable draft. Tune the voice before posting.
Date: 2026-07-02
Numbers below are from one developer's machine, early. They are labeled that way on purpose.

## The short version

I spent months building a tool to compress my coding agent's context. The pitch was not "cut tokens." Anyone can cut tokens. The pitch was: prove, from your own real sessions, that cutting them did not make the agent worse. Measure the corrections you had to make after a trim, and only trim what your own work shows is safe.

I ran it on myself for 25 days. Across 2,731 recorded decisions, the correction detector fired zero times.

Not because the trimming was flawless. Because the signal I had bet the whole product on almost never fires. I had built a proof around evidence that was not there.

So I rebuilt the tool around what the data actually supported. This is what that looked like.

## What the data said

Three things, all from the live database on one machine.

**The headline metric was hollow.** Zero corrections across 2,731 joined decisions. The gate required an explicit complaint ("that's wrong", "revert that") to land in a short window right after a trim that actually dropped lines. The steering I do constantly ("didn't revert back", "which one for b?", "clean enough?") almost never lands inside that exact window with that exact wording. The pitch was writing a check the data would not cash.

**The quiet parts worked.** The tool removed roughly 475K tokens of tool output that nobody was reading, applied, not projected. Its model for "what did the agent actually need to keep whole" reached a holdout AUC of 0.89, and has since improved to 0.95 as more labels accrued. As a "do not trim what the agent needs in full" model, that is genuinely useful.

**The thing I never built was the thing I needed most.** The tool showed what it did to each tool's output. It never showed the one thing I did not understand: where my context actually goes, and why a long session gets duller. The raw material was already sitting in the database. A single Linear list call cost 112,732 characters and collapsed to 553. Read was the number one sink. That is an itemized context bill, and no agent vendor shows it. The platforms hide it.

## What I changed

I stopped leading with the proof claim and demoted it to a background check. In its place I built the parts the data backed:

- **A context bill.** Where your agent's context went this week, per tool, ranked by size, with what is reclaimable and what was already reclaimed. Works on day one with zero labels.
- **Reversible trims.** The biggest savings pool (file reads) is also the one trimming hurts most. So trims are now reversible: the agent asks for the verbatim original back when it needs the detail, and the risky trim becomes a cheap round trip.
- **One honest view across agents.** The tool sits under more than one coding agent on the same machine and reports on all of them, neutrally. No agent vendor will ever grade its rivals, or itself, honestly. That neutrality is the real moat, and it was buried as a footnote.
- **A suspected-cost read, not a proof.** Instead of claiming a trim caused a correction (unprovable for any single case, the full-output branch is in no log), it aggregates: when the tool trimmed a given tool, how often did the agent behave as if it needed the dropped content back. Named as a suspect, per tool, never a verdict. That honest aggregate is what flagged that my file-read and shell trimming were too blunt, which sent me back to fix the trimming itself.

## What I kept, and what I dropped

Kept: the honest empty states ("none yet" instead of a fake number), the local-only promise (nothing leaves the machine), the earned-only savings gate, the model as a quiet engine part rather than an "it adapts to you" pitch.

Dropped: the corrections-proof headline, the compression-ratio game, and the idea that the cleverest mechanism was the product. It was not. Understanding your own problem was.

## Three things I took with me

1. Instrument the thing you are claiming before you claim it. My headline metric had never once fired, and I did not find that out until I looked.
2. The honest empty state was right, but honesty that undersells a real result is its own kind of dishonesty. "None yet" was true; it also hid 475K tokens of real savings.
3. Users do not want your cleverest mechanism. They want to understand their own problem. Educate first.

Killing your own best idea on the evidence is not a setback. It is the job.

## The numbers, single-machine and early

One developer, one machine. Do not read these as product metrics; read them as an honest starting line.

| Metric | Value | Note |
| --- | --- | --- |
| Recorded decisions | 2,731 | over 25 days |
| Corrections the detector fired | 0 | the killed thesis |
| Tool output removed | ~475K tokens | applied, not projected |
| "Needed whole" model, holdout AUC | 0.89, now 0.95 | improved as labels accrued |
| Bytes leaving the machine | 0 | a promise, not a trend |

The context bill, the reversible trims, and the cross-agent view all ship on this same data. Whether they find users is a separate question, and an honest one to leave open.
