// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";
import { READY } from "@/features/labs/network-diagram/elk-ready";
import { LAYOUT_MS, MAIN_THREAD_KEY, READY_MS } from "@/features/labs/network-diagram/elk-engine";

// The worker and fallback tests run no ELK: the worker is a stub speaking elk-api's message
// protocol, and the main-thread ELK is a stand-in. Both answer with a canned layout that says
// where it was laid out. Time is fake and moved by hand, so the ready / layout timeouts race
// nothing real. One test runs the real ELK, on the main thread (the no-worker path).

type Graph = { id: string; children?: { id: string }[] };
type Laid = { by?: string; children?: { y?: number }[] };

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

/** A canned layout of `g`, one child per row, marked with what laid it out. */
const canned = (g: Graph, by: string) => ({ ...g, by, children: g.children?.map((c, i) => ({ ...c, x: 0, y: i * 20 })) });

/** Workers created, and messages posted to them. */
const created: string[] = [];
let posted = 0;

type Listener = (e: { data?: unknown; message?: string }) => void;
type Message = { id: number; cmd: string; graph?: Graph };

/** A browser worker speaking elk-api's protocol: says it is ready after `readyAfter` ms (never
 *  when null), then answers each message on the next tick (unless `answers` is off), a layout
 *  with a canned one (or an error with `layoutFails`). */
function worker({ readyAfter = 0 as number | null, answers = true, fails = false, layoutFails = false } = {}) {
  return class {
    private listeners: Record<string, Listener[]> = {};
    onmessage: Listener | null = null;
    terminated = false;
    constructor(url: URL) {
      created.push(String(url));
      if (fails) setTimeout(() => this.emit("error", { message: "" }), 0);
      else if (readyAfter !== null) setTimeout(() => this.emit("message", { data: READY }), readyAfter);
    }
    postMessage(m: Message) {
      posted++;
      if (!answers || this.terminated) return;
      const answer =
        m.cmd !== "layout"
          ? { id: m.id, data: null }
          : layoutFails
            ? { id: m.id, error: { message: "no such node" } }
            : { id: m.id, data: canned(m.graph!, "worker") };
      setTimeout(() => this.onmessage?.({ data: answer }), 0);
    }
    private emit(type: string, e: { data?: unknown; message?: string }) {
      (this.listeners[type] ?? []).forEach((f) => f(e));
      if (type === "message") this.onmessage?.(e);
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

/** A fresh engine module (a new page in the same session), with `Worker` set as given. The
 *  main-thread ELK is a stand-in unless `realElk`. */
async function engineWith(w?: unknown, { realElk = false } = {}) {
  vi.resetModules();
  if (realElk) vi.doUnmock("elkjs/lib/elk.bundled.js");
  else
    vi.doMock("elkjs/lib/elk.bundled.js", () => ({
      default: class {
        layout = (g: Graph) => Promise.resolve(canned(g, "main thread"));
      },
    }));
  if (w) vi.stubGlobal("Worker", w);
  const { elkEngine } = await import("@/features/labs/network-diagram/elk-engine");
  return elkEngine;
}

type Engine = { layout: (g: ReturnType<typeof graph>) => Promise<Laid> };

/** Lays out `graph()` with the engine and checks B is below A and who laid it out. With fake
 *  timers, `tick` moves them past the stub's message hops (a few 0 ms timers). */
async function laysOut(elkEngine: () => Promise<Engine>, by: string | undefined, { tick = false } = {}) {
  const out = (await elkEngine()).layout(graph());
  if (tick) await vi.advanceTimersByTimeAsync(10);
  const laid = await out;
  expect(laid.children![1].y).toBeGreaterThan(laid.children![0].y!);
  expect(laid.by).toBe(by);
}

/** Waits (in real time: loading elk-api is a dynamic import) until the engine created its
 *  worker, so its ready timeout is running on the fake clock. */
async function workerCreated() {
  while (!created.length) await new Promise((r) => setImmediate(r));
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
  vi.doUnmock("elkjs/lib/elk.bundled.js");
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("elkEngine", () => {
  it("runs the real ELK on the main thread where there are no workers, chosen once", async () => {
    const elkEngine = await engineWith(undefined, { realElk: true });
    await laysOut(elkEngine, undefined);
    expect(elkEngine()).toBe(elkEngine());
    expect(warned()).toEqual([expect.stringContaining("main thread (no Web Worker here)")]);
  });

  it("runs ELK in the worker once it says it is ready in time", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ readyAfter: READY_MS - 200 }));
    const engine = elkEngine();
    await workerCreated();
    await vi.advanceTimersByTimeAsync(READY_MS - 200);
    await engine;
    await laysOut(elkEngine, "worker", { tick: true });
    await laysOut(elkEngine, "worker", { tick: true });
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
    const engine = elkEngine();
    await workerCreated();
    await vi.advanceTimersByTimeAsync(READY_MS);
    await engine;
    await laysOut(elkEngine, "main thread");
    expect(terminate).toHaveBeenCalled();
    expect(warned()).toEqual([expect.stringContaining("main thread (the layout worker did not start)")]);
    expect(sessionStorage.getItem(MAIN_THREAD_KEY)).toBe("1");

    // A later page in the session doesn't try the worker again, so doesn't wait for it.
    elkEngine = await engineWith(worker());
    await laysOut(elkEngine, "main thread");
    expect(created).toHaveLength(1);
    expect(warned()[1]).toContain("didn't start earlier in this session");
  });

  it("falls back when the worker fails to load, or the webview refuses it", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ fails: true }));
    const engine = elkEngine();
    await workerCreated();
    await vi.advanceTimersByTimeAsync(0);
    await engine;
    await laysOut(elkEngine, "main thread");
    expect(warned()[0]).toContain("main thread (the layout worker failed to load)");
    sessionStorage.clear();
    await laysOut(await engineWith(RefusedWorker), "main thread");
    expect(warned()[1]).toContain("main thread (SecurityError)");
  });

  it("gives up a started worker that stops answering, for this layout and the next", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ answers: false }));
    const started = elkEngine();
    await workerCreated();
    await vi.advanceTimersByTimeAsync(0);
    const engine = await started;
    const out = engine.layout(graph());
    await vi.advanceTimersByTimeAsync(LAYOUT_MS - 1);
    expect(warned()).toEqual(["network diagram: ELK runs in a web worker"]);
    await vi.advanceTimersByTimeAsync(1);
    expect(((await out) as Laid).by).toBe("main thread");
    const sent = posted;
    await laysOut(async () => engine, "main thread");
    await laysOut(elkEngine, "main thread");
    expect(posted).toBe(sent);
    expect(warned()).toEqual(["network diagram: ELK runs in a web worker", expect.stringContaining("main thread (the layout worker stopped answering)")]);
    expect(sessionStorage.getItem(MAIN_THREAD_KEY)).toBe("1");
  });

  it("passes on a layout error from the worker", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const elkEngine = await engineWith(worker({ layoutFails: true }));
    const started = elkEngine();
    await workerCreated();
    await vi.advanceTimersByTimeAsync(0);
    const out = (await started).layout(graph());
    const failed = expect(out).rejects.toMatchObject({ message: "no such node" });
    await vi.advanceTimersByTimeAsync(10);
    await failed;
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
