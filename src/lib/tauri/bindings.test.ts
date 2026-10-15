// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import * as tauri from "@/lib/tauri";
import { invokeStreaming } from "@/lib/tauri/invoke";
import { installTauri, stream } from "@/test/tauri";

// The typed wrappers are one-liners over `invoke`: each must call its snake_case Rust command.
describe("Tauri bindings", () => {
  it("every wrapper invokes a snake_case command", async () => {
    const calls = installTauri();
    const noop = () => {};
    const fns = Object.entries(tauri).filter(([, v]) => typeof v === "function") as [string, (...a: unknown[]) => unknown][];
    expect(fns.length).toBeGreaterThan(40);
    for (const [, fn] of fns) {
      const before = calls.length;
      // Positional args: ids and kinds as strings, callbacks last; enough for any wrapper.
      await Promise.resolve(fn("lab-invoice", "DOCKER", "x", 80, 24, noop)).catch(() => {});
      for (const c of calls.slice(before)) expect(c.cmd).toMatch(/^[a-z0-9_]+$|^plugin:/);
    }
    expect(new Set(calls.map((c) => c.cmd)).size).toBeGreaterThan(40);
  });

  it("passes the arguments the Rust side reads", async () => {
    const calls = installTauri();
    await tauri.labStatus("lab-invoice", "DOCKER");
    expect(calls.at(-1)).toEqual({ cmd: "lab_status", args: { id: "lab-invoice", runtime: "DOCKER" } });
    await tauri.awsMonthToDateCost();
    expect(calls.at(-1)).toEqual({ cmd: "aws_month_to_date_cost", args: { profile: null } });
    await tauri.serverTest("pve");
    expect(calls.at(-1)?.args).toEqual({ id: "pve" });
  });

  it("returns what the command answers", async () => {
    installTauri({ running_labs: () => ["a", "b"] });
    expect(await tauri.runningLabs()).toEqual(["a", "b"]);
  });

  it("rejects when the command fails", async () => {
    installTauri({
      lab_status: () => {
        throw new Error("docker is not running");
      },
    });
    await expect(tauri.labStatus("x", "DOCKER")).rejects.toThrow(/docker is not running/);
  });

  it("streams log lines over a channel, then resolves", async () => {
    installTauri({
      lab_stop: (a) => {
        stream(a, "Stopping api", "Removed network");
        return null;
      },
    });
    const lines: string[] = [];
    await tauri.labStop("lab-invoice", "DOCKER", (l) => lines.push(l));
    expect(lines).toEqual(["Stopping api", "Removed network"]);
  });

  it("streams typed events on the `events` channel", async () => {
    const calls = installTauri({
      custom_cmd: (a) => {
        stream(a, { step: 1 }, { step: 2 });
        return 7;
      },
    });
    const events: unknown[] = [];
    const out = await invokeStreaming<number>("custom_cmd", { id: "x" }, (e: { step: number }) => events.push(e), "events");
    expect(out).toBe(7);
    expect(events).toEqual([{ step: 1 }, { step: 2 }]);
    expect(Object.keys(calls[0].args ?? {})).toEqual(["id", "events"]);
  });
});
