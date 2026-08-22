// Behavioral coherence runner. Drives a LIVE ctx dashboard and checks the invariants in
// invariants.mjs. Exits non-zero if any fail, so it can gate a deploy or a git push.
//
// Env:
//   SMOKE_BASE            dashboard base URL (default http://127.0.0.1:8799)
//   PW_CORE               path to a playwright-core install (default: resolve 'playwright-core')
//   CTX_HOME_LIVE         the isolated CTX_HOME the dashboard is using (for config reset between clicks)
//   CTX_PRISTINE_CONFIG   a pristine copy of config.toml to restore from
//
// The wrapper (coherence.sh) sets these up against an isolated copy of ~/.ctx so clicking mutation
// controls never touches real data.

import { createRequire } from 'module';
import { readFileSync, writeFileSync } from 'fs';
import { invariants } from './invariants.mjs';
import { waitForChange } from './wait-for-change.mjs';

const require = createRequire(import.meta.url);
const pwPath = process.env.PW_CORE || 'playwright-core';
const { chromium } = require(pwPath);

const BASE = (process.env.SMOKE_BASE || 'http://127.0.0.1:8799').replace(/\/$/, '');
const LIVE_CONFIG = process.env.CTX_HOME_LIVE ? `${process.env.CTX_HOME_LIVE}/config.toml` : null;
const PRISTINE_CONFIG = process.env.CTX_PRISTINE_CONFIG || null;

const WAIT_SEL = { home: '#h2-ladder .h2-rung', see: '.see-tax', save: '#save-ladder .sv-rung', settings: '#view-settings' };

function prettyTool(name) {
  if (name && name.startsWith('mcp__')) {
    const parts = name.split('__');
    if (parts[1] === 'codex_apps' && parts.length >= 4) {
      const app = parts[2] || 'app';
      const method = parts.slice(3).join(' ').replace(/_/g, ' ');
      return `${app.charAt(0).toUpperCase()}${app.slice(1)}: ${method}`;
    }
    const prov = (parts[1] || '').split('_').pop();
    const method = (parts[2] || '').replace(/_/g, ' ');
    return prov ? `${prov}: ${method}` : method;
  }
  return name;
}

function parseK(str) {
  if (!str) return 0;
  const m = String(str).replace(/,/g, '').match(/([\d.]+)\s*([KMB]?)/i);
  if (!m) return 0;
  const mult = { '': 1, K: 1e3, M: 1e6, B: 1e9 }[(m[2] || '').toUpperCase()];
  return Math.round(parseFloat(m[1]) * mult);
}

