// Local ctx roadmap server. Serves the committed snapshot at / and streams a
// live Linear refresh over SSE at /api/refresh. The LINEAR_PAT stays server-side.
// Run: node tools/status-server.mjs   (then open http://localhost:4318)
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { exec } from 'node:child_process';
import { buildData, renderHTML, SNAPSHOT } from './status-data.mjs';

const PORT = Number(process.env.STATUS_PORT) || 4318;

const server = createServer(async (req, res) => {
  const url = (req.url || '/').split('?')[0];

  if (url === '/' || url === '/index.html') {
    try {
      const html = existsSync(SNAPSHOT)
        ? readFileSync(SNAPSHOT, 'utf8')
        : renderHTML(await buildData());
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      res.end(html);
    } catch (e) {
      res.writeHead(500, { 'Content-Type': 'text/plain' });
      res.end('Failed to render: ' + e.message);
    }
    return;
  }

  if (url === '/api/refresh') {
    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    });
    const send = (event, obj) => { res.write(`event: ${event}\n`); res.write(`data: ${JSON.stringify(obj)}\n\n`); };
    try {
      const data = await buildData((msg, pct) => send('log', { msg, pct: Math.round(pct) }));
      try { writeFileSync(SNAPSHOT, renderHTML(data)); } catch {}
      send('done', data);
    } catch (e) {
      send('failed', { msg: e.message });
    }
    res.end();
    return;
  }

  res.writeHead(404, { 'Content-Type': 'text/plain' });
  res.end('not found');
});

server.listen(PORT, () => {
  const link = `http://localhost:${PORT}`;
  console.log(`\n  ctx roadmap  ->  ${link}`);
  console.log('  Refresh pulls live from Linear (team CTX) when LINEAR_PAT is in .env. Ctrl-C to stop.\n');
  if (process.platform === 'darwin' && !process.env.NO_OPEN) exec(`open ${link}`);
});
