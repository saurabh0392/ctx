// Live data builder for the ctx roadmap pipeline.
//
// Reads LINEAR_PAT from .env server-side only (never sent to the browser),
// fetches the ctx team's Linear issues over GraphQL, folds them into the
// authored roadmap components below, adds git history and the Rust modules,
// and returns the object the page renders. When no LINEAR_PAT is present it
// falls back to the last cached issue snapshot so the page still renders.
import { readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(__dirname, '..');
export const TEMPLATE = join(__dirname, 'status-template.html');
export const SNAPSHOT = join(ROOT, 'docs', 'roadmap.html');
export const CACHE = join(__dirname, 'status-cache.json');

const TEAM_KEY = 'CTX';
const STAT = ['Done', 'In Review', 'In Progress', 'Todo', 'Backlog'];
const ORDER = { Done: 0, 'In Review': 1, 'In Progress': 2, Todo: 3, Backlog: 4, Canceled: 5, Duplicate: 6 };

// The roadmap. Each component is a station in the pipeline toward the vision:
// ctx is the context truth and safety layer. `issueIds` attaches Linear work to
// the component; anything unmapped lands in a final "Unsorted" station so new
// tickets never disappear. The narrative is authored and honest: what is shipped,
// what is moving, and what still stands between us and the component's vision.
export const COMPONENTS = [
  {
    id: 'proof',
    name: 'Proof and measurement',
    tag: 'The differentiator',
    vision:
      'Prove, on your own work, whether trimming a tool costs you corrections or re-reads, with real confidence intervals instead of vibes.',
    narrative: {
      done: [
        'Causal before/after engine: compare runs ctx wanted to trim, split by whether the trim was applied, with Wilson and Newcombe intervals (SAU-150).',
        'Nearest-preceding attribution so one correction is never double counted across decisions.',
        'Backend API and Proof page: GET /api/context/proof and POST /api/context/trial, computed in Rust so the page can never disagree with the live gate (ADR 0002).',
      ],
      doing: [
        'CTX-2 in review: the Proof page and its API land with the dashboard revamp PR.',
      ],
      remaining: [
        'Stratify proof by reference vs working reads so Read numbers are honest after the edit-intent guard (CTX-9).',
        'Surface the same before/after for more tools as each one earns a trial.',
      ],
    },
    issueIds: ['CTX-2'],
    modules: ['src/stats.rs', 'src/context_ctl.rs', 'src/db.rs (causal_tool_outcomes)'],
  },
  {
    id: 'safety',
    name: 'Safety and governance',
    tag: 'Fail closed, earn trust',
    vision:
      'Never change context unless your data has earned it, and never trim a file the agent is about to edit.',
    narrative: {
      done: [
        'Evidence-gated activation: a tool only auto-activates when the causal bar clears, and it fails closed until a trial collects the trimmed arm.',
        'Read edit-intent guard, phase 1: a Read only trims when it is a reference read, never a working file (CTX-8, ADR 0001).',
        'Deliberate, one-tool-at-a-time trim trials so the after arm is collected on purpose, not by accident.',
      ],
      doing: [],
      remaining: [
        'Phase 2 session working-set guard: protect any file the agent has touched this session (CTX-9).',
        'Extend the guard pattern beyond Read as more tools get trialed.',
      ],
    },
    issueIds: ['CTX-8', 'CTX-9'],
    modules: ['src/compress/activation.rs', 'src/compress/edit_intent.rs', 'src/agent.rs'],
  },
  {
    id: 'learning',
    name: 'Learning that sharpens',
    tag: 'Yours, not the average',
    vision:
      'A local model that learns this repo and your habits from your own corrections and re-reads, and gets better the more you use ctx.',
    narrative: {
      done: [
        'Per-repo model training and versioning with a holdout AUC, exposed in the Improving view.',
        'Model history so you can see each retrain sharpen on your newest sessions.',
      ],
      doing: [],
      remaining: [
        'Close the loop so retrains trigger from accumulated outcomes automatically.',
        'Use model signals to pick which tool to trial next.',
      ],
    },
    issueIds: [],
    modules: ['src/learn.rs', 'src/tuning.rs'],
  },
  {
    id: 'surfaces',
    name: 'Surfaces and ingest',
    tag: 'Where ctx plugs in',
    vision:
      'Capture every tool result across Claude Code and Cursor, hook in safely, and ingest sessions without ever losing the original.',
    narrative: {
      done: [
        'Claude Code hooks plus the filter proxy, with the full original always kept in the transcript.',
        'Compression pipeline per tool kind: read, grep, git, test, and mcp, with session de-dup and retain rules.',
        'Non-blocking ingest that reflects the just-finished turn.',
      ],
      doing: [
        'Cursor surface and ingest so the same truth layer works outside Claude Code.',
      ],
      remaining: [
        'Round out the Cursor outcome join so corrections and re-reads label cleanly there too.',
      ],
    },
    issueIds: [],
    modules: ['src/hook.rs', 'src/surface/', 'src/compress/'],
  },
  {
    id: 'dashboard',
    name: 'Dashboard and product',
    tag: 'Clear in 10 seconds',
    vision:
      'A dashboard a customer understands in ten seconds: Home, Proof, Activity. The truth and safety story, with no cost theater.',
    narrative: {
      done: [
        'Approved clickable prototype as the visual spec (CTX-7).',
        'New information architecture: Home, Proof, Activity plus Profiles and Settings; Savings and Prompt Stats retired (CTX-3, CTX-4, CTX-6, ADR 0003).',
        'Home reframe with an honest status line and headline Proof card, panel misalignment fixed (CTX-5).',
      ],
      doing: [
        'CTX-1 epic in progress: the whole revamp is up as one PR in review.',
      ],
      remaining: [
        'Prune the now-dead backend cost endpoints in a follow-up cleanup.',
        'Click-through polish after a few days of real usage.',
      ],
    },
    issueIds: ['CTX-1', 'CTX-3', 'CTX-4', 'CTX-5', 'CTX-6', 'CTX-7'],
    modules: ['src/dashboard.rs', 'src/dashboard_static/'],
  },
  {
    id: 'foundations',
    name: 'Foundations and reliability',
    tag: 'Never corrupt your state',
    vision:
      'Deterministic tests, a clean install every time, and config handling that never touches your real settings during tests.',
    narrative: {
      done: [
        'Test isolation for HOME and CTX_HOME so integration tests cannot corrupt live config.',
        'Clean teardown and setup cycle verified end to end on a fresh install.',
      ],
      doing: [],
      remaining: [
        'Serialize all HOME and CTX_HOME access in tests so the suite is deterministic in parallel (CTX-10).',
      ],
    },
    issueIds: ['CTX-10'],
    modules: ['src/config.rs', 'src/setup.rs', 'src/daemon.rs'],
  },
];

function loadEnv() {
  try {
    const txt = readFileSync(join(ROOT, '.env'), 'utf8');
    const env = {};
    for (const line of txt.split('\n')) {
      const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/);
      if (!m) continue;
      let v = m[2].trim();
      if ((v.startsWith('"') && v.endsWith('"')) || (v.startsWith("'") && v.endsWith("'"))) v = v.slice(1, -1);
      env[m[1]] = v;
    }
    return env;
  } catch {
    return {};
  }
}

