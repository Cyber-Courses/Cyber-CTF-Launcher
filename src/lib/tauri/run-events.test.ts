// @vitest-environment jsdom
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it } from "vitest";
import { awaitOperation, connectRunEvents, inFlight, lastRunOf, runEventsNow, takeIn } from "@/lib/tauri/run-events";
import { EVENTS } from "@/lib/tauri/generated/Events";
import type { CommandError, Cursored } from "@/lib/tauri/generated";
import { installTauri, rejects } from "@/test/tauri";
import { flush } from "@/test/ui";

// The webview's one store of the lab operations, driven by a scripted stream of the core's run
// events: what it replays, what it follows live, and what each reader gets of it.

const started = (cursor: number, operation: string, labId: string, at = 0): Cursored => ({
  cursor,
  event: { kind: "started", operation, labId, op: "launch", machine: null, runtime: "DOCKER", deploying: true, at },
});
const log = (cursor: number, operation: string, labId: string, line: string): Cursored => ({ cursor, event: { kind: "log", operation, labId, line } });
const ended = (cursor: number, operation: string, labId: string, error: CommandError | null = null): Cursored => ({
  cursor,
  event: { kind: "ended", operation, labId, op: "launch", error },
});

describe("the run events store", () => {
  it("replays what the core kept, then follows the live events, each taken in once and in order", async () => {
    installTauri({ run_events: () => [started(1, "op-1", "web"), log(2, "op-1", "web", "Get images")] });
    await connectRunEvents();
    expect(runEventsNow().operations.get("op-1")?.lines).toEqual(["Get images"]);
    // Live: one after the replay, and one the replay already had (taken in once).
    await emit(EVENTS.runEvent, log(3, "op-1", "web", "Start containers"));
    await emit(EVENTS.runEvent, log(2, "op-1", "web", "Get images"));
    await flush();
    expect(runEventsNow().operations.get("op-1")?.lines).toEqual(["Get images", "Start containers"]);
    expect(runEventsNow().cursor).toBe(3);
  });

  it("asks the core only for what came after the last event it took in", async () => {
    const calls = installTauri({ run_events: () => [] });
    takeIn([started(1, "op-1", "web"), log(2, "op-1", "web", "a")]);
    await connectRunEvents();
    expect(calls.find((c) => c.cmd === "run_events")?.args).toMatchObject({ after: 2 });
  });

  it("keeps the operations in flight and each lab's last ended one", () => {
    takeIn([started(1, "op-1", "web", 10), started(2, "op-2", "ad", 20)]);
    expect(inFlight().map((r) => r.operation)).toEqual(["op-1", "op-2"]);
    takeIn([ended(3, "op-1", "web"), started(4, "op-3", "web", 30), ended(5, "op-3", "web", { code: "other", message: "no" })]);
    expect(inFlight().map((r) => r.operation)).toEqual(["op-2"]);
    expect(runEventsNow().operations.has("op-1")).toBe(false);
    expect(lastRunOf("web")?.operation).toBe("op-3");
    expect(lastRunOf("web")?.error?.message).toBe("no");
  });

  it("follows an operation to its end for the action that started it: every line, then its outcome", async () => {
    installTauri({ run_events: () => [] });
    takeIn([started(1, "op-1", "web"), log(2, "op-1", "web", "before")]);
    const lines: string[] = [];
    const done = awaitOperation("op-1", (l) => lines.push(l));
    await flush();
    takeIn([log(3, "op-1", "web", "after"), ended(4, "op-1", "web")]);
    await expect(done).resolves.toBeUndefined();
    expect(lines).toEqual(["before", "after"]);

    takeIn([started(5, "op-2", "web")]);
    const failed = awaitOperation("op-2", () => {});
    takeIn([ended(6, "op-2", "web", { code: "dockerNotRunning", message: "Docker isn't running." })]);
    await expect(failed).rejects.toMatchObject({ code: "dockerNotRunning" });
  });

  it("starts an operation through its command and follows it, a refusal rejecting at once", async () => {
    const { labResume } = await import("@/lib/tauri");
    installTauri({
      lab_resume: (a) => {
        (a?.logs as { onmessage: (l: string) => void }).onmessage("==> DC01: Resuming");
      },
    });
    const lines: string[] = [];
    await labResume("ad", "VM", (l) => lines.push(l));
    expect(lines).toEqual(["==> DC01: Resuming"]);
    installTauri({
      lab_resume: rejects({ code: "alreadyInProgress", message: "busy" }),
    });
    await expect(labResume("ad", "VM", () => {})).rejects.toMatchObject({ code: "alreadyInProgress" });
  });
});
