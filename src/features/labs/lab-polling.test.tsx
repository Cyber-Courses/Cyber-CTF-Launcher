// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

// The machine as the mocked backend sees it: what the scan finds running here, and the labs
// running elsewhere (a server, the cloud), which only their own status reports.
const world = { scan: [] as string[], remote: new Set<string>(), ops: [] as { labId: string; op: string; machine: null; step: null }[] };
const statusReads: string[] = [];
const status = (running: boolean) => ({ running, parked: null, machines: [], networks: [], url: null }) as unknown as LabStatus;
vi.mock("@/lib/tauri", () => ({
  labStatus: async (id: string) => {
    statusReads.push(id);
    return status(world.scan.includes(id) || world.remote.has(id));
  },
  runningLabs: async () => [...world.scan],
  activeOperations: async () => world.ops,
  deployingLabs: async () => [],
}));

const lab = (id: string) => ({ id, slug: id, title: id, runtime: { runtime: "DOCKER", architectures: [], providers: [] } }) as unknown as Lab;
const labs = ["a", "b", "c", "d"].map(lab);

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "visibilityState", { value: hidden ? "hidden" : "visible", configurable: true });
  document.dispatchEvent(new Event("visibilitychange"));
}

async function fresh() {
  vi.resetModules();
  const polling = await import("@/features/labs/lab-polling");
  const store = await import("@/features/labs/lab-store");
  const deploy = await import("@/lib/deploy-store");
  return { ...polling, ...store, ...deploy };
}

/** Lets the reads in flight answer, `ms` of fake time later. */
const advance = (ms = 0) => act(() => vi.advanceTimersByTimeAsync(ms));
/** The labs read since the last call, sorted. */
const taken = () => statusReads.splice(0).sort();

beforeEach(() => {
  vi.useFakeTimers();
  world.scan = [];
  world.remote = new Set();
  world.ops = [];
  statusReads.length = 0;
});
afterEach(() => {
  setHidden(false);
  vi.useRealTimers();
});

describe("hotLabs", () => {
  it("picks the labs that can change on their own, and the open one", async () => {
    const { hotLabs } = await fresh();
    const picked = hotLabs([...labs, lab("e"), lab("f")], {
      scanRunning: new Set(["a"]),
      statuses: { b: status(true), c: status(false) },
      busy: new Set(["d"]),
      watched: new Set(["e"]),
    });
    expect(picked.map((l) => l.id)).toEqual(["a", "b", "d", "e"]);
  });
});

describe("useLabPolling", () => {
  it("reads only the running, busy and open labs on each poll", async () => {
    const { useLabPolling, useWatchLab, LAB_POLL_MS, getLabState, beginDeploy, endDeploy } = await fresh();
    world.scan = ["a"];
    world.remote = new Set(["b"]);
    const shell = renderHook(() => useLabPolling(labs));
    await advance();
    // The first scan finds `a`: read it. The rest were read on the catalogue's load (not here).
    expect(taken()).toEqual(["a"]);
    expect(getLabState().scanRunning.has("a")).toBe(true);

    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a"]);

    // `b` known running (on a server, out of the scan's sight): read on each poll too.
    const { refreshStatus } = await import("@/features/labs/lab-store");
    await act(() => refreshStatus(labs[1]));
    taken();
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b"]);

    // A lab page open on `c`: read while it is, then once more after it closes.
    const page = renderHook(() => useWatchLab("c"));
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b", "c"]);
    page.unmount();
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b", "c"]);
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b"]);

    // An operation on `d` (here, or one the backend reports): read while it runs.
    beginDeploy("d", "launch");
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b", "d"]);
    endDeploy("d");
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b", "d"]);
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["a", "b"]);
    shell.unmount();
  });

  it("reads a lab as soon as the scan sees it start or stop", async () => {
    const { useLabPolling, LAB_POLL_MS, getLabState } = await fresh();
    const shell = renderHook(() => useLabPolling(labs));
    await advance();
    expect(taken()).toEqual([]);
    // Started from the website: the scan sees it, and it is read right after the scan.
    world.scan = ["c"];
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["c"]);
    expect(getLabState().statuses.c.running).toBe(true);
    // Stopped elsewhere: read while the scan still saw it, once more after (its final word),
    // then left alone.
    world.scan = [];
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["c"]);
    expect(getLabState().statuses.c.running).toBe(false);
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["c"]);
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual([]);
    shell.unmount();
  });

  it("is one poll for every screen, paused while the window is hidden", async () => {
    const { useLabPolling, LAB_POLL_MS } = await fresh();
    world.scan = ["a"];
    const home = renderHook(() => useLabPolling(labs));
    const sidebar = renderHook(() => useLabPolling(labs));
    await advance();
    taken();
    await advance(LAB_POLL_MS);
    // Two screens, one read.
    expect(taken()).toEqual(["a"]);
    act(() => setHidden(true));
    await advance(LAB_POLL_MS * 5);
    expect(taken()).toEqual([]);
    // Back: every lab read at once (they may have changed meanwhile).
    act(() => setHidden(false));
    await advance();
    expect(taken()).toEqual(["a", "b", "c", "d"]);
    home.unmount();
    sidebar.unmount();
    await advance(LAB_POLL_MS * 3);
    expect(taken()).toEqual([]);
  });

  it("reads every lab on focus, at most once per poll interval", async () => {
    const { useLabPolling, LAB_POLL_MS } = await fresh();
    const shell = renderHook(() => useLabPolling(labs));
    await advance();
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await advance();
    expect(taken()).toEqual(["a", "b", "c", "d"]);
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await advance();
    expect(taken()).toEqual([]);
    await advance(LAB_POLL_MS);
    taken();
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await advance();
    expect(taken()).toEqual(["a", "b", "c", "d"]);
    shell.unmount();
  });

  it("counts a lab page open twice until both close", async () => {
    const { useLabPolling, useWatchLab, LAB_POLL_MS } = await fresh();
    const shell = renderHook(() => useLabPolling(labs));
    const one = renderHook(() => useWatchLab("c"));
    const two = renderHook(() => useWatchLab("c"));
    await advance(LAB_POLL_MS);
    taken();
    one.unmount();
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["c"]);
    two.unmount();
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual(["c"]);
    await advance(LAB_POLL_MS);
    expect(taken()).toEqual([]);
    shell.unmount();
  });

  it("starts without a catalogue, and keeps scanning", async () => {
    const { useLabPolling, getLabState } = await fresh();
    world.scan = ["a"];
    const shell = renderHook(() => useLabPolling(null));
    await advance();
    expect(taken()).toEqual([]);
    expect(getLabState().scanRunning.has("a")).toBe(true);
    shell.unmount();
  });
});
