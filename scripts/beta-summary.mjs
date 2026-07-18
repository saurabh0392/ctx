#!/usr/bin/env node
// Aggregate local copies of private check-in objects. Output never contains participant IDs,
// labels, answers, paths, or individual rows. Keep input files and any roster mapping outside git.

import { readdirSync, readFileSync, statSync } from 'fs';
import { join } from 'path';

const root = process.argv[2];
if (!root) {
  console.error('usage: node scripts/beta-summary.mjs <downloaded-checkins-directory>');
  process.exit(2);
}

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? files(path) : entry.name.endsWith('.json') ? [path] : [];
  });
}

const rows = [];
let skipped = 0;
for (const file of files(root)) {
  if (!statSync(file).isFile()) continue;
  try {
    const value = JSON.parse(readFileSync(file, 'utf8'));
    if (value.schema === 'ctx.beta-checkin.v1' && value.snapshot && value.answers) {
      rows.push({ ...value, _received: Date.parse(value.received_at || '') || statSync(file).mtimeMs });
    } else {
      skipped++;
    }
  } catch { skipped++; }
}

// Day 7 and day 21 snapshots are cumulative. Use only the latest check-in per participant for
// product totals and answers, otherwise the same person's sessions/reclaimed tokens are counted
// twice. Raw check-in volume remains visible separately.
const latestByParticipant = new Map();
for (const row of rows) {
  const id = String(row.snapshot.participant_id || '');
  if (!id) { skipped++; continue; }
  const prior = latestByParticipant.get(id);
  if (!prior || row._received >= prior._received) latestByParticipant.set(id, row);
}
const latest = [...latestByParticipant.values()];
const count = (field, answer) => latest.filter((r) => String(r.answers[field] || '').trim().toLowerCase() === answer).length;
const sum = (field) => latest.reduce((n, r) => n + Math.max(0, Number(r.snapshot[field]) || 0), 0);
const average = (field) => latest.length ? Math.round(sum(field) / latest.length) : 0;

const summary = {
  schema: 'ctx.beta-summary.v1',
  generated_at: new Date().toISOString(),
  checkins: rows.length,
  skipped_files: skipped,
  unique_participants: latest.length,
  context_bill_ready_participants: latest.filter((r) => r.snapshot.bill_ready === true).length,
  active_on_or_after_day_8: latest.filter((r) => Number(r.snapshot.active_days_total) >= 8 && Number(r.snapshot.active_days_last7) > 0).length,
  learned_something_nonempty: latest.filter((r) => String(r.answers.learned_something || '').trim().length > 0).length,
  insight_actors: latest.filter((r) => Number(r.snapshot.insight_action_count) > 0).length,
  keep_using: { yes: count('keep_using', 'yes'), maybe: count('keep_using', 'maybe'), no: count('keep_using', 'no') },
  price_interest_25_per_developer: {
    yes: count('price_interest_25_per_developer', 'yes'),
    maybe: count('price_interest_25_per_developer', 'maybe'),
    no: count('price_interest_25_per_developer', 'no'),
  },
  latest_snapshot_totals: {
    average_active_days: average('active_days_total'),
    average_sessions: average('sessions_total'),
    reclaimed_tokens: sum('reclaimed_tokens'),
    reexpansions: sum('reexpansions'),
    insight_actions: sum('insight_action_count'),
  },
};

console.log(JSON.stringify(summary, null, 2));
