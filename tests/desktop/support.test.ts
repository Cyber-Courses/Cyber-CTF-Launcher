import { describe, expect, it } from "vitest";
import { containersChanged, linkTargets, mainButton, secondCopy } from "./lib/support.mjs";

// The specs' shared selectors (lib/support.mjs), as WebdriverIO gets them.

describe("the labs the deep-link spec links to", () => {
  // jarvis, debian13-quick, e2e.shell deep-link: the Labs list once the fixture was imported (its
  // rows first), then the catalogue's, which holds the lab the fixture was imported from.
  const imported = { label: "Open Supplier portal API, imported from a folder", title: "Supplier portal API", slug: "local-invoice-portal-api" };
  const caterpillar = {
    label: "Open Caterpillar: public fork-based poisoned pipeline execution",
    title: "Caterpillar: public fork-based poisoned pipeline execution",
    slug: "cicd-goat-caterpillar",
  };
  const supplier = { label: "Open Supplier portal API", title: "Supplier portal API", slug: "invoice-portal-api" };
  const installed = [{ id: "local-invoice-portal-api", title: "Supplier portal API" }];

  it("are the catalogue's first lab and the imported one, never the imported row taken for the catalogue's", () => {
    // The spec took the first row: a "catalogue" lab titled "Supplier portal API, imported from a
    // folder" (its button's name), which no page shows, so launch-link and not-started failed.
    expect(linkTargets([imported, caterpillar, supplier], installed)).toEqual([
      { kind: "catalogue", title: caterpillar.title, slug: "cicd-goat-caterpillar" },
      { kind: "imported", title: "Supplier portal API", slug: "local-invoice-portal-api" },
    ]);
  });

  it("pass over a catalogue lab whose title an imported lab shares, and do with what there is", () => {
    expect(linkTargets([imported, supplier, caterpillar], installed)[0].slug).toBe("cicd-goat-caterpillar");
    expect(linkTargets([supplier], [])).toEqual([{ kind: "catalogue", title: "Supplier portal API", slug: "invoice-portal-api" }]);
    expect(linkTargets([imported], installed)).toEqual([{ kind: "imported", title: "Supplier portal API", slug: "local-invoice-portal-api" }]);
    expect(linkTargets([], [])).toEqual([]);
  });
});

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
