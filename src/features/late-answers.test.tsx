// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import { useSystemCheck } from "@/components/shell/use-system-check";
import { useHostedLabs, type HostedSession } from "@/features/hosted/use-hosted-labs";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { SERVER_CHANGED, type ServerList, type ServerTest, type SystemReport } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";
import { deferred } from "@/test/deferred";

// Async answers applied in the order they arrive let an older, slower one overwrite a newer one.
// Each hook here keeps only the newest request's answer (useLatest).

/** Each call to a command answers with the next of `answers`, settled by the test. */
function queue<T>(count: number) {
  const answers = Array.from({ length: count }, () => deferred<T>());
  let next = 0;
  return { answers, handler: () => answers[next++].promise };
}

const settle = (fn: () => void) =>
  act(async () => {
    fn();
    await flush(5);
  });

afterEach(() => {
  document.body.innerHTML = "";
});

describe("late answers", () => {
  it("a slow, older system check doesn't put back a stale state over a newer one", async () => {
    const { answers, handler } = queue<SystemReport>(2);
    installTauri({ system_check: handler });
    const { result } = renderHook(() => useSystemCheck());
    act(() => void result.current.check());
    act(() => void result.current.check());
    await settle(() => {
      answers[1].resolve(readyReport());
    });
    await settle(() => {
      answers[0].reject({ code: "dockerNotRunning", message: "Docker isn't running." });
    });
    expect(result.current.checkError).toBeNull();
    expect(result.current.report?.dockerRunning).toBe(true);
  });

  it("the server list read on mount doesn't overwrite the one a change asked for", async () => {
    const { answers, handler } = queue<ServerList>(2);
    installTauri({ server_list: handler });
    const { result } = renderHook(() => useServerList());
    await act(() => emit(SERVER_CHANGED));
    await settle(() => {
      answers[1].resolve({ default: null, hosts: [] });
    });
    await settle(() => {
      answers[0].resolve({ default: "old", hosts: [{ id: "old" } as ServerList["hosts"][number]] });
    });
    expect(result.current.list).toEqual({ default: null, hosts: [] });
  });

  it("two Test clicks on a host show the newest test, whichever answers last", async () => {
    const { answers, handler } = queue<ServerTest>(2);
    installTauri({ server_test: handler });
    const { result } = renderHook(() => useHostTests());
    act(() => void result.current.test("pve"));
    act(() => void result.current.test("pve"));
    const test = (ok: boolean) => ({ ok, reachable: ok, authenticated: ok, latencyMs: 3, message: ok ? "Connected" : "timed out", checks: [] });
    await settle(() => {
      answers[1].resolve(test(true));
    });
    await settle(() => {
      answers[0].resolve(test(false));
    });
    expect(result.current.tests.pve).toMatchObject({ ok: true, message: "Connected" });
  });

  it("the hosted session read on mount doesn't bring back one the player just stopped", async () => {
    const active = deferred<unknown>();
    const running: HostedSession = {
      id: "s1",
      labId: "lab",
      target: "hosted",
      state: "RUNNING",
      endpoints: [],
      message: null,
      expiresAt: "2026-10-11T12:00:00Z",
    };
    installTauri({
      api_query: (a) => (a?.operation === "activeSession" ? active.promise : a?.operation === "launchHosted" ? { requestLabLaunch: running } : {}),
    });
    const { result } = renderHook(() => useHostedLabs());
    await act(() => result.current.launch("lab"));
    expect(result.current.session?.id).toBe("s1");
    await act(() => result.current.stop());
    await settle(() => {
      active.resolve({ myActiveLabSession: running });
    });
    expect(result.current.session).toBeNull();
  });

  it("stops the session a launch makes when the player stopped while it was requested", async () => {
    const launched = deferred<unknown>();
    const running: HostedSession = {
      id: "s2",
      labId: "lab",
      target: "hosted",
      state: "REQUESTED",
      endpoints: [],
      message: null,
      expiresAt: "2026-10-11T12:00:00Z",
    };
    const calls = installTauri({
      api_query: (a) => (a?.operation === "launchHosted" ? launched.promise : a?.operation === "activeSession" ? { myActiveLabSession: null } : {}),
    });
    const { result } = renderHook(() => useHostedLabs());
    await flush();
    let launching!: Promise<void>;
    act(() => {
      launching = result.current.launch("lab");
    });
    await act(() => result.current.stop());
    await act(async () => {
      launched.resolve({ requestLabLaunch: running });
      await launching;
      await flush(5);
    });
    expect(result.current.session).toBeNull();
    expect(calls.filter((c) => c.cmd === "api_query" && c.args?.operation === "stopSession").map((c) => c.args?.variables)).toEqual([{ id: "s2" }]);
  });

  it("keeps the target the player picked when the server list is read again", async () => {
    installTauri();
    const rt: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "proxmox", "vmware_esxi"] };
    const { result, rerender } = renderHook(({ ready }) => useRunTarget(rt, ready, "x86_64"), { initialProps: { ready: [] as never[] } });
    await flush();
    act(() => {
      result.current.setRunOn({ kind: "host", id: "esxi" });
    });
    // A server saved in its window, and the machine report arriving with a ready hypervisor.
    await act(() => emit(SERVER_CHANGED));
    rerender({ ready: ["virtualbox"] as never[] });
    await flush();
    expect(result.current.runOn).toEqual({ kind: "host", id: "esxi" });
  });
});