async function fetchIssues(key, log) {
  const query = `query($after:String){ issues(first:100, after:$after, filter:{ team:{ key:{ eq:"${TEAM_KEY}" } } }){ nodes{ identifier title updatedAt url state{ name } project{ name } } pageInfo{ hasNextPage endCursor } } }`;
  let after = null, all = [], page = 0;
  do {
    const res = await fetch('https://api.linear.app/graphql', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: key },
      body: JSON.stringify({ query, variables: { after } }),
    });
    if (!res.ok) throw new Error(`Linear API ${res.status} ${res.statusText}`);
    const j = await res.json();
    if (j.errors) throw new Error('Linear GraphQL: ' + JSON.stringify(j.errors).slice(0, 220));
    const conn = j.data.issues;
    all.push(...conn.nodes);
    page++;
    log(`Fetched ${all.length} issues (page ${page})`, Math.min(62, 22 + all.length / 2));
    after = conn.pageInfo.hasNextPage ? conn.pageInfo.endCursor : null;
  } while (after);
  return all.map((i) => ({
    id: i.identifier,
    title: i.title,
    status: i.state?.name || '?',
    updatedAt: (i.updatedAt || '').slice(0, 10),
    url: i.url || '',
    project: i.project?.name || null,
  }));
}

function readCache() {
  try { return JSON.parse(readFileSync(CACHE, 'utf8')); } catch { return []; }
}

