// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import { useSystemCheck } from "@/components/shell/use-system-check";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { useHostTests, useServerList } from "@/features/servers/use-server-list";
import { SERVER_CHANGED, type Provider, type ServerList, type ServerTest, type SystemReport } from "@/lib/tauri";
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

  it("keeps the target the player picked when the server list is read again", async () => {
    installTauri();
    const rt: LabRuntimeInfo = { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "proxmox", "vmware_esxi"] };
    const { result, rerender } = renderHook(({ ready }) => useRunTarget({ id: "lab-t", runtime: rt }, ready, "x86_64"), {
      initialProps: { ready: [] as never[] },
    });
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

  it("offers one VM per machine where the backend's run options have it: a ready hypervisor that isn't UTM", async () => {
    // The backend reads the lab's spec: Isoloom refuses its per-machine VMs on UTM (no network).
    const path = (target: string, provider: Provider) => ({ provider, target, variant: provider, badge: "none" as const, evidence: "" });
    installTauri({
      lab_run_options: () => ({
        docker: null,
        dockerVm: [path("docker-vm", "utm"), path("docker-vm", "qemu")],
        vms: [path("vagrant", "qemu"), path("vagrant", "virtualbox")],
        servers: [],
        cloudDocker: [],
        cloudVm: [],
        cloudServices: [],
        refused: [{ provider: "utm", target: "vagrant", reason: "no network" }],
      }),
    });
    const source = { repository: "CyberCTF/lab", commit: "0123456789abcdef0123456789abcdef01234567" };
    const both: LabRuntimeInfo = { runtime: "DOCKER", architectures: [], providers: ["virtualbox"], docker: true, vm: true, ...source };
    const { result, rerender } = renderHook(({ ready }) => useRunTarget({ id: "lab-t", runtime: both }, ready, "aarch64"), {
      initialProps: { ready: ["utm"] as Provider[] },
    });
    await flush();
    // UTM alone: the helper VM, but no per-machine place (Isoloom refuses the form on UTM).
    expect(result.current.localVm).toBe("utm");
    expect(result.current.perMachine).toBeNull();
    rerender({ ready: ["utm", "qemu"] });
    await flush();
    expect(result.current.perMachine).toMatchObject({ provider: "qemu", providers: ["qemu"] });
    expect(result.current.hasChoice).toBe(true);
    act(() => {
      result.current.pickForm({ kind: "local-vms", provider: "qemu" });
    });
    expect(result.current.runOn).toEqual({ kind: "local-vms", provider: "qemu" });
    expect(result.current.chosen()).toEqual({ kind: "local-vms", provider: "qemu" });
  });
});
