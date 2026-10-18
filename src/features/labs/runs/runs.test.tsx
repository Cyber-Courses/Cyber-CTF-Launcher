// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SIGNED_OUT_EVENT } from "@/features/account/signed-out";
import {
  appendDeployLog,
  beginDeploy,
  endDeploy,
  failDeploy,
  getDeploySnapshot,
  restoreDeploy,
  subscribeDeploy,
  type DeployRun,
} from "@/features/labs/runs/deploy-runs";
import { getLastRun, setLastRun } from "@/features/labs/runs/last-run";
import { operationLabel, OPERATION_STATUS, useActiveOperations, useDeployingLabs } from "@/features/labs/runs/operations";
import { timeLines, useWorkerLog } from "@/features/labs/runs/worker-log";
import { setLocalePreference } from "@/lib/i18n";
import type { ActiveOperation } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

afterEach(() => {
  act(() => {
    setLocalePreference("en");
  });
  vi.restoreAllMocks();
  vi.useRealTimers();
});

/** `id`'s run now; fails the test when it has none. */
function runOf(id: string): DeployRun {
  const run = getDeploySnapshot().runs[id];
  if (!run) throw new Error(`no run for ${id}`);
  return run;
}

describe("deploy runs", () => {
  it("keeps each lab's logs separate when two deploys run at once", () => {
    // Regression: a single global slot let a second deploy's lines (e.g. an ESXi VM lab) show on
    // the first lab's (e.g. a Docker lab) page. Keyed per lab, each stream stays its own.
    beginDeploy("docker-lab");
    beginDeploy("esxi-lab");
    appendDeployLog("docker-lab", "Container web-1 Started");
    appendDeployLog("esxi-lab", "vagrant up --provider vmware_esxi failed");
    appendDeployLog("docker-lab", "Container web-1 Healthy");
    expect(runOf("docker-lab").logs).toEqual(["Container web-1 Started", "Container web-1 Healthy"]);
    expect(runOf("esxi-lab").logs).toEqual(["vagrant up --provider vmware_esxi failed"]);
  });

  it("marks both labs busy while both deploy, and ends them independently", () => {
    beginDeploy("a");
    beginDeploy("b");
    expect([runOf("a").busy, runOf("b").busy]).toEqual([true, true]);
    endDeploy("a");
    // Ending one lab leaves the other running, and keeps the ended lab's logs for the console.
    expect([runOf("a").busy, runOf("b").busy]).toEqual([false, true]);
  });

  it("clears only that lab's previous logs when it begins again", () => {
    beginDeploy("a");
    appendDeployLog("a", "first run line");
    appendDeployLog("b", "b stays");
    beginDeploy("a");
    expect(runOf("a")).toMatchObject({ logs: [], busy: true });
    expect(runOf("b").logs).toEqual(["b stays"]);
  });

  it("records a timestamp parallel to every log line", () => {
    beginDeploy("a");
    appendDeployLog("a", "one");
    appendDeployLog("a", "two");
    expect(runOf("a").times).toHaveLength(2);
    expect(runOf("a").times.every((t) => typeof t === "number")).toBe(true);
  });

  it("ends no run for a lab that never started", () => {
    endDeploy("never-started");
    expect(getDeploySnapshot().runs["never-started"]).toBeUndefined();
  });

  it("notifies subscribers on every change, until they unsubscribe", () => {
    const cb = vi.fn();
    const unsub = subscribeDeploy(cb);
    beginDeploy("a");
    appendDeployLog("a", "x");
    endDeploy("a");
    expect(cb).toHaveBeenCalledTimes(3);
    unsub();
    appendDeployLog("a", "y");
    getDeploySnapshot();
    expect(cb).toHaveBeenCalledTimes(3);
  });

  it("adds streamed lines in batches, one update for many lines", () => {
    vi.useFakeTimers();
    beginDeploy("a");
    const cb = vi.fn();
    subscribeDeploy(cb);
    for (let i = 0; i < 50; i++) appendDeployLog("a", `line ${String(i)}`);
    appendDeployLog("b", "other lab");
    expect(cb).not.toHaveBeenCalled();
    vi.advanceTimersByTime(100);
    // 51 lines, two labs, one notification.
    expect(cb).toHaveBeenCalledTimes(1);
    expect(runOf("a").logs).toHaveLength(50);
    expect(runOf("a").times).toHaveLength(50);
    expect(runOf("b").logs).toEqual(["other lab"]);
    // Reading outside React, or ending the run, takes in the lines still waiting.
    appendDeployLog("a", "last");
    expect(runOf("a").logs.at(-1)).toBe("last");
    appendDeployLog("a", "✓ done");
    endDeploy("a");
    expect(runOf("a")).toMatchObject({ busy: false });
    expect(runOf("a").logs.at(-1)).toBe("✓ done");
  });

  it("puts a run back exactly as it was, and forgets one that had no past", () => {
    beginDeploy("a", "launch");
    appendDeployLog("a", "line");
    endDeploy("a");
    const before = runOf("a");
    beginDeploy("a", "resume");
    expect(runOf("a").logs).toEqual([]);
    restoreDeploy("a", before);
    expect(runOf("a")).toBe(before);
    beginDeploy("c");
    restoreDeploy("c", undefined);
    expect("c" in getDeploySnapshot().runs).toBe(false);
    // Nothing to put back and nothing to remove: no change, so nobody is told.
    const notified = vi.fn();
    subscribeDeploy(notified);
    restoreDeploy("never", undefined);
    expect(notified).not.toHaveBeenCalled();
  });

  it("leaves the lab idle when a log line arrives after the run ended", () => {
    // A channel's last message can arrive after the command's promise settled: `busy` stays
    // begin/end's to set.
    beginDeploy("a");
    endDeploy("a");
    appendDeployLog("a", "late line");
    expect(runOf("a").busy).toBe(false);
  });

  it("never reads a signed-out from a log line: a lab's own output can say anything", () => {
    const fired = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, fired);
    appendDeployLog("a", "user bob signed out of the domain controller");
    appendDeployLog("a", "✗ You're SIGNED OUT: sign in again");
    expect(runOf("a").logs).toHaveLength(2);
    expect(fired).not.toHaveBeenCalled();
    window.removeEventListener(SIGNED_OUT_EVENT, fired);
  });

  it("keeps the code of the error a run failed with, beside its ✗ line", () => {
    beginDeploy("a", "launch");
    failDeploy("a", { code: "portInUse", args: { port: 8080 }, message: "Host port 8080 is already in use" });
    expect(runOf("a").failureCode).toBe("portInUse");
    expect(runOf("a").logs.at(-1)).toMatch(/^✗ Host port 8080/);
  });

  it("doesn't call a listener another one removed during a change", () => {
    // useSyncExternalStore unsubscribes during emits; a Set iteration skips entries deleted
    // before it reaches them.
    let offB = () => {};
    const b = vi.fn();
    subscribeDeploy(() => {
      offB();
    });
    offB = subscribeDeploy(b);
    beginDeploy("a");
    expect(b).not.toHaveBeenCalled();
  });
});

