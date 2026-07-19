const defaultSleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// Poll an observable value until it differs from its pre-mutation value. The injectable clock and
// sleeper keep the timeout behavior deterministic in unit tests while production uses wall time.
export async function waitForChange(
  readValue,
  before,
  { timeoutMs = 5000, intervalMs = 100, now = Date.now, sleep = defaultSleep } = {},
) {
  const deadline = now() + timeoutMs;
  let value = await readValue();

  while (value === before) {
    const remaining = deadline - now();
    if (remaining <= 0) return { changed: false, value };
    await sleep(Math.min(intervalMs, remaining));
    value = await readValue();
  }

  return { changed: true, value };
}