async function main() {
  // Local runs use the system Chrome (channel: 'chrome'); CI sets PW_CHANNEL='' to use the chromium
  // that `npx playwright install chromium` downloaded, since a CI runner has no branded Chrome.
  const hasChan = process.env.PW_CHANNEL !== undefined;
  const channel = hasChan ? (process.env.PW_CHANNEL || undefined) : 'chrome';
  const launchOpts = { headless: true, args: ['--hide-scrollbars'] };
  if (channel) launchOpts.channel = channel;
  const browser = await chromium.launch(launchOpts);
  const context = await browser.newContext({ viewport: { width: 1280, height: 1600 }, reducedMotion: 'reduce' });
  const page = await context.newPage();
  // Never let a confirm()/prompt() from a clicked control block the run.
  page.on('dialog', (d) => d.dismiss().catch(() => {}));

  const H = {
    page,
    pretty: prettyTool,
    parseK,
    async api(path) {
      const r = await fetch(BASE + path);
      if (!r.ok) throw new Error(`${path} -> ${r.status}`);
      return r.json();
    },
    async goto(view) {
      await page.goto(`${BASE}/#${view}`, { waitUntil: 'load' });
      await page.evaluate((v) => { location.hash = '#' + v; window.dispatchEvent(new HashChangeEvent('hashchange')); }, view);
      try { await page.waitForSelector(WAIT_SEL[view] || `#view-${view}`, { timeout: 9000 }); } catch { /* empty state */ }
      await page.waitForTimeout(350);
    },
    $$(sel, fn, arg) { return page.$$eval(sel, fn, arg); },
    async text(sel) { const el = await page.$(sel); return el ? (await el.textContent()).trim() : ''; },
    // A control that lives in a keyed row is judged on that row alone: its pill and its buttons.
    // Diffing the whole view's innerText made the verdict depend on everything else re-rendering in
    // time, which produced dead-button reports for controls that demonstrably work (each reported
    // one flipped Watching to Testing when driven by hand). Falls back to the view signature for
    // controls that have no row of their own.
    async controlSignature(view, key) {
      if (!key) return H.viewSignature(view);
      return page.evaluate(({ v, k }) => {
        const root = document.getElementById('view-' + v);
        const row = root && [...root.querySelectorAll('[data-key]')].find((d) => d.dataset.key === k);
        if (!row) return 'MISSING';
        const pill = (row.querySelector('.sv-pill')?.textContent || '').trim();
        const acts = [...row.querySelectorAll('[onclick]')].map((e) => (e.textContent || '').trim()).join('|');
        const metric = (row.querySelector('.sv-row-metric')?.textContent || '').trim();
        return `${pill}::${acts}::${metric}`;
      }, { v: view, k: key });
    },
    async viewSignature(view) {
      return page.evaluate((v) => {
        const root = document.getElementById('view-' + v);
        if (!root) return '';
        const opens = root.querySelectorAll('.open').length;
        return 'O' + opens + '|' + (root.innerText || '').replace(/\s+/g, ' ').trim();
      }, view);
    },
    async findControl(view, key) {
      const handles = await page.$$(`#view-${view} [onclick]`);
      for (const h of handles) {
        const label = (await h.textContent() || '').trim().slice(0, 30);
        if (label !== key.label) continue;
        // Prefer the row's stable data-key: display names collide across MCP servers.
        if (key.key) {
          const dk = await h.evaluate((e) => e.closest('[data-key]')?.dataset.key || '');
          if (dk !== key.key) continue;
          return h;
        }
        if (key.tool) {
          const tool = await h.evaluate((e) => (e.closest('.sv-row')?.querySelector('.sv-row-name') || e.closest('.sv-mini')?.querySelector('.n') || e.closest('.sv-card')?.querySelector('.sv-name') || {}).textContent || '');
          if (tool !== key.tool) continue;
        }
        return h;
      }
      return null;
    },
    // Controls may sit inside collapsed <details> rows. Open the chain as a separate step so the
    // caller can take its baseline afterwards: folded into the click, the expansion alone changed
    // innerText and every control looked alive whether or not it did anything.
    async reveal(handle) {
      try {
        await handle.evaluate((e) => {
          let d = e.closest('details');
          while (d) { d.open = true; d = d.parentElement ? d.parentElement.closest('details') : null; }
        });
      } catch { /* detached */ }
    },
    async clickAndSettle(handle) {
      await H.reveal(handle);
      try { await handle.click({ timeout: 2000 }); } catch { /* detached */ }
      try { await page.waitForLoadState('networkidle', { timeout: 3000 }); } catch { /* no net */ }
    },
    async waitForViewChange(view, before, key) {
      return waitForChange(() => H.controlSignature(view, key), before, {
        // A mutation re-renders the view from three sequential API fetches. Under a full suite run
        // the dashboard is busy enough that this can exceed a 5s budget, and a slow render then
        // reads as a dead button: the reported control always turns out to work when driven by
        // hand. Wait long enough that the verdict is about the control, not the machine.
        timeoutMs: 15000,
        intervalMs: 100,
        sleep: (ms) => page.waitForTimeout(ms),
      });
    },
    async resetConfig() {
      if (LIVE_CONFIG && PRISTINE_CONFIG) {
        try { writeFileSync(LIVE_CONFIG, readFileSync(PRISTINE_CONFIG)); } catch { /* best effort */ }
      }
    },
  };

  const results = [];
  for (const inv of invariants) {
    const t0 = Date.now();
    try {
      const r = await inv.run(H);
      results.push({ ...inv, ...r, ms: Date.now() - t0 });
    } catch (e) {
      results.push({ ...inv, pass: false, detail: `threw: ${e.message}`, ms: Date.now() - t0 });
    }
  }
  await H.resetConfig();
  await browser.close();

  const failed = results.filter((r) => !r.pass);
  console.log(`\n  ctx behavioral coherence  ${BASE}\n  ${'─'.repeat(60)}`);
  for (const r of results) {
    console.log(`  ${r.pass ? 'PASS' : 'FAIL'}  [${r.category}] ${r.title}`);
    console.log(`        ${r.detail}`);
    if (!r.pass && r.evidence) console.log(`        evidence: ${JSON.stringify(r.evidence).slice(0, 300)}`);
  }
  console.log(`  ${'─'.repeat(60)}`);
  console.log(`  ${results.length - failed.length}/${results.length} passed${failed.length ? `,  ${failed.length} FAILED` : ''}\n`);
  process.exit(failed.length ? 1 : 0);
}

main().catch((e) => { console.error('runner error:', e); process.exit(2); });
