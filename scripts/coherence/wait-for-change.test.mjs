import assert from 'node:assert/strict';
import test from 'node:test';

import { waitForChange } from './wait-for-change.mjs';

function fakeTime() {
  let elapsed = 0;
  return {
    now: () => elapsed,
    sleep: async (ms) => { elapsed += ms; },
    elapsed: () => elapsed,
  };
}

test('waitForChange observes a value that changes before the deadline', async () => {
  const clock = fakeTime();
  let reads = 0;
  const result = await waitForChange(
    async () => (++reads < 3 ? 'before' : 'after'),
    'before',
    { timeoutMs: 100, intervalMs: 10, now: clock.now, sleep: clock.sleep },
  );

  assert.deepEqual(result, { changed: true, value: 'after' });
  assert.equal(reads, 3);
  assert.equal(clock.elapsed(), 20);
});

test('waitForChange preserves the dead-control signal after the deadline', async () => {
  const clock = fakeTime();
  let reads = 0;
  const result = await waitForChange(
    async () => { reads += 1; return 'unchanged'; },
    'unchanged',
    { timeoutMs: 25, intervalMs: 10, now: clock.now, sleep: clock.sleep },
  );

  assert.deepEqual(result, { changed: false, value: 'unchanged' });
  assert.equal(reads, 4);
  assert.equal(clock.elapsed(), 25);
});
