import { describe, expect, it } from "vitest";

import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import type { Lab } from "@/features/labs/use-labs";

const lab = (id: string, p: Partial<Lab> = {}): Lab => ({
  id,
  slug: id,
  title: id,
  description: null,
  question: null,
  difficulty: 1,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: [], providers: [] },
  skills: [],
  ...p,
});

const labs = [
  lab("cpp", { title: "C++ basics" }),
  lab("shell", { title: "(web) shell" }),
  lab("dot", { title: "a.b" }),
  lab("bracket", { title: "[x] marks" }),
  lab("priv", { title: "Élévation de privilèges", description: "Kerberos et délégation" }),
  lab("zzz-secret", { title: "Other", skills: [{ id: "s", name: "Forensics" }] }),
  lab("oci", { title: "Bucket", runtime: { runtime: "DOCKER", architectures: [], providers: ["oci"] as never } }),
  lab("pve", { title: "Proxmox", runtime: { runtime: "VM", architectures: [], providers: ["proxmox"] as never } }),
  lab("both", { title: "Solved and running" }),
];
const running = new Set(["both"]);
const solved = new Set(["both"]);
const ids = (f: Partial<LabFilters>) => filterLabs(labs, { ...NO_FILTERS, ...f }, (l) => running.has(l.id), solved).map((l) => l.id);

// The search is a plain substring match: nothing in the query is a pattern.
describe("filterLabs: the query", () => {
  it("treats regex metacharacters literally", () => {
    expect(ids({ query: "c++" })).toEqual(["cpp"]);
    expect(ids({ query: "(web)" })).toEqual(["shell"]);
    expect(ids({ query: "a.b" })).toEqual(["dot"]);
    expect(ids({ query: "[x" })).toEqual(["bracket"]);
    expect(ids({ query: "." })).toEqual(["dot"]);
  });

  it("matches accented text as typed, in any case", () => {
    expect(ids({ query: "élé" })).toEqual(["priv"]);
    expect(ids({ query: "PRIVILÈGES" })).toEqual(["priv"]);
  });

  it("the search ignores accents", () => {
    // A player typing "elevation" (no accents, as on many keyboards) finds nothing in a French
    // catalogue entry titled "Élévation de privilèges"; the same for "delegation" in its
    // description. Suggested fix: fold both sides with `.normalize("NFD").replace(/\p{M}/gu, "")`.
    expect(ids({ query: "elevation" })).toEqual(["priv"]);
    expect(ids({ query: "delegation" })).toEqual(["priv"]);
  });

  it("a whitespace-only query keeps everything, and the words match as one phrase", () => {
    expect(ids({ query: "   " })).toHaveLength(labs.length);
    expect(ids({ query: "basics c++" })).toEqual([]);
    expect(ids({ query: "  c++ basics  " })).toEqual(["cpp"]);
  });

  it("searches the description and the skills, but never the id or slug", () => {
    expect(ids({ query: "kerberos" })).toEqual(["priv"]);
    expect(ids({ query: "forensics" })).toEqual(["zzz-secret"]);
    expect(ids({ query: "zzz" })).toEqual([]);
  });
});

describe("filterLabs: status and runtime", () => {
  it("a running lab that is also solved shows under running and solved, never under to do", () => {
    expect(ids({ status: "running" })).toEqual(["both"]);
    expect(ids({ status: "solved" })).toEqual(["both"]);
    expect(ids({ status: "todo" })).not.toContain("both");
  });

  it("the cloud filter keeps labs that list any of the six clouds, and leaves server providers out", () => {
    expect(ids({ runtime: "CLOUD" })).toEqual(["oci"]);
    expect(ids({ runtime: "VM" })).toEqual(["pve"]);
  });

  it("difficulty 0 means any level, and a lab without a runtime passes only the any-runtime filter", () => {
    expect(ids({ difficulty: 0 })).toHaveLength(labs.length);
    const bare = [lab("bare", { runtime: null, difficulty: 0 })];
    const pick = (f: Partial<LabFilters>) => filterLabs(bare, { ...NO_FILTERS, ...f }, () => false, new Set()).length;
    expect(pick({})).toBe(1);
    expect(pick({ runtime: "DOCKER" })).toBe(0);
    expect(pick({ runtime: "CLOUD" })).toBe(0);
    expect(pick({ difficulty: 1 })).toBe(0);
  });
});
