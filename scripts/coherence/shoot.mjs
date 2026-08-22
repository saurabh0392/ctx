// Render each dashboard view to a PNG. fitcheck reads these: a persona review that never looks at
// pixels cannot see a duplicated header, a button loose in a paragraph, or five ragged left edges,
// which is exactly the class of defect that shipped past the source-only gate.
//
//   node scripts/coherence/shoot.mjs http://127.0.0.1:8799 /tmp/shots home save see settings
//
// Expanded by default: collapsed <details> hide the very content a reviewer needs to judge.
import { mkdirSync } from 'node:fs';

// Same resolution as run.mjs: playwright-core plus the system Chrome locally, or the downloaded
// chromium in CI (PW_CHANNEL='').
const { chromium } = await import(process.env.PW_CORE || 'playwright-core');
const hasChan = process.env.PW_CHANNEL !== undefined;
const channel = hasChan ? process.env.PW_CHANNEL || undefined : 'chrome';

const [, , base, outDir, ...views] = process.argv;
if (!base || !outDir) {
  console.error('usage: shoot.mjs <baseUrl> <outDir> [view...]');
  process.exit(2);
}
const targets = views.length ? views : ['home', 'save', 'see', 'settings'];
mkdirSync(outDir, { recursive: true });

const launchOpts = { headless: true, args: ['--hide-scrollbars'] };
if (channel) launchOpts.channel = channel;
const browser = await chromium.launch(launchOpts);
const written = [];
for (const view of targets) {
  const page = await browser.newPage({ viewport: { width: 1180, height: 1400 }, deviceScaleFactor: 2 });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await page.goto(`${base}/#${view}`, { waitUntil: 'load' });
  await page.waitForTimeout(2500);
  await page.evaluate(() => document.querySelectorAll('details').forEach((d) => (d.open = true)));
  await page.waitForTimeout(500);
  const path = `${outDir}/${view}.png`;
  await page.screenshot({ path, fullPage: true });
  written.push({ view, path, errors });
  await page.close();
}
await browser.close();
for (const w of written) {
  console.log(`${w.view}\t${w.path}${w.errors.length ? `\tCONSOLE ERRORS: ${w.errors.slice(0, 3).join(' | ')}` : ''}`);
}
