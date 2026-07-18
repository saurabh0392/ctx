# ctx: LinkedIn post

Learning-project framing, first person, ~238 words. Replace the link with your public portfolio URL before posting. Keep it to the three hashtags.

---

My coding agent was carrying about 77,000 tokens on every request. Roughly 64,000 of them were for tools it never once called.

That is the kind of waste you can't fix if you can't see it. So as a learning project, I built ctx: a local tool that shows a coding agent (Claude Code, Cursor) where its limited context actually goes, and reclaims the waste only after your own sessions prove the cut did no harm. The spine is simple: See where your context goes, Save only what is proven safe, Trust that it stays local and reversible.

The part I'd actually share with other PMs is the process. Redesigning the dashboard, I kept hitting one question: is this version better, and better for whom? So I made my personas executable. I wrote a check I call fitcheck that role-plays five user types through any version of the UI and scores it, with one rule: a version ships only if it beats the last and no single persona regresses. It turned "this feels better" into a call I could defend. The dashboard moved from 2.9 to 4.4 out of 5, and the two users I was designing for gained the most.

One honest lesson: every time I showed a limitation plainly instead of hiding it, the trust score went up, not down.

Write-up: [your portfolio link]

#ProductManagement #AI #DeveloperTools

---

Notes:
- Link: paste the public portfolio URL (the case study page).
- The 77K / 64K figures and the 2.9 to 4.4 score come from your own machine and your fitcheck reports. Safe to share; no mechanism is revealed.
- No launch language on purpose. It reads as a reflection, not an announcement.
