import { describe, expect, it } from "vitest";
import { holdsOf } from "@/lib/dev-mock/holds";
import { scenarioFrom } from "@/lib/dev-mock/scenario";

/** Whether `p` has settled once the callbacks already due have run (a timer runs after them). */
const settled = (p: Promise<void>) =>
  Promise.race([
    p.then(() => true),
    new Promise<boolean>((resolve) => {
      setTimeout(() => {
        resolve(false);
      }, 0);
    }),
  ]);

describe("the steps a test holds in the preview", () => {
  it("are read from the URL, leaving out names nothing can hold", () => {
    expect(scenarioFrom("?mock&hold=provision,nope,hosted").held).toEqual(["provision", "hosted"]);
    expect(scenarioFrom("?mock").held).toEqual([]);
  });

  it("keep a held step waiting until the test releases it, and from then on let it go on", async () => {
    const holds = holdsOf(["provision"]);
    const waiting = holds.passed("provision");
    expect(holds.isHeld("provision")).toBe(true);
    expect(await settled(waiting)).toBe(false);

    holds.release("provision");
    expect(holds.isHeld("provision")).toBe(false);
    expect(await settled(waiting)).toBe(true);
    expect(await settled(holds.passed("provision"))).toBe(true);
  });

  it("let a step the URL doesn't hold go on at once", async () => {
    const holds = holdsOf(["pm"]);
    expect(holds.isHeld("launch")).toBe(false);
    expect(await settled(holds.passed("launch"))).toBe(true);
  });

  it("refuse to release a step the URL doesn't hold, naming the switch that would", () => {
    const holds = holdsOf(["pm"]);
    expect(() => {
      holds.release("launch");
    }).toThrow("add hold=launch to the URL");
    expect(() => {
      holds.release("nope");
    }).toThrow("the preview doesn't hold 'nope'");
  });
});
