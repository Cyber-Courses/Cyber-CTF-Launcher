import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { browserAnswer } from "@/lib/dev-mock/browser";
import { liveFrom } from "@/lib/dev-mock/live";
import { scenarioFrom } from "@/lib/dev-mock/scenario";

/** The preview's state with the machine held down (`hold=check`), and its system check. */
function heldDown() {
  const sc = scenarioFrom("?mock&hold=check");
  const ctx = { sc, live: liveFrom(sc) };
  return { live: ctx.live, check: () => browserAnswer("system_check", undefined, ctx) };
}

const DOWN = { code: "other", message: "docker info timed out after 20 s" };

describe("the preview's system check while a test holds the machine down", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("fails every check until the test lets the machine come back", async () => {
    const { check } = heldDown();
    for (let n = 0; n < 2; n++) {
      const answer = expect(check()).rejects.toEqual(DOWN);
      await vi.runAllTimersAsync();
      await answer;
    }
  });

  it("answers the check in flight when the machine comes back like the next one", async () => {
    const { live, check } = heldDown();
    const inFlight = check();
    live.holds.release("check");
    await vi.runAllTimersAsync();
    await expect(inFlight).resolves.toMatchObject({ dockerRunning: true });
    await expect(check()).resolves.toMatchObject({ dockerRunning: true });
  });
});
