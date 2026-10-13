import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

// labStatus answers from a queue of deferred replies, so a test decides which read lands first.
const replies: { resolve: (s: LabStatus) => void }[] = [];
vi.mock("@/lib/tauri", () => ({
  labStatus: () => new Promise<LabStatus>((resolve) => replies.push({ resolve })),
}));

const lab = { id: "lab-1", runtime: { runtime: "DOCKER", architectures: [], providers: [] } } as unknown as Lab;
const status = (running: boolean) =>
  ({
    running,
    parked: running ? null : "shutdown",
    machines: [],
    networks: [],
    url: null,
    host: null,
    expiresAt: null,
    place: null,
    provider: null,
  }) as unknown as LabStatus;

async function freshStore() {
  vi.resetModules();
  replies.length = 0;
  return import("@/features/labs/lab-store");
}

describe("lab-store", () => {
  beforeEach(() => {
    replies.length = 0;
  });

  it("a poll joins the read in flight instead of stacking another", async () => {
    const { refreshStatus } = await freshStore();
    void refreshStatus(lab);
    void refreshStatus(lab);
    expect(replies).toHaveLength(1);
  });

  it("a poll begun before an operation ended can't overwrite the fresh read after it", async () => {
    // Regression: after a shutdown, an older poll landed with "running" and brought back the
    // Shut down button; a click there shut the lab down again.
    const { refreshStatus, getLabState } = await freshStore();
    const poll = refreshStatus(lab);
    const fresh = refreshStatus(lab, { fresh: true });
    expect(replies).toHaveLength(2);
    replies[1].resolve(status(false));
    await fresh;
    replies[0].resolve(status(true));
    await poll;
    expect(getLabState().statuses["lab-1"].running).toBe(false);
    expect(getLabState().probed.has("lab-1")).toBe(true);
  });

  it("a fresh read finding the lab down drops it from an older scan, and older scans don't count", async () => {
    const { refreshStatus, setScanRunning, getLabState } = await freshStore();
    const before = Date.now() - 1000;
    setScanRunning(["lab-1", "lab-2"], before);
    const fresh = refreshStatus(lab, { fresh: true });
    replies[0].resolve(status(false));
    await fresh;
    expect([...getLabState().scanRunning]).toEqual(["lab-2"]);
    // A scan that began before the fresh read reports lab-1 running: old news, left out.
    expect(setScanRunning(["lab-1", "lab-2"], before)).toEqual(["lab-2"]);
    // One begun after it counts.
    expect(setScanRunning(["lab-1"], Date.now() + 1)).toEqual(["lab-1"]);
  });

  it("keeps the same statuses and scan objects when a read says nothing new", async () => {
    const { refreshStatus, setScanRunning, getLabState } = await freshStore();
    const first = refreshStatus(lab);
    replies[0].resolve(status(true));
    await first;
    const statuses = getLabState().statuses;
    const again = refreshStatus(lab);
    replies[1].resolve(status(true));
    await again;
    expect(getLabState().statuses).toBe(statuses);
    setScanRunning(["lab-1"], Date.now());
    const scan = getLabState().scanRunning;
    setScanRunning(["lab-1"], Date.now());
    expect(getLabState().scanRunning).toBe(scan);
    setScanRunning([], Date.now());
    expect(getLabState().scanRunning.size).toBe(0);
  });
});
