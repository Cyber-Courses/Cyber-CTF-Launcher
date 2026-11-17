import { describe, expect, it } from "vitest";
import { appExit, boxReport, fromRunEvents, isOperationId, logTail, operationEnd, startEnded, stuckLabReport } from "./diagnose.mjs";

// What a spec says of a lab that never showed running (support/diagnose.mjs): the words a failed
// check's reason carries, so the rig's one-line reason says which it was: a start still in
// flight (and at which step), one that failed (and the log's last lines), or one that never began.

const ID = "local-invoice-portal-api";
const launch = (step: string | null = null) => ({ labId: ID, op: "launch", machine: null, deploying: true, step, runtime: "DOCKER" });
const status = (p: Partial<{ running: boolean; parked: string | null; machines: { name: string; state: string }[] }> = {}) => ({
  running: false,
  parked: null,
  machines: [],
  ...p,
});

describe("stuckLabReport", () => {
  it("names the operation in flight and its step", () => {
    const report = stuckLabReport(ID, { status: status(), operations: [launch("Pulling the environment's images…")], log: "" });
    expect(report).toBe(`${ID}: in flight: launch at "Pulling the environment's images…"; status: not running; no deploy log`);
  });

  it("quotes the deploy log's last lines when nothing is in flight: a start that failed", () => {
    const log = "importing the lab\n\nstep up\r\n  Image web Pulling\nError: download_failed: connection reset by peer\n";
    const report = stuckLabReport(ID, { status: status(), operations: [], log });
    expect(report).toBe(
      `${ID}: no operation in flight; status: not running; the deploy log ends: importing the lab | step up | Image web Pulling | Error: download_failed: connection reset by peer`,
    );
  });

  it("says what the status holds: parked, the machines and their states, another lab's operation left out", () => {
    const seen = {
      status: status({
        parked: "shutdown",
        machines: [
          { name: "web", state: "exited" },
          { name: "database", state: "exited" },
        ],
      }),
      operations: [{ ...launch(), labId: "lab-other" }],
      log: null,
    };
    expect(stuckLabReport(ID, seen)).toBe(`${ID}: no operation in flight; status: parked (web exited, database exited)`);
  });

  it("says what couldn't be read, and that nothing could", () => {
    expect(stuckLabReport(ID, { status: null, operations: null, log: null, error: "lab_status: timed out after 10000ms" })).toBe(
      `${ID}: (the app couldn't be asked everything: lab_status: timed out after 10000ms)`,
    );
    expect(stuckLabReport(ID, { status: null, operations: null, log: null })).toBe(`${ID}: nothing could be read`);
  });
});

describe("startEnded", () => {
  it("is a start with nothing in flight and the lab not running; a status unread decides nothing", () => {
    expect(startEnded(ID, { status: status(), operations: [] })).toBe(true);
    expect(startEnded(ID, { status: status(), operations: [launch()] })).toBe(false);
    expect(startEnded(ID, { status: status({ running: true }), operations: [] })).toBe(false);
    expect(startEnded(ID, { status: status(), operations: [{ ...launch(), labId: "lab-other" }] })).toBe(true);
    expect(startEnded(ID, { status: null, operations: [] })).toBe(false);
  });
});

describe("logTail", () => {
  it("keeps the last lines, blank ones and line ends dropped", () => {
    expect(logTail("a\n\nb\r\nc\n", 2)).toBe("b | c");
    expect(logTail("", 3)).toBe("");
    expect(logTail(null as unknown as string)).toBe("");
    expect(logTail(Array.from({ length: 10 }, (_, i) => `line ${String(i)}`).join("\n"))).toBe("line 4 | line 5 | line 6 | line 7 | line 8 | line 9");
  });
});

