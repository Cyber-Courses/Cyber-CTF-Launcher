import { describe, expect, it } from "vitest";
import { appExit, logTail, startEnded, stuckLabReport } from "./lib/diagnose.mjs";

// What a spec says of a lab that never showed running (lib/diagnose.mjs): the words a failed
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