export async function buildData(log = () => {}) {
  log('Reading credentials', 3);
  const key = loadEnv().LINEAR_PAT;

  let issues;
  let source;
  if (key) {
    log(`Connecting to Linear (team ${TEAM_KEY})`, 12);
    issues = await fetchIssues(key, log);
    try { writeFileSync(CACHE, JSON.stringify(issues, null, 2)); } catch {}
    source = 'live';
  } else {
    log('No LINEAR_PAT, reading the cached snapshot', 30);
    issues = readCache();
    source = issues.length ? 'cache' : 'empty';
  }

  log(`Folding ${issues.length} issues into the roadmap`, 68);
  const byId = {};
  for (const it of issues) byId[it.id] = it;

  const counts = (lst) => { const c = {}; STAT.forEach((s) => (c[s] = 0)); lst.forEach((it) => { if (c[it.status] != null) c[it.status]++; }); return c; };
  const sortIssues = (lst) => lst.slice().sort((a, b) => (ORDER[a.status] ?? 9) - (ORDER[b.status] ?? 9) || a.id.localeCompare(b.id));

  const overall = {}; STAT.forEach((s) => (overall[s] = 0));
  const claimed = new Set();

  const components = COMPONENTS.map((C) => {
    const list = sortIssues((C.issueIds || []).map((id) => byId[id]).filter(Boolean));
    list.forEach((it) => claimed.add(it.id));
    const c = counts(list);
    STAT.forEach((s) => (overall[s] += c[s]));
    return { ...C, issues: list, counts: c, total: list.length };
  });

  // Anything not mapped still shows, so a new ticket is never silently dropped.
  const orphans = sortIssues(issues.filter((it) => !claimed.has(it.id)));
  if (orphans.length) {
    const c = counts(orphans);
    STAT.forEach((s) => (overall[s] += c[s]));
    components.push({
      id: 'unsorted',
      name: 'Unsorted',
      tag: 'Not yet placed',
      vision: 'New work that has not been mapped to a roadmap component yet. Map it in tools/status-data.mjs.',
      narrative: { done: [], doing: [], remaining: [] },
      modules: [],
      issues: orphans,
      counts: c,
      total: orphans.length,
    });
  }

  log('Reading git history', 80);
  const sh = (cmd) => { try { return execSync(cmd, { cwd: ROOT }).toString().trim(); } catch { return ''; } };
  const commits = sh('git log --oneline -10 --no-decorate').split('\n').filter(Boolean);
  const branch = sh('git rev-parse --abbrev-ref HEAD');

  log('Scanning modules', 90);
  let modules = [];
  try {
    const dirents = readdirSync(join(ROOT, 'src'), { withFileTypes: true });
    const dirs = dirents.filter((d) => d.isDirectory()).map((d) => d.name + '/');
    const files = dirents.filter((d) => d.isFile() && d.name.endsWith('.rs')).map((d) => d.name);
    modules = [...dirs.sort(), ...files.sort()];
  } catch {}

  const momentum = issues
    .slice()
    .sort((a, b) => (b.updatedAt || '').localeCompare(a.updatedAt || '') || b.id.localeCompare(a.id))
    .slice(0, 10);

  const totalReal = issues.length;
  const generatedAt = new Date().toISOString().slice(0, 16).replace('T', ' ');
  log('Rebuilding the pipeline', 100);
  return { generatedAt, source, overall, totalReal, components, momentum, commits, branch, modules };
}

export function renderHTML(data) {
  const tpl = readFileSync(TEMPLATE, 'utf8');
  const json = JSON.stringify(data).replace(/</g, '\\u003c');
  return tpl.replace('__DATA__', json).replace('__GEN__', data.generatedAt);
}

// Allow `node tools/status-data.mjs` to write a fresh snapshot without the server.
if (process.argv[1] && process.argv[1].endsWith('status-data.mjs')) {
  buildData((m, p) => console.log(`  [${String(p).padStart(3)}%] ${m}`)).then((data) => {
    if (!existsSync(join(ROOT, 'docs'))) throw new Error('docs/ missing');
    writeFileSync(SNAPSHOT, renderHTML(data));
    console.log(`\n  Wrote ${SNAPSHOT}`);
  }).catch((e) => { console.error('  Failed:', e.message); process.exit(1); });
}