describe("operations", () => {
  const op = (o: ActiveOperation["op"], machine: string | null = null): ActiveOperation => ({ labId: "l", op: o, machine, step: null });

  it("names every operation", () => {
    expect((["launch", "resume", "pause", "shutdown", "provision", "attack_vm", "stop"] as const).map((o) => operationLabel(op(o)))).toEqual([
      "Starting…",
      "Resuming…",
      "Pausing…",
      "Shutting down…",
      "Provisioning…",
      "Starting the attack VM…",
      "Stopping…",
    ]);
  });

  it("names a provisioning target without its isoloom- prefix, stripped once", () => {
    const label = operationLabel(op("provision", "isoloom-isoloom-dc01"));
    expect(label).toContain("isoloom-dc01");
    expect(label).not.toContain("isoloom-isoloom-");
    expect(operationLabel(op("provision"))).not.toContain("{");
  });

  it("names the status shown while an operation runs, in the current language", () => {
    const statuses = [OPERATION_STATUS.launch, OPERATION_STATUS.resume, OPERATION_STATUS.pause, OPERATION_STATUS.shutdown, OPERATION_STATUS.stop];
    expect(statuses.every(Boolean)).toBe(true);
    const en = OPERATION_STATUS.launch;
    setLocalePreference("de");
    expect(OPERATION_STATUS.launch).not.toBe(en);
  });

  it("polls the labs deploying and the operations in flight", async () => {
    installTauri();
    const deploying = renderHook(() => useDeployingLabs());
    const ops = renderHook(() => useActiveOperations());
    await flush();
    expect([...deploying.result.current]).toEqual(["lab-goad"]);
    expect(ops.result.current.get("lab-goad")?.step).toContain("SRV02");
  });
});

describe("worker log", () => {
  it("times a worker's log lines as they first show, starting over on a new run", () => {
    const empty = { lines: [], times: [] };
    const one = timeLines(empty, ["a"], 10);
    expect(one).toEqual({ lines: ["a"], times: [10] });
    const two = timeLines(one, ["a", "b"], 20);
    expect(two).toEqual({ lines: ["a", "b"], times: [10, 20] });
    // The same log again: the same object.
    expect(timeLines(two, ["a", "b"], 30)).toBe(two);
    // A new run (shorter, or another first line): timed afresh.
    expect(timeLines(two, ["x"], 40)).toEqual({ lines: ["x"], times: [40] });
    expect(timeLines(two, ["x", "y", "z"], 50)).toEqual({ lines: ["x", "y", "z"], times: [50, 50, 50] });
    expect(timeLines(two, [], 60)).toEqual(empty);
  });

  it("follows a worker's log only while active, skipping blank lines", async () => {
    const calls = installTauri({ lab_deploy_log: () => "line 1\n\nline 2\n" });
    const { result, rerender } = renderHook(({ active }) => useWorkerLog("lab-goad", active, 1000), { initialProps: { active: false } });
    await flush();
    expect(result.current.lines).toEqual([]);
    expect(commands(calls)).not.toContain("lab_deploy_log");
    rerender({ active: true });
    await flush();
    expect(result.current.lines).toEqual(["line 1", "line 2"]);
    expect(result.current.times).toHaveLength(2);
    expect(calls.find((c) => c.cmd === "lab_deploy_log")?.args).toMatchObject({ id: "lab-goad" });
  });
});

describe("last run", () => {
  it("stores when each lab was last launched", () => {
    expect(getLastRun("a")).toBeNull();
    vi.spyOn(Date, "now").mockReturnValueOnce(5).mockReturnValueOnce(9);
    setLastRun("a");
    setLastRun("b");
    expect([getLastRun("a"), getLastRun("b")]).toEqual([5, 9]);
  });

  it("reads an empty or non-numeric value as never run", () => {
    localStorage.setItem("cyberctf.lastrun.a", "");
    localStorage.setItem("cyberctf.lastrun.x", "garbage");
    expect([getLastRun("a"), getLastRun("x")]).toEqual([null, null]);
  });

  it("reads and writes nothing without storage, and never throws", () => {
    const refused = () => {
      throw new Error("SecurityError");
    };
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(refused);
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(refused);
    expect(() => {
      setLastRun("x");
    }).not.toThrow();
    expect(getLastRun("x")).toBeNull();
  });
});
