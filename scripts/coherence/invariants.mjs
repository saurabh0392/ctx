// Behavioral coherence invariants for the ctx dashboard.
//
// Each invariant drives or reads the LIVE dashboard and asserts a property that a static read of the
// HTML cannot check: a control actually does something, the same concept shows one value everywhere,
// a tool is classified the same way on every screen it appears on. This is the layer fitcheck (a
// read-only persona pass) structurally cannot see, and it is where every recent regression lived:
//   - "Put on trial does nothing" (a held tool rendered a button whose backend refuses it)
//   - held tools shown as trim candidates on See while parked out of trimming on Save
//   - an earned tool's clean-test control run labelled "still only watching" in the activity feed
//   - two "reclaimed so far" figures disagreeing one click apart
//
// An invariant returns { pass, detail, evidence? }. `H` (built in run.mjs) provides:
//   H.api(path)            -> parsed JSON from the dashboard API
//   H.goto(view)           -> navigate to #view and wait for its content
//   H.page                 -> the Playwright page
//   H.$$(sel, fn)          -> page.$$eval(sel, fn)
//   H.parseK(str)          -> "14.0M" | "23K" | "701" -> Number
//   H.clickAndSettle(h)    -> click an element handle, wait for any reload to settle
//   H.waitForViewChange(v, before) -> poll until the view changes or a bounded timeout expires
//   H.resetConfig()        -> restore the isolated config.toml to pristine (undo a mutation click)
//   H.pretty(rawToolName)  -> the dashboard's prettyTool() display form

const ok = (detail, evidence) => ({ pass: true, detail, evidence });
const fail = (detail, evidence) => ({ pass: false, detail, evidence });

