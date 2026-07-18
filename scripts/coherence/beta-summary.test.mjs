import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const script = new URL('../beta-summary.mjs', import.meta.url).pathname;

function checkin(participant, day, reclaimed, answer, received) {
  return {
    received_at: received,
    schema: 'ctx.beta-checkin.v1',
    snapshot: {
      participant_id: participant,
      active_days_total: day,
      active_days_last7: day >= 8 ? 2 : day,
      sessions_total: day * 2,
      reclaimed_tokens: reclaimed,
      reexpansions: 1,
      insight_action_count: day >= 8 ? 2 : 0,
      bill_ready: true,
    },
    answers: {
      learned_something: 'The Context Bill changed my MCP setup.',
      keep_using: answer,
      price_interest_25_per_developer: answer,
    },
  };
}

test('uses only the latest cumulative snapshot per participant', () => {
  const root = mkdtempSync(join(tmpdir(), 'ctx-beta-summary-'));
  mkdirSync(join(root, 'nested'));
  writeFileSync(join(root, 'day7.json'), JSON.stringify(checkin('participant-a', 7, 100, 'maybe', '2026-07-01T00:00:00Z')));
  writeFileSync(join(root, 'nested', 'day21.json'), JSON.stringify(checkin('participant-a', 21, 400, 'yes', '2026-07-15T00:00:00Z')));
  writeFileSync(join(root, 'participant-b.json'), JSON.stringify(checkin('participant-b', 9, 250, 'maybe', '2026-07-16T00:00:00Z')));
  writeFileSync(join(root, 'bad.json'), '{');

  const result = JSON.parse(execFileSync(process.execPath, [script, root], { encoding: 'utf8' }));
  assert.equal(result.checkins, 3);
  assert.equal(result.unique_participants, 2);
  assert.equal(result.skipped_files, 1);
  assert.equal(result.latest_snapshot_totals.reclaimed_tokens, 650);
  assert.deepEqual(result.keep_using, { yes: 1, maybe: 1, no: 0 });
  assert.equal(result.active_on_or_after_day_8, 2);
  assert.equal(JSON.stringify(result).includes('participant-a'), false);
});
