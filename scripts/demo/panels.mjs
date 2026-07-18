// Render Claude-Code-style terminal panels for the demo, built from REAL ctx tool output captured in
// artifacts/. These show the actual trim marker, ctx_expand result, ctx_tools, and ctx_restore flow.
import { createRequire } from 'module';
import { readFileSync } from 'fs';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);

const OUT = process.argv[2];
const ART = `${OUT}/artifacts`;
const expand = JSON.parse(readFileSync(`${ART}/expand.json`, 'utf8'));
const tools = JSON.parse(readFileSync(`${ART}/tools.json`, 'utf8'));
const linear = tools.pruned_servers[0];

const CSS = `
  * { margin:0; padding:0; box-sizing:border-box; }
  html,body { width:1920px; height:1080px; background:#0f1311; overflow:hidden; }
  .wrap { padding:110px 150px; height:100%; display:flex; flex-direction:column; }
  .mark { position:absolute; top:56px; left:150px; font-family:-apple-system,sans-serif; font-weight:800; font-size:30px; color:#edf1ed; }
  .mark span { color:#35c88f; }
  .kicker { font-family:"SF Mono",Menlo,monospace; font-size:20px; letter-spacing:.3em; color:#35c88f; margin-bottom:22px; }
  .head { font-family:"Iowan Old Style",Palatino,Georgia,serif; font-size:60px; line-height:1.08; color:#edf1ed; font-weight:600; letter-spacing:-.01em; margin-bottom:44px; }
  .term { background:#0b0f0d; border:1px solid #1b221d; border-radius:16px; padding:34px 40px; font-family:"SF Mono",Menlo,monospace; font-size:26px; line-height:1.62; color:#c9d2cc; box-shadow:0 30px 80px rgba(0,0,0,.4); }
  .tbar { display:flex; gap:10px; margin-bottom:26px; }
  .tbar i { width:14px; height:14px; border-radius:50%; background:#2a332d; display:inline-block; }
  .call { color:#edf1ed; } .dot { color:#35c88f; } .arm { color:#5b6660; }
  .g { color:#35c88f; } .dim { color:#7f8a83; } .amber { color:#e0a44a; } .white { color:#edf1ed; }
  .mk { color:#6f9f88; font-style:italic; display:block; margin-top:14px; }
  .flow { color:#5b6660; margin:22px 0 6px; font-size:22px; letter-spacing:.05em; }
  .take { margin-top:auto; font-family:-apple-system,sans-serif; font-size:30px; color:#a7b0aa; padding-top:40px; }
  .take b { color:#edf1ed; font-weight:600; }
`;

function page(kicker, head, termHTML, take) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>${CSS}</style></head><body>
    <div class="mark">ctx<span>.</span></div>
    <div class="wrap">
      <div class="kicker">${kicker}</div>
      <div class="head">${head}</div>
      <div class="term"><div class="tbar"><i></i><i></i><i></i></div>${termHTML}</div>
      <div class="take">${take}</div>
    </div></body></html>`;
}

const panels = [
  {
    file: 'panel-trim.png',
    kicker: 'TRIM, IN PLACE',
    head: 'A noisy result, shortened where the agent reads it.',
    term:
      `<div><span class="dot">⏺</span> <span class="call">Bash</span>(git status)</div>` +
      `<div class="arm">  ⎿  On branch main</div>` +
      `<div class="arm">     Staged (38): modified: src/file_5.rs, modified: src/file_7.rs, modified: src/file_9.rs, <span class="dim">…</span></div>` +
      `<span class="mk">  [ctx trimmed this output to save context. Full original id: 10ea6ed364121eeb.<br>&nbsp;&nbsp;To read all of it, call the ctx_expand tool, or run: ctx expand 10ea6ed364121eeb]</span>`,
    take: `<b>2,323 characters down to 588.</b> The agent keeps working on the lean version. Nothing is deleted.`,
  },
  {
    file: 'panel-expand.png',
    kicker: 'EXPAND ON DEMAND',
    head: 'Need the whole thing? One call brings it back.',
    term:
      `<div><span class="dot">⏺</span> <span class="call">ctx_expand</span>(id: <span class="g">"10ea6ed364121eeb"</span>)</div>` +
      `<div class="arm">  ⎿  {</div>` +
      `<div class="arm">       "tool": <span class="white">"${expand.tool}"</span>,  "source": <span class="white">"${expand.source}"</span>,</div>` +
      `<div class="arm">       "chars": <span class="g">${expand.chars}</span>,</div>` +
      `<div class="arm">       "original": <span class="dim">"On branch main\\n…the full 80-file status, verbatim"</span></div>` +
      `<div class="arm">     }</div>`,
    take: `<b>Every byte back, ${expand.chars} characters.</b> Reversible by design, not best effort.`,
  },
  {
    file: 'panel-tools.png',
    kicker: 'PRUNE THE DEAD WEIGHT',
    head: 'Idle tools leave the menu. Used tools never do.',
    term:
      `<div><span class="dot">⏺</span> <span class="call">ctx_tools</span>()</div>` +
      `<div class="arm">  ⎿  pruned_servers:</div>` +
      `<div class="arm">       <span class="white">${linear.server}</span> &middot; <span class="amber">${linear.status.replace(/_/g, ' ')}</span></div>` +
      `<div class="arm">       <span class="g">${linear.dead_tools_denied} idle tools denied</span>, the tools you use are kept</div>`,
    take: `A server you use is <b>never disconnected wholesale.</b> It only sheds what sits idle.`,
  },
  {
    file: 'panel-restore.png',
    kicker: 'REACH IT BACK',
    head: 'Pruned something you now need? Get it back, with your task.',
    term:
      `<div><span class="dot">⏺</span> <span class="call">ctx_restore</span>(tool: <span class="g">"Linear"</span>, tasks: <span class="g">"finish the recovery trace"</span>)</div>` +
      `<div class="arm">  ⎿  Queued "Linear" to come back next session. Your note was saved.</div>` +
      `<div class="flow">↓ next session, first prompt</div>` +
      `<div class="arm">  <span class="g">Restored MCP tools from a previous session (available now):</span></div>` +
      `<div class="arm">  - Linear: finish wiring the recovery trace to the dashboard</div>`,
    take: `The prune is reversible too, and the work you were blocked on <b>rides along to the next session.</b>`,
  },
];

const browser = await chromium.launch({ headless: true, channel: 'chrome' });
const pg = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
for (const p of panels) {
  await pg.setContent(page(p.kicker, p.head, p.term, p.take), { waitUntil: 'networkidle' });
  await pg.waitForTimeout(150);
  await pg.screenshot({ path: `${OUT}/${p.file}` });
  console.log('rendered', p.file);
}
await browser.close();
