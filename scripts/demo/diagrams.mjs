// Flow diagrams for the demo: local-first architecture, the trim/expand loop, the earn-it ladder, and
// the prune/restore loop. Box-and-arrow, rendered to 1920x1080 PNG in ctx's aesthetic.
import { createRequire } from 'module';
const require = createRequire(import.meta.url);
const pwPath = require.resolve('playwright-core', {
  paths: ['/Users/chikkupikku/Projects/ctx/scripts/coherence/node_modules'],
});
const { chromium } = require(pwPath);
const OUT = process.argv[2];

const CSS = `
  * { margin:0; padding:0; box-sizing:border-box; }
  html,body { width:1920px; height:1080px; background:#0f1311; overflow:hidden; }
  .mark { position:absolute; top:56px; left:150px; font-family:-apple-system,sans-serif; font-weight:800; font-size:30px; color:#edf1ed; }
  .mark span { color:#35c88f; }
  .wrap { padding:150px 150px; height:100%; display:flex; flex-direction:column; justify-content:center; }
  .kicker { font-family:"SF Mono",Menlo,monospace; font-size:20px; letter-spacing:.3em; color:#35c88f; margin-bottom:22px; }
  .head { font-family:"Iowan Old Style",Palatino,Georgia,serif; font-size:64px; line-height:1.06; color:#edf1ed; font-weight:600; letter-spacing:-.01em; margin-bottom:80px; max-width:1500px; }
  .flow { display:flex; align-items:stretch; gap:0; }
  .card { flex:1; background:#0b0f0d; border:1px solid #1b221d; border-radius:16px; padding:30px 28px; min-height:220px; display:flex; flex-direction:column; }
  .card.accent { border-color:#2d5f49; background:#0c1712; }
  .ct { font-family:"SF Mono",Menlo,monospace; font-size:19px; letter-spacing:.14em; color:#35c88f; margin-bottom:16px; text-transform:uppercase; }
  .cb { font-family:-apple-system,sans-serif; font-size:27px; line-height:1.4; color:#cdd5cf; }
  .arrow { display:flex; align-items:center; justify-content:center; width:70px; color:#35c88f; font-size:40px; flex:0 0 70px; }
  .caption { margin-top:72px; font-family:-apple-system,sans-serif; font-size:30px; color:#a7b0aa; }
  .caption b { color:#edf1ed; font-weight:600; }
`;

const A = '<div class="arrow">→</div>';
function card(t, b, accent) { return `<div class="card ${accent ? 'accent' : ''}"><div class="ct">${t}</div><div class="cb">${b}</div></div>`; }
function diagram(kicker, head, cards, caption) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>${CSS}</style></head><body>
    <div class="mark">ctx<span>.</span></div>
    <div class="wrap">
      <div class="kicker">${kicker}</div>
      <div class="head">${head}</div>
      <div class="flow">${cards.join(A)}</div>
      <div class="caption">${caption}</div>
    </div></body></html>`;
}

const diagrams = [
  {
    file: 'diagram-arch.png',
    html: diagram('LOCAL, BY DESIGN', 'ctx sits between your agent and its tools.',
      [card('Your agent', 'Claude Code, running your session'),
       card('ctx', 'a local hook that trims and prunes', true),
       card('Your tools', 'shell, files, and MCP servers')],
      'Every trim and prune happens on your machine. <b>No account. No telemetry.</b>'),
  },
  {
    file: 'diagram-trim.png',
    html: diagram('THE TRIM LOOP', 'Trim by default. Whole on demand.',
      [card('Tool result', 'a wall of text the agent skims once'),
       card('ctx trims it', 'shortened, with a recovery marker', true),
       card('Agent works lean', 'the context stays small'),
       card('ctx_expand', 'every byte back when it matters', true)],
      'The original is stored, hash-addressed. <b>Reversible, not lossy.</b>'),
  },
  {
    file: 'diagram-earn.png',
    html: diagram('EARN THE CUT', 'A tool only trims once your sessions prove it safe.',
      [card('Watching', 'measured, never touched'),
       card('Trial', 'trimmed on a slice of real work', true),
       card('Proving', 'did you re-read to recover?'),
       card('Earned', 'trims for good', true)],
      'Trialed on your own runs. <b>A tool in use is never cut.</b>'),
  },
  {
    file: 'diagram-restore.png',
    html: diagram('PRUNE, THEN REACH', 'Dead weight leaves. What you need comes back.',
      [card('Idle tools pruned', 'off the menu, tokens reclaimed'),
       card('Agent reaches', 'it needs a pruned tool'),
       card('ctx_restore', 'un-prunes it, saves your task', true),
       card('Next session', 'tool back, task carried forward', true)],
      'The reach is a signal. <b>ctx gets sharper every turn.</b>'),
  },
];

const browser = await chromium.launch({ headless: true, channel: 'chrome' });
const pg = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
for (const d of diagrams) {
  await pg.setContent(d.html, { waitUntil: 'networkidle' });
  await pg.waitForTimeout(150);
  await pg.screenshot({ path: `${OUT}/${d.file}` });
  console.log('rendered', d.file);
}
await browser.close();