describe("appExit", () => {
  it("says how the app ended, and nothing while it runs", () => {
    expect(appExit({ exitCode: null, signalCode: null })).toBeNull();
    expect(appExit(null)).toBeNull();
    expect(appExit({ exitCode: 1, signalCode: null })).toBe("the app exited with code 1");
    expect(appExit({ exitCode: -1073741819, signalCode: null })).toBe("the app exited with code -1073741819");
    expect(appExit({ exitCode: null, signalCode: "SIGSEGV" })).toBe("the app was ended by SIGSEGV");
  });
});

// The run events a window replays (`run_events`): one operation ended, an attach in flight.
const frames = [
  { cursor: 1, event: { kind: "started", operation: "op-1", labId: ID, op: "launch" } },
  { cursor: 2, event: { kind: "log", operation: "op-1", labId: ID, line: "Lab is up" } },
  { cursor: 3, event: { kind: "ended", operation: "op-1", labId: ID, op: "launch", error: null } },
  { cursor: 4, event: { kind: "started", operation: "op-2", labId: ID, op: "attach" } },
  { cursor: 5, event: { kind: "log", operation: "op-2", labId: ID, line: "Pulling kalilinux/kali-rolling" } },
  { cursor: 6, event: { kind: "log", operation: "op-9", labId: "other", line: "not ours" } },
];

describe("fromRunEvents", () => {
  it("reads a lab's operations in flight at their last line, and its last one with its log", () => {
    expect(fromRunEvents(ID, frames)).toEqual({
      operations: [{ labId: ID, op: "attach", step: "Pulling kalilinux/kali-rolling" }],
      last: { op: "attach", ended: null, log: "Pulling kalilinux/kali-rolling" },
    });
    const failed = [
      ...frames,
      { cursor: 7, event: { kind: "ended", operation: "op-2", labId: ID, op: "attach", error: { code: "other", message: "pull access denied" } } },
    ];
    expect(fromRunEvents(ID, failed)).toEqual({ operations: [], last: { op: "attach", ended: "pull access denied", log: "Pulling kalilinux/kali-rolling" } });
    expect(fromRunEvents(ID, null)).toEqual({ operations: [], last: null });
  });
});

describe("boxReport (#321)", () => {
  it("says the box's status, the runner's last operation on the lab, how it ended and its log's end", () => {
    const box = { running: false, reachable: false, ip: "", labNetwork: "lab", networks: [] };
    expect(boxReport(ID, { box, frames })).toBe(
      `${ID}: the box: running false, reachable false, labNetwork lab; the runner's last operation: attach, still in flight; its log ends: Pulling kalilinux/kali-rolling`,
    );
    expect(boxReport(ID, { box: null, frames: [], error: "attack_box_status: timed out after 10000ms" })).toBe(
      `${ID}: no operation of the lab's; (the app couldn't be asked everything: attack_box_status: timed out after 10000ms)`,
    );
  });
});

describe("operationEnd", () => {
  const op = "op-00000000000000a1";
  const frame = (cursor: number, event: Record<string, unknown>) => ({ cursor, event });
  const started = frame(1, { kind: "started", operation: op, labId: ID, op: "launch" });

  it("tells an operation's id from another answer", () => {
    expect(isOperationId(op)).toBe(true);
    for (const answer of [null, true, "op-", "local-lab", ["op-00000000000000a1"], { running: true }]) expect(isOperationId(answer)).toBe(false);
  });

  it("is null while the operation hasn't ended, another one's end left out", () => {
    expect(operationEnd(op, [started, frame(2, { kind: "ended", operation: "op-00000000000000b2", labId: ID, op: "stop", error: null })])).toBeNull();
    expect(operationEnd(op, null)).toBeNull();
  });

  it("says it went through, or why it failed", () => {
    expect(operationEnd(op, [started, frame(2, { kind: "ended", operation: op, labId: ID, op: "launch", error: null })])).toEqual({ ok: true });
    const error = { code: "refused", message: "QEMU can't pause a lab: shut it down instead." };
    expect(operationEnd(op, [started, frame(2, { kind: "ended", operation: op, labId: ID, op: "pause", error })])).toEqual({ err: error.message });
  });
});
