// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";

// The store reaches Tauri only through the polling hooks, which these tests don't render.
vi.mock("@/lib/tauri", () => ({ deployingLabs: async () => [], activeOperations: async () => [], labDeployLog: async () => "" }));

// Fresh module state per test (the store is a module singleton).
async function freshStore() {
  vi.resetModules();
  return import("@/lib/deploy-store");
}

describe("deploy-store: edges", () => {
  beforeEach(() => vi.resetModules());

  it("puts a run back exactly as it was, and forgets one that had no past", async () => {
    const { beginDeploy, appendDeployLog, endDeploy, restoreDeploy, getDeploySnapshot, subscribeDeploy } = await freshStore();
    beginDeploy("a", "launch");
    appendDeployLog("a", "line");
    endDeploy("a");
    const before = getDeploySnapshot().runs.a;
    beginDeploy("a", "resume");
    expect(getDeploySnapshot().runs.a.logs).toEqual([]);
    restoreDeploy("a", before);
    expect(getDeploySnapshot().runs.a).toBe(before);

    beginDeploy("c");
    restoreDeploy("c", undefined);
    expect("c" in getDeploySnapshot().runs).toBe(false);

    // Nothing to put back and nothing to remove: no change, so nobody is told.
    const notified = vi.fn();
    subscribeDeploy(notified);
    restoreDeploy("never", undefined);
    expect(notified).not.toHaveBeenCalled();
  });

  it("a log line arriving after the run ended leaves the lab idle", async () => {
    // appendDeployLog always writes `busy: true`. A channel message delivered after the
    // command's promise settled (Tauri does not order a channel's last message before the
    // return value) flips a finished run back to busy, and nothing ends it again: the row says
    // "Starting…" and Start stays disabled until the next run. Suggested fix: keep `prev.busy`
    // in appendDeployLog (begin/end own that flag).
    const { beginDeploy, appendDeployLog, endDeploy, getDeploySnapshot } = await freshStore();
    beginDeploy("a");
    endDeploy("a");
    appendDeployLog("a", "late line");
    expect(getDeploySnapshot().runs.a.busy).toBe(false);
  });

  it("never reads a signed-out from a log line: a lab's own output can say anything", async () => {
    const { appendDeployLog, SIGNED_OUT_EVENT } = await freshStore();
    const fired = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, fired);
    appendDeployLog("a", "user bob signed out of the domain controller");
    appendDeployLog("a", "✗ You're SIGNED OUT: sign in again");
    expect(fired).not.toHaveBeenCalled();
    window.removeEventListener(SIGNED_OUT_EVENT, fired);
  });

  it("keeps the code of the error a run failed with, beside its ✗ line", async () => {
    const { beginDeploy, failDeploy, getDeploySnapshot } = await freshStore();
    beginDeploy("a", "launch");
    failDeploy("a", { code: "portInUse", args: { port: 8080 }, message: "Host port 8080 is already in use" });
    const run = getDeploySnapshot().runs.a;
    expect(run.failureCode).toBe("portInUse");
    expect(run.logs.at(-1)).toMatch(/^✗ Host port 8080/);
  });

  it("a listener another one removes during a change is not called", async () => {
    // useSyncExternalStore unsubscribes during emits; a Set iteration skips entries deleted
    // before it reaches them.
    const { beginDeploy, subscribeDeploy } = await freshStore();
    let offB = () => {};
    const b = vi.fn();
    subscribeDeploy(() => offB());
    offB = subscribeDeploy(b);
    beginDeploy("a");
    expect(b).not.toHaveBeenCalled();
  });

  it("names a provisioning target without its isoloom- prefix, stripped once", async () => {
    const { operationLabel } = await freshStore();
    const label = operationLabel({ labId: "x", op: "provision", machine: "isoloom-isoloom-dc01", step: null });
    expect(label).toContain("isoloom-dc01");
    expect(label).not.toContain("isoloom-isoloom-");
    expect(operationLabel({ labId: "x", op: "provision", machine: null, step: null })).not.toContain("{");
  });
});
