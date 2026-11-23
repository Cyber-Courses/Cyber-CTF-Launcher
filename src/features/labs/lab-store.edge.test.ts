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
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("a lab without a runtime is neither asked nor counted as probed", async () => {
    const { refreshStatus, getLabState } = await freshStore();
    await refreshStatus({ id: "bare", runtime: null } as unknown as Lab);
    expect(asked.n).toBe(0);
    expect(getLabState().probed.has("bare")).toBe(false);
  });

  it("a failed read leaves the status unknown but marks the lab probed", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { refreshStatus, getLabState } = await freshStore();
    const read = refreshStatus(lab);
    replies[0].reject(new Error("docker is not running"));
    await read;
    expect(getLabState().statuses["lab-1"]).toBeUndefined();
    expect(getLabState().probed.has("lab-1")).toBe(true);
  });

  it("keeps why a status read failed, logging it once, until a read succeeds", async () => {
    // Regression: every failed read was discarded, so Docker down or a lab whose files couldn't be
    // generated again showed its last status with no word of it.
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { refreshStatus, getLabState } = await freshStore();
    const denied = { code: "other", message: "Docker isn't running" };
    for (const i of [0, 1]) {
      const read = refreshStatus(lab);
      replies[i].reject({ ...denied });
      await read;
    }
    expect(getLabState().statusErrors["lab-1"]).toEqual(denied);
    expect(getLabState().probed.has("lab-1")).toBe(true);
    expect(warn).toHaveBeenCalledTimes(1);
    const ok = refreshStatus(lab);
    replies[2].resolve({ running: false } as unknown as LabStatus);
    await ok;
    expect("lab-1" in getLabState().statusErrors).toBe(false);
    warn.mockRestore();
  });

  it("a failed read begun before a newer one doesn't overwrite its answer", async () => {
    const { refreshStatus, getLabState } = await freshStore();
    const poll = refreshStatus(lab);
    const fresh = refreshStatus(lab, { fresh: true });
    replies[1].resolve({ running: true } as unknown as LabStatus);
    await fresh;
    replies[0].reject({ code: "other", message: "late" });
    await poll;
    expect(getLabState().statusErrors).toEqual({});
  });
});
