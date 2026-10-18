import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

// labStatus answers from a queue of deferred replies, so a test decides which read lands first
// and whether it fails; `asked` counts the reads.
const replies: { resolve: (s: LabStatus) => void; reject: (e: unknown) => void }[] = [];
const asked = { n: 0 };
vi.mock("@/lib/tauri", () => ({
  labStatus: () => {
    asked.n++;
    return new Promise<LabStatus>((resolve, reject) => replies.push({ resolve, reject }));
  },
}));

const lab = { id: "lab-1", runtime: { runtime: "DOCKER", architectures: [], providers: [] } } as unknown as Lab;
const status = (running: boolean) => ({ running, parked: null, machines: [], networks: [] }) as unknown as LabStatus;

async function freshStore() {
  vi.resetModules();
  replies.length = 0;
  asked.n = 0;
  return import("@/features/labs/lab-store");
}

describe("lab-store: edges", () => {
  beforeEach(() => {
    replies.length = 0;
    asked.n = 0;
  });
  afterEach(() => vi.useRealTimers());

  it("a lab without a runtime is neither asked nor counted as probed", async () => {
    const { refreshStatus, getLabState } = await freshStore();
    await refreshStatus({ id: "bare", runtime: null } as unknown as Lab);
    expect(asked.n).toBe(0);
    expect(getLabState().probed.has("bare")).toBe(false);
  });

  it("a failed read leaves the status unknown but marks the lab probed", async () => {
    const { refreshStatus, getLabState } = await freshStore();
    const read = refreshStatus(lab);
    replies[0].reject(new Error("docker is not running"));
    await read;
    expect(getLabState().statuses["lab-1"]).toBeUndefined();
    expect(getLabState().probed.has("lab-1")).toBe(true);
  });

  it("noting a stop on a lab the scan never saw changes nothing", async () => {
    const { noteStopped, getLabState } = await freshStore();
    const before = getLabState();
    noteStopped("lab-1");
    expect(getLabState()).toBe(before);
  });

  it("a scan after this app stopped a lab reports it to be read, but keeps it out of the running set until a later read sees it running", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    const { noteStopped, setScanRunning, refreshStatus, getLabState } = await freshStore();
    noteStopped("lab-1");
    vi.setSystemTime(2000);
    expect(setScanRunning(["lab-1"], 2000)).toEqual(["lab-1"]);
    expect(getLabState().scanRunning.has("lab-1")).toBe(false);
    // Started again from the website: a read begun after the stop sees it running.
    vi.setSystemTime(3000);
    const read = refreshStatus(lab);
    replies[0].resolve(status(true));
    await read;
    setScanRunning(["lab-1"], 4000);
    expect(getLabState().scanRunning.has("lab-1")).toBe(true);
  });
});
