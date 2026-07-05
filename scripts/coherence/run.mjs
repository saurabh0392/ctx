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
  const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--hide-scrollbars'] });
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
        if (key.tool) {
          const tool = await h.evaluate((e) => (e.closest('.sv-mini')?.querySelector('.n') || e.closest('.sv-card')?.querySelector('.sv-name') || {}).textContent || '');
          if (tool !== key.tool) continue;
        }
        return h;
      }
      return null;
    },
    async clickAndSettle(handle) {
      try { await handle.click({ timeout: 2000 }); } catch { /* detached */ }
      try { await page.waitForLoadState('networkidle', { timeout: 3000 }); } catch { /* no net */ }
      await page.waitForTimeout(700);
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
