import { describe, expect, it } from "vitest";
import { mainButton, secondCopy } from "./lib/support.mjs";

// The specs' shared selectors (lib/support.mjs), as WebdriverIO gets them.

describe("a main-pane button by its labels", () => {
  it("is the first match, so a page showing two of the labels never trips WebdriverIO's strict selectors", () => {
    const asked: string[] = [];
    const b = {
      $: (selector: string) => {
        asked.push(selector);
        return selector;
      },
    };
    mainButton(b, "Stop & remove", "Stop & clean up");
    expect(asked).toEqual(['(//main//button[normalize-space()="Stop & remove" or normalize-space()="Stop & clean up"])[1]']);
  });
});

describe("a second copy of a lab", () => {
  const project = "cyberctf-local-invoice-portal-api";
  const c = (service: string, p = project, n = 1) => ({ name: `${p}-${service}-${String(n)}`, project: p, service });

  it("is not the attack box the copy gains: the rig's reimport-running read 2 -> 3 containers as one", () => {
    // jarvis, debian13-quick, e2e.resilience: web and database, then the attack box "Start it with
    // each lab" attached once the launch ended; one Compose project, each service once.
    const before = [c("web"), c("database")];
    const after = [c("attackbox"), c("web"), c("database")];
    expect(after.length > before.length).toBe(true); // what the spec took for a second copy
    expect(secondCopy(after)).toBeNull();
  });

  it("is a second Compose project, or a service running twice", () => {
    expect(secondCopy([c("web"), c("web", `${project}-2`)])).toBe(`2 Compose projects: ${project}, ${project}-2`);
    expect(secondCopy([c("web"), c("web", project, 2), c("database")])).toBe("a service runs more than once: web ×2");
    expect(secondCopy([])).toBeNull();
  });
});
