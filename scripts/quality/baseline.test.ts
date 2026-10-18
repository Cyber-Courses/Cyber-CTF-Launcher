import { describe, expect, it } from "vitest";

import { type Baseline, raisedTotals } from "./baseline.ts";

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
