// Capture real ctx dashboard screenshots for the demo video. Uses the same playwright-core + system
// Chrome the coherence suite uses. Dark theme, 1080p frames at 2x for crisp text.
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);

const OUT = process.argv[2] || '.';
const PORT = process.argv[3] || '8789';
const base = `http://127.0.0.1:${PORT}`;

const shots = [
  { view: 'home', file: 'shot-home.png' },
  { view: 'see', file: 'shot-see.png' },
  { view: 'save', file: 'shot-save.png' },
  { view: 'settings', file: 'shot-settings.png' },
];

const browser = await chromium.launch({ headless: true, channel: 'chrome', args: ['--hide-scrollbars'] });
const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();

await page.goto(base, { waitUntil: 'networkidle' });
await page.evaluate(() => document.documentElement.setAttribute('data-theme', 'dark'));
await page.waitForTimeout(1200);

for (const s of shots) {
  await page.evaluate((v) => window.go && window.go(v), s.view);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(1400);
  await page.screenshot({ path: `${OUT}/${s.file}` });
  console.log('captured', s.file);
}

// Scrolled framings so a scene can focus a lower region (per-tool bars, earn-it ladder) without
// fragile cropping math.
async function scrolledShot(view, y, file) {
  await page.evaluate((v) => window.go && window.go(v), view);
  await page.waitForTimeout(700);
  await page.evaluate((yy) => window.scrollTo(0, yy), y);
  await page.waitForTimeout(900);
  await page.screenshot({ path: `${OUT}/${file}` });
  console.log('captured', file);
}
await scrolledShot('see', 760, 'shot-see2.png'); // per-tool trimmable bars
await scrolledShot('save', 520, 'shot-save2.png'); // earn-it ladder detail

// Report modal (the alpha-ask scene).
await page.evaluate(() => window.go && window.go('home'));
await page.waitForTimeout(500);
await page.evaluate(() => window.openReport && window.openReport());
await page.waitForTimeout(900);
await page.screenshot({ path: `${OUT}/shot-report.png` });
console.log('captured shot-report.png');

// Extract the exact numbers the dashboard is showing right now, so the narration speaks the same
// figures that are on screen (the live values drift, this keeps voice and picture in sync).
const norm = (t) => t.replace(/\s+/g, ' ').trim();
await page.evaluate(() => window.go && window.go('home'));
await page.waitForTimeout(900);
const homeText = norm(await page.evaluate(() => document.querySelector('#view-home')?.innerText || ''));
await page.evaluate(() => window.go && window.go('see'));
await page.waitForTimeout(900);
const seeText = norm(await page.evaluate(() => document.querySelector('#view-see')?.innerText || ''));
const grab = (re, s) => { const m = s.match(re); return m ? m[1].replace(/\s+/g, '') : null; };
const numbers = {
  home_reclaimed: grab(/([\d.]+\s*[MK]?)\s*tokens\s*reclaimed so far/i, homeText),
  see_output: grab(/([\d.]+\s*[MK]?)\s*tokens read back/i, seeText),
  see_input: grab(/([\d.]+\s*[MK]?)\s*tokens \/ request/i, seeText),
};
const { writeFileSync } = await import('fs');
writeFileSync(`${OUT}/numbers.json`, JSON.stringify(numbers, null, 2));
console.log('numbers:', JSON.stringify(numbers));

await browser.close();
console.log('done');
