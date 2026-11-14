/** The mocked commands' answers still on their way (`installTauri` tracks them): what
 *  `settled()` waits for, rather than a fixed delay a loaded machine can outlast. */
let inflight = 0;

/** Tracks `answer` until it settles. */
export function track(answer: Promise<unknown>): void {
  inflight += 1;
  const done = () => {
    inflight -= 1;
  };
  answer.then(done, done);
}

/** How many answers are still on their way. */
export const inFlight = () => inflight;

/** Forgets every answer on its way (a new double installed). */
export function resetInFlight(): void {
  inflight = 0;
}

/** How long a wait for the answers gives up after: a test holding an answer without saying so
 *  (`held()`) fails on its assertions rather than hanging. */
export const SETTLE_LIMIT_MS = 15_000;

/** Waits, after at least `ms`, until no answer is on its way (or the limit passed). */
export async function waitForAnswers(ms: number): Promise<void> {
  const start = Date.now();
  const tick = (t: number) => new Promise((r) => setTimeout(r, t));
  await tick(ms);
  while (inflight > 0 && Date.now() - start < SETTLE_LIMIT_MS) await tick(0);
  await tick(0);
}
