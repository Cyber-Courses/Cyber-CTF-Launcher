// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
// elk-worker's in-process stand-in for a worker (what it exports outside a worker).
import * as elkWorker from "elkjs/lib/elk-worker.js";
import { READY } from "@/features/labs/network-diagram/elk-ready";
import { LAYOUT_MS, MAIN_THREAD_KEY, READY_MS } from "@/features/labs/network-diagram/elk-engine";

const ElkFakeWorker = (elkWorker as unknown as { Worker: new () => { postMessage: (m: unknown) => void } }).Worker;

// A two-node graph: B is laid out below A once ELK ran, wherever it ran.
const graph = () => ({
  id: "root",
  layoutOptions: { "elk.algorithm": "layered", "elk.direction": "DOWN" },
  children: [
    { id: "a", width: 10, height: 10 },
    { id: "b", width: 10, height: 10 },
  ],
  edges: [{ id: "e", sources: ["a"], targets: ["b"] }],
});

/** Workers created, and messages posted to them. */
const created: string[] = [];
let posted = 0;

type Listener = (e: { data?: unknown; message?: string }) => void;

/** A browser worker running ELK in process: says it is ready after `readyAfter` ms (never when
 *  null), then answers (unless `answers` is off). */
function worker({ readyAfter = 0 as number | null, answers = true, fails = false } = {}) {
  return class extends ElkFakeWorker {
    private listeners: Record<string, Listener[]> = {};
    terminated = false;
    constructor(url: URL) {
      super();
      created.push(String(url));
      // The stand-in sets postMessage on the instance: wrap that one.
      const answer = this.postMessage;
      this.postMessage = (m: unknown) => {
        posted++;
        if (answers && !this.terminated) answer(m);
      };
      if (fails) setTimeout(() => this.emit("error", { message: "" }), 0);
      else if (readyAfter !== null) setTimeout(() => this.emit("message", { data: READY }), readyAfter);
    }
    private emit(type: string, e: { data?: unknown; message?: string }) {
      (this.listeners[type] ?? []).forEach((f) => f(e));
    }
    addEventListener(type: string, f: Listener) {
      (this.listeners[type] ??= []).push(f);
    }
    terminate() {
      this.terminated = true;
    }
  };
}

/** A worker the webview refuses to create. */
class RefusedWorker {
  constructor() {
    throw new Error("SecurityError");
  }
}

/** A fresh engine module (a new page in the same session), with `Worker` set as given. */
async function engineWith(w?: unknown) {
  vi.resetModules();
  if (w) vi.stubGlobal("Worker", w);
  const { elkEngine } = await import("@/features/labs/network-diagram/elk-engine");
  return elkEngine;
}

type Laid = { children?: { y?: number }[] };
async function laysOut(elkEngine: () => Promise<{ layout: (g: ReturnType<typeof graph>) => Promise<Laid> }>) {
  const out = await (await elkEngine()).layout(graph());
  expect(out.children![1].y).toBeGreaterThan(out.children![0].y!);
}

/** Under fake timers: runs them until `p` settles (ELK's stand-in worker answers on a timer). */
async function drive<T>(p: Promise<T>, upTo = 10_000): Promise<T> {
  let done = false;
  p.then(
    () => (done = true),
    () => (done = true),
  );
  for (let t = 0; !done && t < upTo; t += 10) await vi.advanceTimersByTimeAsync(10);
  return p;
}

let warn: MockInstance<typeof console.warn>;
const warned = () => warn.mock.calls.map((c) => String(c[0]));

beforeEach(() => {
  sessionStorage.clear();
  warn = vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  created.length = 0;
  posted = 0;
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("elkEngine", () => {
  it("runs ELK on the main thread where there are no workers, chosen once", async () => {
    const elkEngine = await engineWith();
    await laysOut(elkEngine);
    expect(elkEngine()).toBe(elkEngine());
    expect(warned()).toEqual([expect.stringContaining("main thread (no Web Worker here)")]);
  });

  it("runs ELK in the worker once it says it is ready in time", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ readyAfter: READY_MS - 200 }));
    await drive(elkEngine());
    await drive(laysOut(elkEngine));
    await drive(laysOut(elkEngine));
    expect(created).toHaveLength(1);
    expect(created[0]).toMatch(/elk\.worker\.ts/);
    expect(warned()).toEqual(["network diagram: ELK runs in a web worker"]);
    expect(sessionStorage.getItem(MAIN_THREAD_KEY)).toBeNull();
  });

  it("falls back to the main thread when ready comes too late, and remembers it for the session", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const Late = worker({ readyAfter: READY_MS + 500 });
    const terminate = vi.spyOn(Late.prototype, "terminate");
    let elkEngine = await engineWith(Late);
    await drive(elkEngine());
    vi.useRealTimers();
    await laysOut(elkEngine);
    expect(terminate).toHaveBeenCalled();
    expect(warned()).toEqual([expect.stringContaining("main thread (the layout worker did not start)")]);
    expect(sessionStorage.getItem(MAIN_THREAD_KEY)).toBe("1");

    // A later page in the session doesn't try the worker again, so doesn't wait for it.
    elkEngine = await engineWith(worker());
    await laysOut(elkEngine);
    expect(created).toHaveLength(1);
    expect(warned()[1]).toContain("didn't start earlier in this session");
  });

  it("falls back when the worker fails to load, or the webview refuses it", async () => {
    await laysOut(await engineWith(worker({ fails: true })));
    expect(warned()[0]).toContain("main thread (the layout worker failed to load)");
    sessionStorage.clear();
    await laysOut(await engineWith(RefusedWorker));
    expect(warned()[1]).toContain("main thread (SecurityError)");
  });

  it("gives up a started worker that stops answering, for this layout and the next", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ answers: false }));
    const engine = await drive(elkEngine());
    const out = (await drive(engine.layout(graph()), LAYOUT_MS + 1000)) as Laid;
    vi.useRealTimers();
    expect(out.children![1].y).toBeGreaterThan(out.children![0].y!);
    const sent = posted;
    await laysOut(async () => engine);
    await laysOut(elkEngine);
    expect(posted).toBe(sent);
    expect(warned()).toEqual(["network diagram: ELK runs in a web worker", expect.stringContaining("main thread (the layout worker stopped answering)")]);
    expect(sessionStorage.getItem(MAIN_THREAD_KEY)).toBe("1");
  });

  it("passes on a layout error from the worker", async () => {
    const elkEngine = await engineWith(worker());
    await expect((await elkEngine()).layout({ id: "root", edges: [{ id: "e", sources: ["nowhere"], targets: ["b"] }] })).rejects.toBeTruthy();
  });
});

describe("the layout worker", () => {
  it("says it is ready once ELK has loaded", async () => {
    const post = vi.spyOn(self, "postMessage").mockImplementation(() => {});
    vi.resetModules();
    await import("@/features/labs/network-diagram/elk.worker");
    expect(post).toHaveBeenCalledWith(READY);
  });
});
