import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { checkIdsOf, commandsOf, listedCases, matchesSetupItem, readFacts, routesOf, settingsSectionsOf, setupItemsOf, tabsOf } from "./facts.ts";

describe("the readers", () => {
  it("take the commands registered with generate_handler!, by their last path segment", () => {
    const src = "builder.invoke_handler(tauri::generate_handler![\n    machine::system_check,\n    labs::lab_launch,\n])\n";
    expect(commandsOf(src)).toEqual(["system_check", "lab_launch"]);
  });

  it("take each page's route, the shell's screens and the Settings sections", () => {
    expect(routesOf(["page.tsx", "shell/page.tsx", "machine-setup\\page.tsx"])).toEqual(["route:/", "route:/machine-setup", "route:/shell"]);
    expect(tabsOf(`export type Tab = "home" | "labs";\nconst x = "other";`)).toEqual(["tab:home", "tab:labs"]);
    expect(settingsSectionsOf(`{ id: "account", label: "settings.tabs.account", icon }, { id: "x", label: "other" }`)).toEqual(["settings:account"]);
  });

  it("take the cases of a runner's listing, past any other line", () => {
    expect(listedCases(`note\n{"suite":"desktop","case":"lab-form-vm","spec":"lab-form"}\n`)).toEqual([
      { suite: "desktop", case: "lab-form-vm", spec: "lab-form" },
    ]);
  });

  it("take a spec's check ids, and the strings a skip loop could name", () => {
    expect([...checkIdsOf(`r.check("theme", ok); await r.step( "window", "w", f); r.pass("x", y);`)]).toEqual(["theme", "window", "x"]);
    expect(checkIdsOf(`for (const id of ["search", "list"]) r.skip(id, why);`).has("list")).toBe(true);
    expect(checkIdsOf(`const a = ["search"]; r.pass("x", 1);`).has("search")).toBe(false);
  });

  it("take the setup's items from its documentation, and match a reference to one", () => {
    const src = "//! The\n//! items: `wsl`, `engine:<id>`, then\n//! `selftest:vm`. Its record:\nfn x() {}\n";
    const items = setupItemsOf(src);
    expect(items).toEqual(["wsl", "engine:<id>", "selftest:vm"]);
    expect(["wsl", "engine:colima", "engine:<id>", "selftest:vm"].every((r) => matchesSetupItem(r, items))).toBe(true);
    expect(matchesSetupItem("engine:", items) || matchesSetupItem("selftest:docker", items)).toBe(false);
  });
});

describe("readFacts on this repository", () => {
  const facts = readFacts(fileURLToPath(new URL("../..", import.meta.url)));

  it("finds what every reader needs, so a moved file fails here and not in a quiet check", () => {
    expect(facts.commands.length).toBeGreaterThan(50);
    expect(facts.pages).toEqual(expect.arrayContaining(["route:/", "tab:labs", "settings:appearance"]));
    expect(facts.scenarios).toContain("vm-lab");
    expect(facts.desktopCases.get("lab-form-vm")).toBe("lab-form");
    expect(facts.desktopChecks.get("settings")?.has("theme")).toBe(true);
    expect(facts.setupItems).toContain("selftest:docker");
    expect(facts.read(path.join("tests", "README.md"))).toContain("# Testing");
    expect(facts.read("no/such/file")).toBeNull();
  });
});