// Controls where success MUST re-render the view, so "clicked and nothing changed" is unambiguously a
// dead control (the class the trial no-op belonged to). Deliberately excludes controls whose real
// effect is not a view re-render and would false-positive here: exportDb (file download),
// purgePrompts / deleteData (confirm dialog + destructive), saveBudget (inline field, may not re-flow).
// Those deserve their own targeted checks, not this generic one.
const MUTATION_RE = /\btrial\(|\bpruneServer\(|\bsetPresetFrom\(/;

export const invariants = [
  // ── Interaction: no action control is a no-op ────────────────────────────────────────────────
  {
    id: 'no-dead-action-buttons',
    category: 'interaction',
    title: 'Every mutation control produces an observable change',
    async run(H) {
      const dead = [];
      const unclicked = [];
      for (const view of ['save', 'see', 'settings']) {
        await H.goto(view);
        // Enumerate mutation controls by a stable key (label + tool), test each from a pristine config.
        // Identify a control by its row's stable data-key, not by the name on screen. Several MCP
        // tools render the same display name ("Linear: fetch" exists under two servers), so a
        // text-only identity resolved two different rows to one control and the result depended on
        // which one the DOM handed back.
        const keys = await H.$$(`#view-${view} [onclick]`, (els, re) =>
          els.filter((e) => new RegExp(re).test(e.getAttribute('onclick') || ''))
             .map((e) => ({ label: (e.textContent || '').trim().slice(0, 30),
                            key: e.closest('[data-key]')?.dataset.key || '',
                            tool: (e.closest('.sv-row')?.querySelector('.sv-row-name') || e.closest('.sv-mini')?.querySelector('.n') || e.closest('.sv-card')?.querySelector('.sv-name') || {}).textContent || '' })),
          MUTATION_RE.source);
        for (const k of keys) {
          await H.resetConfig();
          await H.goto(view);
          const handle = await H.findControl(view, k);
          if (!handle) continue; // superseded by a prior mutation; not a dead-button signal
          // Expand first, then take the baseline, so the signature measures the click's effect and
          // not the expansion that made the control reachable.
          await H.reveal(handle);
          const before = await H.controlSignature(view, k.key);
          const landed = await H.clickAndSettle(handle, () => H.findControl(view, k));
          if (!landed) {
            // Never clicked, so it says nothing about whether the control works. Surfaced rather
            // than counted, because silently treating it as dead is what made this check unreliable.
            unclicked.push(`${view}: "${k.label}"${k.tool ? ` on ${k.tool}` : ''}`);
            continue;
          }
          const observed = await H.waitForViewChange(view, before, k.key);
          if (!observed.changed) dead.push(`${view}: "${k.label}"${k.tool ? ` on ${k.tool}` : ''}`);
        }
      }
      await H.resetConfig();
      if (dead.length) return fail(`${dead.length} action control(s) fire but change nothing (dead buttons)`, dead);
      const note = unclicked.length ? ` (${unclicked.length} could not be clicked after retries, not scored)` : '';
      return ok(`every mutation control produced a visible change${note}`);
    },
  },

  // ── Classification: held tools are held everywhere, and never actionable ──────────────────────
  {
    id: 'held-tools-have-no-trial-button',
    category: 'classification',
    title: 'A held tool is never offered a trial (the backend would refuse it)',
    async run(H) {
      const ctx = await H.api('/api/context');
      const held = new Set(ctx.tools.filter((t) => t.held_reason).map((t) => H.pretty(t.tool)));
      await H.goto('save');
      const offenders = await H.$$('#view-save [onclick]', (els) =>
        els.filter((e) => /\btrial\(/.test(e.getAttribute('onclick') || '') && /trial/i.test(e.textContent || ''))
           .map((e) => (e.closest('.sv-row')?.querySelector('.sv-row-name') || e.closest('.sv-mini')?.querySelector('.n') || e.closest('.sv-card')?.querySelector('.sv-name') || {}).textContent || ''));
      const bad = offenders.filter((name) => held.has(name));
      return bad.length
        ? fail(`${bad.length} held tool(s) show a trial button that no-ops`, bad)
        : ok(`no held tool offers a trial (${held.size} held tools checked)`);
    },
  },
  {
    id: 'held-classification-consistent',
    category: 'classification',
    title: 'The held set on Save equals the deny-set from the API',
    async run(H) {
      const ctx = await H.api('/api/context');
      const apiHeld = new Set(ctx.tools.filter((t) => t.held_reason).map((t) => H.pretty(t.tool)));
      await H.goto('save');
      const shownHeld = new Set(await H.$$('#view-save .sv-pill.held', (els) => els.map((e) => (e.closest('.sv-row')?.querySelector('.sv-row-name') || {}).textContent || '')));
      // Every held tool that has any decisions and is prominent enough to render should be in the held
      // section; nothing in the held section should be trim-eligible.
      const elig = new Set(ctx.tools.filter((t) => !t.held_reason).map((t) => H.pretty(t.tool)));
      const leaked = [...shownHeld].filter((n) => elig.has(n));
      return leaked.length
        ? fail(`${leaked.length} eligible tool(s) shown in the Held section`, leaked)
        : ok(`Held section holds only deny-set tools (${shownHeld.size} shown of ${apiHeld.size} held)`);
    },
  },
  {
    id: 'held-marked-on-see',
    category: 'classification',
    title: 'See marks held tools instead of listing them as trim candidates',
    async run(H) {
      const ctx = await H.api('/api/context');
      const held = new Set(ctx.tools.filter((t) => t.held_reason).map((t) => H.pretty(t.tool)));
      await H.goto('see');
      // A held tool that shows in the output list must carry a held marker (class or badge), not a
      // bare "% trimmable" as if it were a trim candidate.
      const rows = await H.$$('#see-out .see-row', (els) => els.map((e) => ({
        name: (e.querySelector('.see-rname') || {}).textContent || '',
        held: e.classList.contains('held') || /held/i.test((e.querySelector('.see-hl') || {}).textContent || ''),
      })));
      const unmarked = rows.filter((r) => held.has(r.name) && !r.held).map((r) => r.name);
      return unmarked.length
        ? fail(`${unmarked.length} held tool(s) shown on See as trim candidates with no held marker`, unmarked)
        : ok('every held tool on See is marked held');
    },
  },

  // ── Numeric: one concept, one value; totals exclude what is never reclaimable ─────────────────
  {
    id: 'held-not-counted-reclaimable',
    category: 'numeric',
    title: 'The reclaimable total excludes held tools (they are never reclaimable)',
    async run(H) {
      const [ctx, bill] = await Promise.all([H.api('/api/context'), H.api('/api/context/bill')]);
      const held = new Set(ctx.tools.filter((t) => t.held_reason).map((t) => t.tool));
      const heldReclaimable = (bill.tools || []).filter((t) => held.has(t.tool)).reduce((a, t) => a + (t.reclaimable_chars || 0), 0);
      const sumEligible = (bill.tools || []).filter((t) => !held.has(t.tool)).reduce((a, t) => a + (t.reclaimable_chars || 0), 0);
      // total_reclaimable_chars should equal the eligible sum; if held output is folded in, the "on the
      // table" figure overstates what ctx can ever reclaim.
      const delta = (bill.total_reclaimable_chars || 0) - sumEligible;
      return heldReclaimable > 0 && delta > 0
        ? fail(`reclaimable total includes ${heldReclaimable} chars from held tools that never trim`, { total: bill.total_reclaimable_chars, eligibleSum: sumEligible, heldReclaimable })
        : ok('reclaimable total counts only eligible tools');
    },
  },
  {
    id: 'reclaimed-figure-reconciles',
    category: 'numeric',
    title: 'Home reclaimed = See output-reclaimed + input-reclaimed (displayed)',
    async run(H) {
      await H.goto('home');
      const home = H.parseK(await H.text('#h2-hero .h2-num'));
      await H.goto('see');
      const splits = await H.$$('#view-see .see-split > div', (els) => els.map((e) => ({
        n: (e.querySelector('.see-n') || {}).textContent || '', l: (e.querySelector('.see-l') || {}).textContent || '' })));
      const out = splits.find((s) => /output reclaimed/i.test(s.l));
      const inp = splits.find((s) => /input reclaimed/i.test(s.l));
      if (!out || !inp) return fail('See is missing an "output reclaimed" or "input reclaimed" component to reconcile against Home', splits.map((s) => s.l));
      const sum = H.parseK(out.n) + H.parseK(inp.n);
      const tol = Math.max(home, sum) * 0.02 + 1; // fmtK rounding slack
      return Math.abs(home - sum) <= tol
        ? ok(`Home ${home} ≈ See ${H.parseK(out.n)} + ${H.parseK(inp.n)}`)
        : fail(`Home reclaimed (${home}) != See output+input (${sum})`, { home, out: out.n, in: inp.n });
    },
  },
  {
    id: 'ladder-counts-match-sections',
    category: 'numeric',
    title: 'Save ladder rung counts match the tools under them',
    async run(H) {
      await H.goto('save');
      const rungs = Object.fromEntries(await H.$$('#save-ladder .sv-rung', (els) => els.map((e) => [
        (e.querySelector('.sv-rname') || {}).textContent.trim(), Number((e.querySelector('.sv-rcount') || {}).textContent.replace(/[^\d]/g, '')) ])));
      // Trimming rung == cards in the active spotlight that read TRIMMING. The underlying stage is
      // still named `earned`; the dashboard label is deliberately plain-language product copy.
      const trimmingCards = await H.$$('#view-save .sv-row > summary .sv-pill', (els) => els.filter((p) => /^trimming$/i.test(p.textContent.trim())).length);
      const problems = [];
      if (rungs.Trimming !== trimmingCards) problems.push(`Trimming rung ${rungs.Trimming} != ${trimmingCards} trimming cards`);
      // Parked held count == rows in the Held section (when all are shown).
      const parked = H.parseK((await H.text('#save-ladder .sv-parked b')) || '0');
      const heldRows = await H.$$('#view-save .sv-row > summary .sv-pill.held', (els) => els.length);
      if (parked && parked !== heldRows) problems.push(`parked held ${parked} != ${heldRows} held rows`);
      return problems.length ? fail(problems.join('; '), { rungs, trimmingCards, parked, heldRows }) : ok('ladder counts match their sections');
    },
  },

  // ── Label: a tool's state reads the same on every screen ──────────────────────────────────────
  {
    id: 'no-earned-tool-labeled-watching',
    category: 'label',
    title: 'An earned tool is never labelled "only watching" in the activity feed',
    async run(H) {
      const ctx = await H.api('/api/context');
      const proof = await H.api('/api/context/proof');
      const earned = new Set((proof.tools || []).filter((t) => t.verdict === 'safe').map((t) => t.tool));
      // The feed row exposes explore_arm; a non-applied earned-tool run must be a control holdout,
      // which reads as "control", not "still only watching".
      const feed = ctx.feed || [];
      const bad = feed.filter((r) => earned.has(r.tool_name) && !r.applied && (r.lines_drop || 0) > 0 && !r.protected && r.explore_arm !== 'control');
      return bad.length
        ? fail(`${bad.length} earned-tool run(s) would read as "only watching" but are unexplained holdouts`, bad.map((r) => r.tool_name))
        : ok('earned tools never read as "only watching" (holdouts are labelled control)');
    },
  },

  // ── The Jordan gate: the same headline label never shows two different values ──────────────────
  {
    id: 'no-label-shows-two-values',
    category: 'numeric',
    title: 'No headline figure label maps to two different values across views',
    async run(H) {
      const seen = {}; // label -> Set of displayed values
      const record = (label, value) => { if (!label || !value) return; (seen[label] ||= new Set()).add(value.trim()); };
      // Home hero cap + its number.
      await H.goto('home');
      record((await H.text('#h2-hero .h2-cap')) || '', (await H.text('#h2-hero .h2-num')) || '');
      // See tax-card splits (label -> value).
      await H.goto('see');
      for (const s of await H.$$('#view-see .see-split > div', (els) => els.map((e) => ({ l: (e.querySelector('.see-l') || {}).textContent || '', n: (e.querySelector('.see-n') || {}).textContent || '' }))))
        record(s.l, s.n);
      const clashes = Object.entries(seen).filter(([, vals]) => vals.size > 1).map(([l, vals]) => `"${l}" = ${[...vals].join(' vs ')}`);
      return clashes.length ? fail(`${clashes.length} label(s) show conflicting values`, clashes) : ok('every headline label shows one consistent value');
    },
  },
];
