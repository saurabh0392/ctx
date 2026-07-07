// Claude-driven fix pipeline for a single GitHub issue. Triage -> implement -> validate for
// hallucination and accuracy -> gates -> re-fix (<=3) -> draft PR -> notify. Runs in GitHub Actions;
// see .github/workflows/agent-fix.yml and scripts/agent/README.md.
//
// Every knob below exists to bound cost and protect quality. The agent never merges: a human reviews
// the draft PR, and branch protection requires green CI on main.

import { execSync, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const REPO = process.env.GITHUB_REPOSITORY;                 // owner/repo
const ISSUE = parseInt(process.env.ISSUE_NUMBER || '0', 10);
const GH_TOKEN = process.env.GH_TOKEN;                      // Contents + Pull requests + Issues: write
const ANTHROPIC_API_KEY = process.env.ANTHROPIC_API_KEY;

// Cost + quality controls.
const MAX_ATTEMPTS = parseInt(process.env.MAX_ATTEMPTS || '3', 10);
const MAX_DIFF_LINES = parseInt(process.env.MAX_DIFF_LINES || '500', 10);
const MIN_CONFIDENCE = parseFloat(process.env.MIN_CONFIDENCE || '0.6');
const MODEL_CHEAP = process.env.MODEL_CHEAP || 'claude-sonnet-5';   // triage + validate
const MODEL_STRONG = process.env.MODEL_STRONG || 'claude-opus-4-8'; // implement
const TURNS = { triage: 10, implement: 40, validate: 12 };

let usdSpent = 0;
const fail = (msg) => { console.error('pipeline:', msg); process.exit(1); };
if (!REPO || !ISSUE || !GH_TOKEN || !ANTHROPIC_API_KEY) fail('missing REPO / ISSUE_NUMBER / GH_TOKEN / ANTHROPIC_API_KEY');

// ---- GitHub REST ----
async function gh(path, method = 'GET', body) {
  const res = await fetch(`https://api.github.com${path}`, {
    method,
    headers: { authorization: `Bearer ${GH_TOKEN}`, accept: 'application/vnd.github+json', 'content-type': 'application/json', 'user-agent': 'ctx-agent' },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`gh ${method} ${path} -> ${res.status} ${(await res.text()).slice(0, 200)}`);
  return res.status === 204 ? {} : res.json();
}
const comment = (bodyMd) => gh(`/repos/${REPO}/issues/${ISSUE}/comments`, 'POST', { body: bodyMd });
const addLabels = (labels) => gh(`/repos/${REPO}/issues/${ISSUE}/labels`, 'POST', { labels }).catch(() => {});

// ---- shell ----
const sh = (cmd, opts = {}) => execSync(cmd, { encoding: 'utf8', stdio: 'pipe', ...opts });
function gate(cmd) { // returns { ok, output } without throwing
  const r = spawnSync('bash', ['-lc', cmd], { encoding: 'utf8' });
  return { ok: r.status === 0, output: ((r.stdout || '') + (r.stderr || '')).slice(-4000) };
}

// ---- Claude (headless) ----
// The prompt (which embeds untrusted issue text) is passed as a single argv element to spawnSync with
// no shell, so it is never interpolated into a command line. The prompt itself also fences the issue.
function claude(prompt, model, maxTurns) {
  const r = spawnSync('claude', ['-p', prompt, '--model', model, '--max-turns', String(maxTurns), '--output-format', 'json', '--dangerously-skip-permissions'],
    { encoding: 'utf8', env: process.env, maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(`claude exited ${r.status}: ${(r.stderr || '').slice(-500)}`);
  let obj; try { obj = JSON.parse(r.stdout); } catch { obj = { result: r.stdout }; }
  if (obj.total_cost_usd) usdSpent += obj.total_cost_usd;
  return String(obj.result ?? '');
}
// Pull the first JSON object out of a model reply.
function jsonFrom(text, fallback) {
  const m = text.match(/\{[\s\S]*\}/); if (!m) return fallback;
  try { return JSON.parse(m[0]); } catch { return fallback; }
}

const fence = (s) => '```\n' + String(s || '').replace(/```/g, "'''").slice(0, 6000) + '\n```';

async function main() {
  const issue = await gh(`/repos/${REPO}/issues/${ISSUE}`);
  const author = issue.user?.login;
  const cc = author ? `@${author} ` : '';
  const issueCtx = `Issue #${ISSUE}: ${issue.title}\n\n${(issue.body || '').slice(0, 8000)}`;

  // 1. Triage (cheap). Decide fixable + confidence + scope. Bail early if not a good candidate.
  const triage = jsonFrom(claude(
    `You are triaging a bug for the ctx repo (a Rust CLI + dashboard). Read the issue below and the repo. ` +
    `Use ctx and normal tools to gather just enough context. Decide if this is a narrow, well-scoped fix an ` +
    `agent should attempt now (single area, small diff), not a broad refactor or a design question.\n\n` +
    `Reply with ONLY this JSON: {"fixable":bool,"confidence":0..1,"scope":"one sentence","files":["path"],"approach":"2-3 sentences","reason":"why / why not"}\n\n` +
    `--- issue ---\n${issueCtx}`,
    MODEL_CHEAP, TURNS.triage), { fixable: false, confidence: 0, reason: 'triage parse failed' });

  if (!triage.fixable || (triage.confidence || 0) < MIN_CONFIDENCE) {
    await addLabels(['needs-human']);
    await comment(`${cc}The fix agent looked at this and is handing it to a human.\n\n**Why:** ${triage.reason || 'low confidence'} (confidence ${triage.confidence ?? 0}).`);
    console.log('triage: not auto-fixable, stopped. cost $' + usdSpent.toFixed(3));
    return;
  }

  // 2. Branch.
  const branch = `agent/issue-${ISSUE}`;
  sh(`git config user.name "ctx-agent" && git config user.email "agent@ctx.local"`);
  sh(`git checkout -B ${branch}`);

  // 3. Implement -> validate -> gates, up to MAX_ATTEMPTS, feeding failures back in.
  let feedback = '';
  let passed = false;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    console.log(`--- attempt ${attempt}/${MAX_ATTEMPTS} ---`);
    claude(
      `Fix ctx issue #${ISSUE} on the current branch. Scope: ${triage.scope}. Approach: ${triage.approach}. ` +
      `Make the minimal correct change, match surrounding style, and do not invent APIs (grep to confirm every symbol exists). ` +
      `${feedback ? `\n\nThe previous attempt failed. Address this exactly:\n${feedback}` : ''}\n\n--- issue ---\n${issueCtx}`,
      MODEL_STRONG, TURNS.implement);

    const diff = sh(`git diff --unified=0`);
    const changed = sh(`git diff --stat`).trim();
    if (!diff.trim()) { feedback = 'No changes were made. Actually edit the files.'; continue; }
    const diffLines = diff.split('\n').filter((l) => /^[+-]/.test(l) && !/^[+-]{3}/.test(l)).length;
    if (diffLines > MAX_DIFF_LINES) {
      await addLabels(['needs-human']);
      await comment(`${cc}The fix agent produced a ${diffLines}-line change, over the ${MAX_DIFF_LINES}-line auto-fix limit, so this needs a human.\n\n${fence(changed)}`);
      console.log('diff too large, handed to human. cost $' + usdSpent.toFixed(3));
      return;
    }

    // Adversarial validation: hallucination + does-it-fix-it, before spending on gates.
    const v = jsonFrom(claude(
      `Adversarially review this diff for ctx issue #${ISSUE}. Check: every symbol/API used actually exists in the repo (grep), ` +
      `the change genuinely addresses the issue, no invented behaviour, no obvious regression. Be skeptical.\n\n` +
      `Reply ONLY: {"pass":bool,"problems":["..."]}\n\n--- issue ---\n${issueCtx}\n\n--- diff ---\n${diff.slice(0, 12000)}`,
      MODEL_CHEAP, TURNS.validate), { pass: false, problems: ['validation parse failed'] });
    if (!v.pass) { feedback = 'Validation found: ' + (v.problems || []).join('; '); console.log('validate failed:', feedback); continue; }

    // Deterministic gates: cheap-first.
    const t = gate('cargo test --quiet 2>&1');
    if (!t.ok) { feedback = 'cargo test failed:\n' + t.output; console.log('cargo test failed'); continue; }
    const coh = gate('bash scripts/coherence/coherence.sh 2>&1');
    if (!coh.ok) { feedback = 'coherence suite failed:\n' + coh.output; console.log('coherence failed'); continue; }

    passed = true; break;
  }

  if (!passed) {
    await addLabels(['needs-human']);
    await comment(`${cc}The fix agent could not produce a change that clears the gates after ${MAX_ATTEMPTS} attempts, so it is handing this to a human.\n\nLast blocker:\n${fence(feedback)}\n\n<sub>Agent cost: $${usdSpent.toFixed(2)}</sub>`);
    console.log('gave up after max attempts. cost $' + usdSpent.toFixed(3));
    return;
  }

  // 4. Commit, push, draft PR, link the issue.
  sh(`git add -A`);
  sh(`git commit -m "fix: address #${ISSUE}\n\nAgent-authored fix. Human review required.\n\nCloses #${ISSUE}\n\nCo-Authored-By: ctx-agent <agent@ctx.local>"`);
  sh(`git push -f origin ${branch}`);
  const base = process.env.DEFAULT_BRANCH || 'main';
  const pr = await gh(`/repos/${REPO}/pulls`, 'POST', {
    title: `fix: ${issue.title} (#${ISSUE})`, head: branch, base, draft: true,
    body: `Agent-authored fix for #${ISSUE}. **Human review required; do not merge without reading the diff.**\n\n` +
      `**Scope:** ${triage.scope}\n**Approach:** ${triage.approach}\n\nGates passed: cargo test, coherence suite.\n\nCloses #${ISSUE}\n\n<sub>Agent cost: $${usdSpent.toFixed(2)}</sub>`,
  });
  await addLabels(['agent-authored']);
  await comment(`${cc}A draft PR is ready for your review: ${pr.html_url}\n\nIt passed cargo test and the coherence suite. It will not merge until you approve it and CI is green. <sub>Cost: $${usdSpent.toFixed(2)}</sub>`);
  console.log('opened draft PR', pr.html_url, 'cost $' + usdSpent.toFixed(3));
}

main().catch(async (e) => {
  try { await comment(`The fix agent hit an error and stopped: ${String(e.message || e).slice(0, 300)}`); await addLabels(['needs-human']); } catch {}
  fail(e.message || String(e));
});
