import { describe, expect, it } from "vitest";

import { filterLabs, NO_FILTERS, type LabFilters } from "@/features/labs/lab-filters";
import type { Lab } from "@/features/labs/use-labs";
import { testLab as lab } from "@/features/labs/labs-test-kit";

const labs = [
  lab("sqli", { title: "SQL injection", skills: [{ id: "s", name: "Databases" }] }),
  lab("ad", { title: "Domain", category: "Active Directory", difficulty: 3, runtime: { runtime: "VM", architectures: [], providers: [] } }),
  lab("cloud", { title: "Bucket", difficulty: 2, runtime: { runtime: "DOCKER", architectures: [], providers: ["aws"] as never } }),
  lab("bare", { runtime: null }),
];
/** The ids `filters` keep out of `catalogue`, with the labs in `running` running and those in `solved` solved. */
const idsOf = (catalogue: Lab[], running: Set<string>, solved: Set<string>) => (filters: Partial<LabFilters>) =>
  filterLabs(catalogue, { ...NO_FILTERS, ...filters }, (l) => running.has(l.id), solved).map((l) => l.id);
const ids = idsOf(labs, new Set(["ad"]), new Set(["sqli"]));

describe("filterLabs", () => {
  it("keeps everything with no filter", () => {
    expect(ids({})).toEqual(["sqli", "ad", "cloud", "bare"]);
  });
  it("searches titles, categories and skills, ignoring case and spaces", () => {
    expect(ids({ query: "  active " })).toEqual(["ad"]);
    expect(ids({ query: "DATABASES" })).toEqual(["sqli"]);
  });
  it("filters by runtime, CLOUD meaning labs that run on AWS", () => {
    expect(ids({ runtime: "VM" })).toEqual(["ad"]);
    expect(ids({ runtime: "DOCKER" })).toEqual(["sqli", "cloud"]);
    expect(ids({ runtime: "CLOUD" })).toEqual(["cloud"]);
  });
  it("filters by level", () => {
    expect(ids({ difficulty: 2 })).toEqual(["cloud"]);
  });
  it("filters by status, to do being neither solved nor running", () => {
    expect(ids({ status: "running" })).toEqual(["ad"]);
    expect(ids({ status: "solved" })).toEqual(["sqli"]);
    expect(ids({ status: "todo" })).toEqual(["cloud", "bare"]);
  });
});

// Odd titles and providers, and one lab both solved and running.
const oddLabs = [
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
const oddIds = idsOf(oddLabs, new Set(["both"]), new Set(["both"]));

// The search is a plain substring match: nothing in the query is a pattern.
describe("filterLabs: the query", () => {
  it("treats regex metacharacters literally", () => {
    expect(oddIds({ query: "c++" })).toEqual(["cpp"]);
    expect(oddIds({ query: "(web)" })).toEqual(["shell"]);
    expect(oddIds({ query: "a.b" })).toEqual(["dot"]);
    expect(oddIds({ query: "[x" })).toEqual(["bracket"]);
    expect(oddIds({ query: "." })).toEqual(["dot"]);
  });

  it("matches accented text as typed, in any case", () => {
    expect(oddIds({ query: "élé" })).toEqual(["priv"]);
    expect(oddIds({ query: "PRIVILÈGES" })).toEqual(["priv"]);
  });

  it("the search ignores accents", () => {
    // A player typing "elevation" (no accents, as on many keyboards) finds nothing in a French
    // catalogue entry titled "Élévation de privilèges"; the same for "delegation" in its
    // description. Suggested fix: fold both sides with `.normalize("NFD").replace(/\p{M}/gu, "")`.
    expect(oddIds({ query: "elevation" })).toEqual(["priv"]);
    expect(oddIds({ query: "delegation" })).toEqual(["priv"]);
  });

  it("a whitespace-only query keeps everything, and the words match as one phrase", () => {
    expect(oddIds({ query: "   " })).toHaveLength(oddLabs.length);
    expect(oddIds({ query: "basics c++" })).toEqual([]);
    expect(oddIds({ query: "  c++ basics  " })).toEqual(["cpp"]);
  });

  it("searches the description and the skills, but never the id or slug", () => {
    expect(oddIds({ query: "kerberos" })).toEqual(["priv"]);
    expect(oddIds({ query: "forensics" })).toEqual(["zzz-secret"]);
    expect(oddIds({ query: "zzz" })).toEqual([]);
  });
});

describe("filterLabs: status and runtime", () => {
  it("a running lab that is also solved shows under running and solved, never under to do", () => {
    expect(oddIds({ status: "running" })).toEqual(["both"]);
    expect(oddIds({ status: "solved" })).toEqual(["both"]);
    expect(oddIds({ status: "todo" })).not.toContain("both");
  });

  it("the cloud filter keeps labs that list any of the six clouds, and leaves server providers out", () => {
    expect(oddIds({ runtime: "CLOUD" })).toEqual(["oci"]);
    expect(oddIds({ runtime: "VM" })).toEqual(["pve"]);
  });

  it("difficulty 0 means any level, and a lab without a runtime passes only the any-runtime filter", () => {
    expect(oddIds({ difficulty: 0 })).toHaveLength(oddLabs.length);
    const bare = [lab("bare", { runtime: null, difficulty: 0 })];
    const pick = (f: Partial<LabFilters>) => filterLabs(bare, { ...NO_FILTERS, ...f }, () => false, new Set()).length;
    expect(pick({})).toBe(1);
    expect(pick({ runtime: "DOCKER" })).toBe(0);
    expect(pick({ runtime: "CLOUD" })).toBe(0);
    expect(pick({ difficulty: 1 })).toBe(0);
  });
});
