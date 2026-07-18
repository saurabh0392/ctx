#!/usr/bin/env node
// Runs the fitcheck persona review via the GitHub Copilot API.
// Reads the skill context, personas doc, rubric, and target file; embeds them in a single
// prompt; and calls the Copilot chat-completions endpoint.  Prints the full model response to
// stdout so fitcheck-ci.sh can parse the FITCHECK verdict line.
//
// Usage:  node fitcheck-engine.mjs <target-path>
//         <target-path> defaults to src/dashboard.html
//
// Env:
//   GITHUB_TOKEN (or GH_TOKEN)   required — the standard Actions token works when Copilot is
//                                 enabled for the repository's organization.
//   FITCHECK_MODEL               optional model name passed to the Copilot API (default: gpt-4o)
//
// Exit codes:
//   0  model output written to stdout
//   2  setup / API error (hard fail)
//   3  Copilot access denied (401/403) — caller should skip gracefully

import { readFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repo = resolve(__dirname, '../..');
const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN || '';

if (!token) {
  process.stderr.write('fitcheck-engine: GITHUB_TOKEN not set\n');
  process.exit(2);
}

const target = process.argv[2] || 'src/dashboard.html';

function read(relPath) {
  try {
    return readFileSync(resolve(repo, relPath), 'utf8');
  } catch (e) {
    process.stderr.write(`fitcheck-engine: could not read ${relPath}: ${e.message}\n`);
    process.exit(2);
  }
}

const skill          = read('.claude/skills/fitcheck/SKILL.md');
const rubric         = read('.claude/skills/fitcheck/rubric.md');
const reportTemplate = read('.claude/skills/fitcheck/report-template.md');
const personas       = read('docs/personas-ctx.md');
const targetContent  = read(target);

const prompt = `You are running the fitcheck persona review for the ctx dashboard. \
Follow the procedure below precisely. Be honest -- this gates a release.

## Skill instructions

${skill}

## Full persona details (docs/personas-ctx.md)

${personas}

## Scoring rubric (rubric.md)

${rubric}

## Report template (report-template.md)

${reportTemplate}

## Target file: ${target}

\`\`\`html
${targetContent}
\`\`\`

Run the full fitcheck now: scope=all, mode=brief. Walk every persona through the target. \
Score every dimension per the rubric. Include both empty and populated states. \
Be honest -- this gates a release. \
As the very last line of your reply, print exactly one machine line:
FITCHECK verdict=<Ship|Iterate|Rework> overall=<number> coherence=<number>`;

const model = process.env.FITCHECK_MODEL || 'gpt-4o';

let resp;
try {
  resp = await fetch('https://api.githubcopilot.com/chat/completions', {
    method: 'POST',
    headers: {
      'Authorization': 'Bearer ' + token,
      'Content-Type': 'application/json',
      'Copilot-Integration-Id': 'ctx-fitcheck',
    },
    body: JSON.stringify({
      model,
      messages: [{ role: 'user', content: prompt }],
      max_tokens: 4096,
    }),
  });
} catch (e) {
  process.stderr.write(`fitcheck-engine: request failed: ${e.message}\n`);
  process.exit(2);
}

if (!resp.ok) {
  const body = await resp.text();
  process.stderr.write(`fitcheck-engine: Copilot API ${resp.status}: ${body}\n`);
  process.exit(resp.status === 401 || resp.status === 403 ? 3 : 2);
}

const data = await resp.json();
const content = data.choices?.[0]?.message?.content;
if (!content) {
  process.stderr.write('fitcheck-engine: empty response from Copilot API\n');
  process.exit(2);
}

process.stdout.write(content + '\n');
