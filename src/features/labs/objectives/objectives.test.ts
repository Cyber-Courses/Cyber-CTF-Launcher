// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  MAIN,
  afterSubmit,
  challengesFor,
  emptyProgress,
  hintsOf,
  initiallyOpen,
  loadObjectives,
  loadProgress,
  openHint,
  openSteps,
  submitObjective,
  usedHelp,
  withHint,
  withSteps,
  type Challenge,
} from "@/features/labs/objectives/objectives";
import { installTauri } from "@/test/tauri";
import { rejectWith } from "@/features/labs/labs-test-kit";

const challenge = (key: string, p: Partial<Challenge> = {}): Challenge => ({
  key,
  title: key,
  prompt: `Solve ${key}`,
  hintCount: 0,
  optional: false,
  points: null,
  steps: [],
  ...p,
});

describe("objectives", () => {
  it("falls back to the lab's question as its implicit main challenge", () => {
    expect(challengesFor([], "What is the total?", "Objective")).toEqual([
      { key: MAIN, title: "Objective", prompt: "What is the total?", hintCount: 0, optional: false, points: null, steps: [] },
    ]);
    expect(challengesFor([], null, "Objective")).toBeNull();
    const own = [challenge("a")];
    expect(challengesFor(own, "ignored", "Objective")).toBe(own);
  });

  it("counts only required challenges in empty progress", () => {
    expect(emptyProgress([challenge("a"), challenge("b", { optional: true }), challenge("c")]).requiredTotal).toBe(2);
  });

  it("applies a submit: first correct solves count once, wrong ones change nothing", () => {
    const list = [challenge("a"), challenge("b", { optional: true }), challenge("c", { steps: [{ key: "c1", title: "c1", prompt: null, hintCount: 0 }] })];
    const p0 = emptyProgress(list);
    expect(afterSubmit(p0, list, "a", { correct: false, alreadySolved: false, labCompleted: false })).toBe(p0);
    const p1 = afterSubmit(p0, list, "a", { correct: true, alreadySolved: false, labCompleted: false });
    expect(p1).toMatchObject({ solved: ["a"], requiredSolved: 1, completed: false });
    // Again: already solved, nothing counted twice.
    expect(afterSubmit(p1, list, "a", { correct: true, alreadySolved: true, labCompleted: false })).toMatchObject({ solved: ["a"], requiredSolved: 1 });
    // An optional challenge and a step are solved but don't count toward the required total.
    const p2 = afterSubmit(afterSubmit(p1, list, "b", { correct: true, alreadySolved: false, labCompleted: false }), list, "c1", {
      correct: true,
      alreadySolved: false,
      labCompleted: false,
    });
    expect(p2).toMatchObject({ solved: ["a", "b", "c1"], requiredSolved: 1 });
    const p3 = afterSubmit(p2, list, "c", { correct: true, alreadySolved: false, labCompleted: true });
    expect(p3).toMatchObject({ requiredSolved: 2, requiredTotal: 2, completed: true });
  });

  it("keeps opened hints once and in order, and knows when help was used", () => {
    const c = challenge("a", { hintCount: 2, steps: [{ key: "a1", title: "a1", prompt: null, hintCount: 1 }] });
    let p = emptyProgress([c]);
    expect(usedHelp(p, c)).toBe(false);
    p = withHint(p, { key: "a", index: 1, text: "second" });
    p = withHint(p, { key: "a", index: 0, text: "first" });
    p = withHint(p, { key: "a", index: 0, text: "first" });
    expect(hintsOf(p, "a").map((h) => h.text)).toEqual(["first", "second"]);
    expect(usedHelp(p, c)).toBe(true);
    // A step's hint counts as help for its challenge; so do opened steps.
    expect(usedHelp(withHint(emptyProgress([c]), { key: "a1", index: 0, text: "x" }), c)).toBe(true);
    const s = withSteps(emptyProgress([c]), "a");
    expect(withSteps(s, "a")).toBe(s);
    expect(usedHelp(s, c)).toBe(true);
  });

  it("opens all of a short list, else the first unsolved challenge", () => {
    expect(initiallyOpen([challenge("a"), challenge("b")], [])).toEqual(["a", "b"]);
    const many = ["a", "b", "c", "d"].map((k) => challenge(k));
    expect(initiallyOpen(many, ["a", "b"])).toEqual(["c"]);
    expect(initiallyOpen(many, ["a", "b", "c", "d"])).toEqual([]);
  });
});

describe("objectives API", () => {
  it("reads a lab's objectives and progress, defaulting missing fields", async () => {
    installTauri({
      api_query: (a) => {
        const q = String(a?.operation);
        if (q === "labProgress") return { labs: [{ id: "lab-1", myProgress: { solved: ["a"], requiredSolved: 1, requiredTotal: 2 } }] };
        return { labs: [{ id: "lab-1", objectives: [{ key: "a", title: "A", hintCount: 1, steps: [{ key: "a1", title: "A1" }] }] }] };
      },
    });
    expect(await loadObjectives("lab-1")).toEqual([
      { key: "a", title: "A", prompt: null, hintCount: 1, optional: false, points: null, steps: [{ key: "a1", title: "A1", prompt: null, hintCount: 0 }] },
    ]);
    expect(await loadProgress("lab-1")).toEqual({ solved: ["a"], stepsOpened: [], hints: [], requiredSolved: 1, requiredTotal: 2, completed: false });
    // Another lab's answer (or none) is not this lab's.
    expect(await loadObjectives("lab-2")).toEqual([]);
    expect(await loadProgress("lab-2")).toBeNull();
  });

  it("signed out: no progress", async () => {
    installTauri({ api_query: () => ({ labs: [{ id: "lab-1", myProgress: null }] }) });
    expect(await loadProgress("lab-1")).toBeNull();
  });

  it("sends the two mutations with the lab, key and help kind", async () => {
    const calls = installTauri({
      api_query: (a) => {
        const q = String(a?.operation);
        if (q === "submitObjective") return { submitObjective: { correct: true, alreadySolved: false, labCompleted: false } };
        if (q === "openObjectiveHelp")
          return { openObjectiveHelp: { ok: true, hint: (a?.variables as { what: string }).what === "HINT" ? "Look closer" : null } };
        return {};
      },
    });
    expect(await submitObjective("lab-1", "a", " FLAG{x} ")).toEqual({ correct: true, alreadySolved: false, labCompleted: false });
    await openSteps("lab-1", "a");
    expect(await openHint("lab-1", "a", 0)).toBe("Look closer");
    const sent = calls.filter((c) => c.cmd === "api_query");
    expect(sent.map((c) => c.args?.operation)).toEqual(["submitObjective", "openObjectiveHelp", "openObjectiveHelp"]);
    const vars = sent.map((c) => c.args?.variables);
    expect(vars).toEqual([
      { labId: "lab-1", key: "a", answer: " FLAG{x} " },
      { labId: "lab-1", key: "a", what: "STEPS", index: null },
      { labId: "lab-1", key: "a", what: "HINT", index: 0 },
    ]);
  });

  it("passes a backend refusal on", async () => {
    installTauri({
      api_query: () => rejectWith("Requires login"),
    });
    await expect(submitObjective("lab-1", "a", "x")).rejects.toBe("Requires login");
  });
});
