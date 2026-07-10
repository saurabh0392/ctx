// Extra assets: the install terminal panel (alpha version) and the diagonal light/dark theme split
// (portfolio version). 1920x1080 at 2x.
import { createRequire } from 'module';
import { readFileSync } from 'fs';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);
const OUT = process.argv[2];
const dataUri = (f) => `data:image/png;base64,${readFileSync(f).toString('base64')}`;

const browser = await chromium.launch({ headless: true, channel: 'chrome' });
const pg = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });

// --- install panel ---------------------------------------------------------
const installCSS = `
  * { margin:0; padding:0; box-sizing:border-box; }
  html,body { width:1920px; height:1080px; background:#0f1311; overflow:hidden; }
  .mark { position:absolute; top:56px; left:150px; font-family:-apple-system,sans-serif; font-weight:800; font-size:30px; color:#edf1ed; }
  .mark span { color:#35c88f; }
  .wrap { padding:110px 150px; height:100%; display:flex; flex-direction:column; }
  .kicker { font-family:"SF Mono",Menlo,monospace; font-size:20px; letter-spacing:.3em; color:#35c88f; margin-bottom:22px; }
  .head { font-family:"Iowan Old Style",Palatino,Georgia,serif; font-size:60px; line-height:1.08; color:#edf1ed; font-weight:600; margin-bottom:44px; }
  .term { background:#0b0f0d; border:1px solid #1b221d; border-radius:16px; padding:34px 40px; font-family:"SF Mono",Menlo,monospace; font-size:26px; line-height:1.7; color:#c9d2cc; box-shadow:0 30px 80px rgba(0,0,0,.4); }
  .tbar { display:flex; gap:10px; margin-bottom:24px; } .tbar i { width:14px; height:14px; border-radius:50%; background:#2a332d; }
  .p { color:#35c88f; } .ok { color:#35c88f; } .dim { color:#7f8a83; } .w { color:#edf1ed; }
  .take { margin-top:auto; font-family:-apple-system,sans-serif; font-size:30px; color:#a7b0aa; padding-top:40px; } .take b { color:#edf1ed; font-weight:600; }
`;
const installHTML = `<!doctype html><html><head><meta charset="utf-8"><style>${installCSS}</style></head><body>
  <div class="mark">ctx<span>.</span></div>
  <div class="wrap">
    <div class="kicker">GET STARTED</div>
    <div class="head">One command. Then it runs itself.</div>
    <div class="term"><div class="tbar"><i></i><i></i><i></i></div>
      <div><span class="p">$</span> curl -fsSL ctx.sh/install | <span class="w">CTX_TOKEN</span>=your-token sh</div>
      <div class="dim">&nbsp;&nbsp;<span class="ok">✓</span> downloaded ctx for macOS arm64, checksum verified</div>
      <div class="dim">&nbsp;&nbsp;<span class="ok">✓</span> Claude Code detected</div>
      <div class="dim">&nbsp;&nbsp;<span class="ok">✓</span> hooks + MCP registered in ~/.claude/settings.json</div>
      <div class="dim">&nbsp;&nbsp;<span class="ok">✓</span> dashboard live at <span class="w">http://127.0.0.1:8789</span></div>
    </div>
    <div class="take">No repo, no Rust, no config. <b>ctx wires into your agent and starts watching in the background.</b></div>
  </div></body></html>`;
await pg.setContent(installHTML, { waitUntil: 'networkidle' });
await pg.waitForTimeout(150);
await pg.screenshot({ path: `${OUT}/install-panel.png` });
console.log('rendered install-panel.png');

// --- theme split (diagonal dark | light of the same Home view) -------------
const dark = dataUri(`${OUT}/shot-home.png`);
const light = dataUri(`${OUT}/shot-home-light.png`);
const splitHTML = `<!doctype html><html><head><meta charset="utf-8"><style>
  * { margin:0; padding:0; box-sizing:border-box; }
  html,body { width:1920px; height:1080px; overflow:hidden; background:#000; position:relative; }
  img { position:absolute; inset:0; width:1920px; height:1080px; object-fit:cover; }
  .light { clip-path: polygon(42% 0, 100% 0, 100% 100%, 58% 100%); }
  svg { position:absolute; inset:0; width:1920px; height:1080px; }
  .lbl { position:absolute; font-family:"SF Mono",Menlo,monospace; font-size:22px; letter-spacing:.3em; }
  .lbl.d { bottom:56px; left:80px; color:#8a938c; }
  .lbl.l { top:56px; right:80px; color:#5b6660; }
</style></head><body>
  <img class="dark" src="${dark}">
  <img class="light" src="${light}">
  <svg viewBox="0 0 1920 1080" preserveAspectRatio="none"><line x1="806" y1="0" x2="1114" y2="1080" stroke="#35c88f" stroke-width="3" opacity="0.9"/></svg>
  <div class="lbl d">DARK</div>
  <div class="lbl l">LIGHT</div>
</body></html>`;
await pg.setContent(splitHTML, { waitUntil: 'networkidle' });
await pg.waitForTimeout(250);
await pg.screenshot({ path: `${OUT}/theme-split.png` });
console.log('rendered theme-split.png');

await browser.close();
