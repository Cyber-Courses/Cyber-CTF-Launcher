import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";

// labStatus answers from a queue of deferred replies, so a test decides which read lands first.
const replies: { resolve: (s: LabStatus) => void; reject: (e: unknown) => void }[] = [];
vi.mock("@/lib/tauri", () => ({
  labStatus: () => new Promise<LabStatus>((resolve, reject) => replies.push({ resolve, reject })),
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
    setScanRunning(["lab-1"]);
    const scan = getLabState().scanRunning;
    setScanRunning(["lab-1"]);
    expect(getLabState().scanRunning).toBe(scan);
    setScanRunning([]);
    expect(getLabState().scanRunning.size).toBe(0);
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
    replies[2].resolve(status(false));
    await ok;
    expect("lab-1" in getLabState().statusErrors).toBe(false);
    warn.mockRestore();
  });

  it("a failed read begun before a newer one doesn't overwrite its answer", async () => {
    const { refreshStatus, getLabState } = await freshStore();
    const poll = refreshStatus(lab);
    const fresh = refreshStatus(lab, { fresh: true });
    replies[1].resolve(status(true));
    await fresh;
    replies[0].reject({ code: "other", message: "late" });
    await poll;
    expect(getLabState().statusErrors).toEqual({});
  });
});
