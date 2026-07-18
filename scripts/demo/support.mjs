// Honest support matrix for the demo: which surfaces and platforms ctx actually covers, and where it
// only gives insight (no hooks = no trimming). Dark, matches the video aesthetic.
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);
const OUT = process.argv[2];

// status: full (green), partial (amber), soon (dim amber)
const surfaces = [
  ['Claude Code, CLI', 'trim, prune, tools, dashboard', 'full'],
  ['Claude Code in your IDE', 'Cursor, VS Code, Windsurf', 'full'],
  ['Claude Desktop', 'tools + dashboard, no auto-trim', 'partial'],
];
const platforms = [
  ['macOS', 'Apple Silicon and Intel', 'full'],
  ['Linux', 'x86_64', 'full'],
  ['Windows', 'first-class build in progress', 'soon'],
];

const pill = { full: ['#0f2018', '#35c88f', 'full'], partial: ['#241b0e', '#e0a44a', 'insight only'], soon: ['#1a1a1a', '#8a938c', 'coming'] };

function rows(list) {
  return list.map(([name, note, st]) => {
    const [bg, fg, label] = pill[st];
    return `<div class="row">
      <div><div class="rn">${name}</div><div class="rnote">${note}</div></div>
      <div class="pill" style="background:${bg};color:${fg};border-color:${fg}55">${label}</div>
    </div>`;
  }).join('');
}

const html = `<!doctype html><html><head><meta charset="utf-8"><style>
  * { margin:0; padding:0; box-sizing:border-box; }
  html,body { width:1920px; height:1080px; background:#0f1311; overflow:hidden; }
  .mark { position:absolute; top:56px; left:150px; font-family:-apple-system,sans-serif; font-weight:800; font-size:30px; color:#edf1ed; }
  .mark span { color:#35c88f; }
  .wrap { padding:150px 150px; height:100%; display:flex; flex-direction:column; justify-content:center; }
  .kicker { font-family:"SF Mono",Menlo,monospace; font-size:20px; letter-spacing:.3em; color:#35c88f; margin-bottom:22px; }
  .head { font-family:"Iowan Old Style",Palatino,Georgia,serif; font-size:60px; line-height:1.06; color:#edf1ed; font-weight:600; margin-bottom:64px; }
  .cols { display:flex; gap:60px; }
  .col { flex:1; }
  .ct { font-family:"SF Mono",Menlo,monospace; font-size:19px; letter-spacing:.22em; color:#7f8a83; margin-bottom:24px; }
  .row { display:flex; align-items:center; justify-content:space-between; padding:22px 26px; border:1px solid #1b221d; border-radius:14px; background:#0b0f0d; margin-bottom:16px; }
  .rn { font-family:-apple-system,sans-serif; font-size:29px; color:#edf1ed; font-weight:500; }
  .rnote { font-family:-apple-system,sans-serif; font-size:22px; color:#7f8a83; margin-top:4px; }
  .pill { font-family:"SF Mono",Menlo,monospace; font-size:19px; padding:8px 18px; border-radius:999px; border:1px solid; white-space:nowrap; }
  .foot { margin-top:56px; font-family:-apple-system,sans-serif; font-size:26px; color:#a7b0aa; }
  .foot b { color:#edf1ed; font-weight:600; }
</style></head><body>
  <div class="mark">ctx<span>.</span></div>
  <div class="wrap">
    <div class="kicker">WHERE IT RUNS, HONESTLY</div>
    <div class="head">Full where the hooks are. Insight everywhere else.</div>
    <div class="cols">
      <div class="col"><div class="ct">SURFACES</div>${rows(surfaces)}</div>
      <div class="col"><div class="ct">PLATFORMS</div>${rows(platforms)}</div>
    </div>
    <div class="foot">Trimming needs Claude Code hooks. On Desktop you get the tools and the dashboard, <b>not automatic trimming.</b></div>
  </div>
</body></html>`;

const browser = await chromium.launch({ headless: true, channel: 'chrome' });
const pg = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
await pg.setContent(html, { waitUntil: 'networkidle' });
await pg.waitForTimeout(150);
await pg.screenshot({ path: `${OUT}/diagram-support.png` });
console.log('rendered diagram-support.png');
await browser.close();
