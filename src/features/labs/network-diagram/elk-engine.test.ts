import { afterEach, describe, expect, it, vi } from "vitest";
// elk-worker's in-process stand-in for a worker (what it exports outside a worker).
import * as elkWorker from "elkjs/lib/elk-worker.js";
const ElkFakeWorker = (elkWorker as unknown as { Worker: new () => object }).Worker;

// A two-node graph: B is laid out below A once ELK ran, wherever it ran.
const graph = {
  id: "root",
  layoutOptions: { "elk.algorithm": "layered", "elk.direction": "DOWN" },
  children: [
    { id: "a", width: 10, height: 10 },
    { id: "b", width: 10, height: 10 },
  ],
  edges: [{ id: "e", sources: ["a"], targets: ["b"] }],
};

const created: string[] = [];

/** A browser worker running ELK: the stand-in, plus the listener and terminate a Worker has. */
class GoodWorker extends ElkFakeWorker {
  constructor(url: URL) {
    super();
    created.push(String(url));
  }
  addEventListener() {}
  terminate() {}
}

/** A worker whose script fails to load: an error event, no answer. */
class BrokenWorker {
  private onError?: (e: { message: string }) => void;
  terminated = false;
  constructor(url: URL) {
    created.push(String(url));
  }
  addEventListener(_: string, f: (e: { message: string }) => void) {
    this.onError = f;
  }
  postMessage() {
    setTimeout(() => this.onError?.({ message: "" }), 0);
  }
  terminate() {
    this.terminated = true;
  }
}

/** A worker that never answers. */
class SilentWorker extends BrokenWorker {
  postMessage() {}
}

/** A worker the webview refuses to create. */
class RefusedWorker {
  constructor() {
    throw new Error("SecurityError");
  }
}

async function engineWith(worker?: unknown) {
  vi.resetModules();
  if (worker) vi.stubGlobal("Worker", worker);
  const { elkEngine } = await import("@/features/labs/network-diagram/elk-engine");
  return elkEngine;
}

async function laysOut(elkEngine: () => Promise<{ layout: (g: typeof graph) => Promise<{ children?: { y?: number }[] }> }>) {
  const out = await (await elkEngine()).layout(structuredClone(graph));
  expect(out.children![1].y).toBeGreaterThan(out.children![0].y!);
}

afterEach(() => {
  created.length = 0;
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("elkEngine", () => {
  it("runs ELK inline where there are no workers, and creates it once", async () => {
    const elkEngine = await engineWith();
    await laysOut(elkEngine);
    expect(elkEngine()).toBe(elkEngine());
  });

  it("runs ELK in the layout worker when the browser has workers", async () => {
    const elkEngine = await engineWith(GoodWorker);
    await laysOut(elkEngine);
    expect(created).toHaveLength(1);
    expect(created[0]).toMatch(/elk\.worker\.ts$/);
  });

  it("falls back to the main thread when the worker fails to load", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const terminate = vi.spyOn(BrokenWorker.prototype, "terminate");
    const elkEngine = await engineWith(BrokenWorker);
    await laysOut(elkEngine);
    expect(created).toHaveLength(1);
    expect(terminate).toHaveBeenCalled();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("layout worker unavailable"), "the layout worker failed to load");
  });

  it("falls back when the webview refuses the worker", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const elkEngine = await engineWith(RefusedWorker);
    await laysOut(elkEngine);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("layout worker unavailable"), expect.any(Error));
  });

  it("falls back when the worker never answers", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(SilentWorker);
    const engine = elkEngine();
    await vi.advanceTimersByTimeAsync(15_000);
    await engine;
    vi.useRealTimers();
    await laysOut(elkEngine);
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("layout worker unavailable"), "the layout worker did not answer");
  });
});
