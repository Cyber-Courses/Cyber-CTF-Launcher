import { describe, expect, it } from "vitest";

import { type Baseline, raisedTotals, ratchet, readBaseline } from "./baseline.ts";

const baseline = (clippy: Baseline["clippy"]): Baseline => ({ rules: { "dup/clone": { "rs:runtime": 9 } }, clippy, exceptions: {} });

describe("raisedTotals", () => {
  it("records a platform's first clippy counts instead of refusing them", () => {
    const before = baseline({ darwin: { "clippy/doc_markdown": { "rs:runtime": 48 } } });
    const now = baseline({ ...before.clippy, win32: { "clippy/doc_markdown": { "rs:runtime": 48 }, "clippy/large_futures": { "rs:runtime": 12 } } });
    expect(raisedTotals(now, before, "win32")).toEqual([]);
  });

  it("refuses a platform's clippy count that rose above its baseline", () => {
    const before = baseline({ win32: { "clippy/large_futures": { "rs:runtime": 12 } } });
    const now = baseline({ win32: { "clippy/large_futures": { "rs:runtime": 12, "rs:machine": 1 } } });
    expect(raisedTotals(now, before, "win32")).toEqual(["clippy clippy/large_futures: 13 (baseline 12)"]);
  });

  it("refuses a rule whose total rose, whatever the platform", () => {
    const before = baseline({});
    const now: Baseline = { ...before, rules: { "dup/clone": { "rs:runtime": 9, "ts:lib": 1 } } };
    expect(raisedTotals(now, before, "linux")).toEqual(["rule dup/clone: 10 (baseline 9)"]);
  });
});

describe("ratchet", () => {
  const before = baseline({ darwin: { "clippy/panic": { "rs:app": 2 } } });
  const rose: Baseline = { ...before, rules: { "dup/clone": { "rs:runtime": 9, "ts:lib": 1 } } };
  const fell: Baseline = { ...before, rules: { "dup/clone": { "rs:runtime": 8 } } };
  const check = { update: false, force: false };

  it("fails a check on a count over the baseline and says one under it, writing nothing", () => {
    expect(ratchet(rose, before, "darwin", check)).toEqual({
      write: false,
      failures: ["over the baseline: rule dup/clone in ts:lib: 1 (baseline 0)"],
      notes: [],
    });
    expect(ratchet(fell, before, "darwin", check)).toEqual({
      write: false,
      failures: [],
      notes: ["under the baseline (lock it in with --update): rule dup/clone in rs:runtime: 8 (baseline 9)"],
    });
  });

  it("says a platform has no clippy baseline yet", () => {
    expect(ratchet(before, before, "linux", check).notes).toEqual(["no clippy baseline for linux yet: --update records one"]);
  });

  it("lowers the baseline on --update, refuses a rule's total that rose, and takes it with --force", () => {
    expect(ratchet(fell, before, "darwin", { update: true, force: false })).toEqual({ write: true, failures: [], notes: [] });
    expect(ratchet(rose, before, "darwin", { update: true, force: false })).toEqual({
      write: false,
      failures: ["raised: rule dup/clone: 10 (baseline 9)"],
      notes: [],
    });
    expect(ratchet(rose, before, "darwin", { update: true, force: true }).write).toBe(true);
  });
});

describe("readBaseline", () => {
  it("starts from nothing when there is no baseline file", () => {
    expect(readBaseline("/nonexistent/quality-baseline.json")).toEqual({ rules: {}, clippy: {}, exceptions: {} });
  });
});
