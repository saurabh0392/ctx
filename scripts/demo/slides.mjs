// Render the demo's connective slides to PNG in ctx's dark aesthetic, so they cut cleanly against the
// real dashboard screenshots. 1920x1080 at 2x.
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);

const OUT = process.argv[2] || '.';

const slides = [
  {
    file: 'slide-title.png',
    kicker: 'LOCAL-FIRST CONTEXT CONTROL',
    head: 'ctx makes your agent<br>leaner without<br>losing the thread.',
    sub: 'A local tool that trims what your coding agent sends and receives, and shows you the bill.',
  },
  {
    file: 'slide-reversible.png',
    kicker: 'NOTHING IS LOST',
    head: 'Every trim is<br>one call from whole.',
    sub: 'ctx leaves a marker on what it shortens. The agent calls <span class="g">ctx_expand</span> and gets the verbatim original back.',
  },
  {
    file: 'slide-restore.png',
    kicker: 'A USED TOOL IS NEVER CUT',
    head: 'Prune the dead weight.<br>Keep what works.',
    sub: 'A server in use is never disconnected. If ctx prunes something you later need, <span class="g">ctx_restore</span> brings it back next session, with a note of what you were doing.',
  },
  {
    file: 'slide-trust.png',
    kicker: 'YOURS, ON YOUR MACHINE',
    head: 'Local-first.<br>No telemetry.',
    sub: 'No account. Nothing about your code or prompts leaves the laptop. When ctx is unsure, it leaves your context alone.',
  },
  {
    file: 'slide-outro.png',
    kicker: 'THE ALPHA IS OPEN',
    head: 'Keep the context lean.<br>Stay in control.',
    sub: 'Install ctx, use your agent as you do today, and press <span class="g">Report</span> when something feels off.',
  },
  {
    file: 'slide-portfolio-outro.png',
    kicker: 'LOCAL-FIRST CONTEXT CONTROL',
    head: 'See where your context goes.<br>Take it back.',
    sub: 'A dashboard for your coding agent’s context: what it costs, what is safe to cut, and how much you got back.',
  },
];

function html(s) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
    * { margin:0; padding:0; box-sizing:border-box; }
    html,body { width:1920px; height:1080px; background:#0f1311; overflow:hidden; }
    .wrap { padding:150px 170px; height:100%; display:flex; flex-direction:column; justify-content:center; }
    .kicker { font-family:"SF Mono",Menlo,monospace; font-size:22px; letter-spacing:.32em; color:#35c88f; margin-bottom:44px; }
    .head { font-family:"Iowan Old Style",Palatino,Georgia,serif; font-size:96px; line-height:1.05; color:#edf1ed; font-weight:600; letter-spacing:-.01em; }
    .sub { font-family:"SF Pro Text",-apple-system,Helvetica,Arial,sans-serif; font-size:34px; line-height:1.5; color:#a7b0aa; margin-top:52px; max-width:1250px; }
    .g { color:#35c88f; font-family:"SF Mono",Menlo,monospace; font-size:30px; }
    .mark { position:absolute; top:60px; left:170px; font-family:"SF Pro Text",-apple-system,sans-serif; font-weight:800; font-size:34px; color:#edf1ed; letter-spacing:-.02em; }
    .mark span { color:#35c88f; }
  </style></head><body>
    <div class="mark">ctx<span>.</span></div>
    <div class="wrap">
      <div class="kicker">${s.kicker}</div>
      <div class="head">${s.head}</div>
      <div class="sub">${s.sub}</div>
    </div>
  </body></html>`;
}

const browser = await chromium.launch({ headless: true, channel: 'chrome' });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
for (const s of slides) {
  await page.setContent(html(s), { waitUntil: 'networkidle' });
  await page.waitForTimeout(200);
  await page.screenshot({ path: `${OUT}/${s.file}` });
  console.log('rendered', s.file);
}
await browser.close();
