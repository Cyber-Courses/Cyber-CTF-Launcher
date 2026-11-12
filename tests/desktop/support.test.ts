import { describe, expect, it } from "vitest";
import { containersChanged, mainButton, secondCopy } from "./lib/support.mjs";

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

describe("what changed of a lab's containers", () => {
  const p = "cyberctf-local-invoice-portal-api";
  const c = (service: string, state: string) => ({ name: `${p}-${service}-1`, state });
  // jarvis, debian13-quick, e2e.resilience park-resume: read as soon as the Overview showed the lab
  // running, then after its Pause was refused, the attack box "Start it with each lab" adds once the
  // launch ends having joined in between.
  const launched = [c("web", "running"), c("isoloom-init-database-1", "exited"), c("database", "running"), c("isoloom-probe", "exited")];
  const withBox = [c("attackbox", "running"), ...launched];

  it("names the box that joined, where the running count only differed: the refused Pause stopped nothing", () => {
    const running = (cs: { state: string }[]) => cs.filter((x) => x.state === "running").length;
    expect(running(launched)).not.toBe(running(withBox)); // what the spec took for the refusal's doing
    expect(containersChanged(launched, withBox)).toBe(`${p}-attackbox-1 added (running)`);
  });

  it("is nothing for the lab read again as it was, and each container stopped, gone or added otherwise", () => {
    expect(containersChanged(withBox, [...withBox].reverse())).toBeNull();
    const after = [c("attackbox", "running"), c("web", "exited"), c("database", "paused"), c("isoloom-probe", "exited"), c("extra", "created")];
    expect(containersChanged(withBox, after)).toBe(
      `${p}-web-1 running → exited, ${p}-isoloom-init-database-1-1 gone (was exited), ${p}-database-1 running → paused, ${p}-extra-1 added (created)`,
    );
  });
});
