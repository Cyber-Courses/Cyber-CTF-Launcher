// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { getDockerEngine, getLocalVmForm, getPortMode, getVmProvider, setLocalVmForm, setPortMode, setVmProvider } from "@/lib/settings";
import type { DockerEngine, LabRunOptions, LabRunPath, Provider } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The launch picker's steps: a remembered choice skips its step, Back walks to the step before,
// and "Don't ask again" saves what the path chose.

const path = (target: string, provider: Provider): LabRunPath => ({ provider, badge: "none", evidence: target });
/** A dual-form lab's run options here: its helper VM and one VM per machine, each on VirtualBox and QEMU. */
const options: LabRunOptions = {
  docker: path("compose", "virtualbox"),
  dockerVm: [path("docker-vm", "virtualbox"), path("docker-vm", "qemu")],
  vms: [path("vagrant", "virtualbox"), path("vagrant", "qemu")],
  servers: [],
  cloudDocker: [],
  cloudVm: [],
  cloudServices: [],
  refused: [],
  emulated: false,
};
const source = { repository: "CyberCTF/lab", commit: "0123456789abcdef0123456789abcdef01234567" };
const dualForm: LabRuntimeInfo = { runtime: "DOCKER", architectures: [], providers: ["virtualbox", "qemu"], docker: true, vm: true, ...source };
const ready: Provider[] = ["virtualbox", "qemu"];

async function picker(engines: DockerEngine[] = []) {
  installTauri({ lab_run_options: () => options });
  const hook = renderHook(() => useRunTarget({ id: "lab-t", runtime: dualForm }, ready, engines.at(0) ?? null, engines));
  await act(() => flush());
  return hook.result;
}

beforeEach(() => {
  localStorage.clear();
});

describe("useRunTarget's steps", () => {
  it("asks for the form, then the hypervisor, and Back from the hypervisor returns to the form", async () => {
    const run = await picker();
    act(() => {
      run.current.steps.pickPlatform({ kind: "vm" });
    });
    expect(run.current.steps.step).toBe("form");
    act(() => {
      run.current.steps.pickForm({ kind: "local-vm", provider: "virtualbox" });
    });
    expect(run.current.steps.step).toBe("hypervisor");
    expect(run.current.steps.isLastStep).toBe(true);
    act(() => {
      run.current.steps.back();
    });
    expect(run.current.steps.step).toBe("form");
  });

  it("skips the hypervisor step for the one Settings remembers, the form step ending the path on it", async () => {
    setVmProvider("qemu");
    const run = await picker();
    act(() => {
      run.current.steps.pickPlatform({ kind: "vm" });
    });
    act(() => {
      run.current.steps.pickForm({ kind: "local-vm", provider: "virtualbox" });
    });
    expect(run.current.steps.step).toBe("form");
    expect(run.current.steps.isLastStep).toBe(true);
    expect(run.current.runOn).toEqual({ kind: "local-vm", provider: "qemu" });
  });

  it("skips the form step for a remembered form, and Back from the hypervisor returns to the places", async () => {
    setLocalVmForm("vms");
    const run = await picker();
    act(() => {
      run.current.steps.pickPlatform({ kind: "vm" });
    });
    expect(run.current.steps.step).toBe("hypervisor");
    expect(run.current.runOn.kind).toBe("local-vms");
    act(() => {
      run.current.steps.back();
    });
    expect(run.current.steps.step).toBe("platform");
  });

  it("asks for the ports after the engine, unless they are remembered: the engine step ends the path then", async () => {
    const run = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      run.current.steps.pickPlatform({ kind: "containers" });
    });
    expect(run.current.steps.step).toBe("engine");
    expect(run.current.steps.isLastStep).toBe(false);
    act(() => {
      run.current.steps.pickEngine("orbstack");
    });
    expect(run.current.steps.step).toBe("ports");

    setPortMode("default");
    const remembered = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      remembered.current.steps.pickPlatform({ kind: "containers" });
    });
    expect(remembered.current.steps.isLastStep).toBe(true);
    act(() => {
      remembered.current.steps.pickEngine("orbstack");
    });
    expect(remembered.current.steps.step).toBe("engine");
  });
});

describe("useRunTarget's Don't ask again", () => {
  it("saves the container path's ports and engine on Start, only when checked", async () => {
    const run = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      run.current.steps.pickPlatform({ kind: "containers" });
      run.current.steps.pickEngine("orbstack");
      run.current.setPorts("default");
    });
    act(() => {
      run.current.dontAskAgain.commit();
    });
    expect(getPortMode()).toBeNull();
    act(() => {
      run.current.dontAskAgain.set(true);
    });
    act(() => {
      run.current.dontAskAgain.commit();
    });
    expect(getPortMode()).toBe("default");
    expect(getDockerEngine()).toBe("orbstack");
  });

  it("saves the VM path's form, and its hypervisor when there was one to choose", async () => {
    const run = await picker();
    act(() => {
      run.current.steps.pickPlatform({ kind: "vm" });
    });
    act(() => {
      run.current.steps.pickForm({ kind: "local-vms", provider: "virtualbox" });
    });
    act(() => {
      run.current.steps.pickHypervisor("qemu");
      run.current.dontAskAgain.set(true);
    });
    act(() => {
      run.current.dontAskAgain.commit();
    });
    expect(getLocalVmForm()).toBe("vms");
    expect(getVmProvider()).toBe("qemu");
  });
});
