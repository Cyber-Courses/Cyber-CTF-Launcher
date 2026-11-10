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
      run.current.pickPlatform({ kind: "vm" });
    });
    expect(run.current.step).toBe("form");
    act(() => {
      run.current.pickForm({ kind: "local-vm", provider: "virtualbox" });
    });
    expect(run.current.step).toBe("hypervisor");
    expect(run.current.isLastStep).toBe(true);
    act(() => {
      run.current.back();
    });
    expect(run.current.step).toBe("form");
  });

  it("skips the hypervisor step for the one Settings remembers, the form step ending the path on it", async () => {
    setVmProvider("qemu");
    const run = await picker();
    act(() => {
      run.current.pickPlatform({ kind: "vm" });
    });
    act(() => {
      run.current.pickForm({ kind: "local-vm", provider: "virtualbox" });
    });
    expect(run.current.step).toBe("form");
    expect(run.current.isLastStep).toBe(true);
    expect(run.current.runOn).toEqual({ kind: "local-vm", provider: "qemu" });
  });

  it("skips the form step for a remembered form, and Back from the hypervisor returns to the places", async () => {
    setLocalVmForm("vms");
    const run = await picker();
    act(() => {
      run.current.pickPlatform({ kind: "vm" });
    });
    expect(run.current.step).toBe("hypervisor");
    expect(run.current.runOn.kind).toBe("local-vms");
    act(() => {
      run.current.back();
    });
    expect(run.current.step).toBe("platform");
  });

  it("asks for the ports after the engine, unless they are remembered: the engine step ends the path then", async () => {
    const run = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      run.current.pickPlatform({ kind: "containers" });
    });
    expect(run.current.step).toBe("engine");
    expect(run.current.isLastStep).toBe(false);
    act(() => {
      run.current.pickEngine("orbstack");
    });
    expect(run.current.step).toBe("ports");

    setPortMode("default");
    const remembered = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      remembered.current.pickPlatform({ kind: "containers" });
    });
    expect(remembered.current.isLastStep).toBe(true);
    act(() => {
      remembered.current.pickEngine("orbstack");
    });
    expect(remembered.current.step).toBe("engine");
  });
});

describe("useRunTarget's Don't ask again", () => {
  it("saves the container path's ports and engine on Start, only when checked", async () => {
    const run = await picker(["docker-desktop", "orbstack"]);
    act(() => {
      run.current.pickPlatform({ kind: "containers" });
      run.current.pickEngine("orbstack");
      run.current.setPorts("default");
    });
    act(() => {
      run.current.commitChoice();
    });
    expect(getPortMode()).toBeNull();
    act(() => {
      run.current.setRemember(true);
    });
    act(() => {
      run.current.commitChoice();
    });
    expect(getPortMode()).toBe("default");
    expect(getDockerEngine()).toBe("orbstack");
  });

  it("saves the VM path's form, and its hypervisor when there was one to choose", async () => {
    const run = await picker();
    act(() => {
      run.current.pickPlatform({ kind: "vm" });
    });
    act(() => {
      run.current.pickForm({ kind: "local-vms", provider: "virtualbox" });
    });
    act(() => {
      run.current.pickHypervisor("qemu");
      run.current.setRemember(true);
    });
    act(() => {
      run.current.commitChoice();
    });
    expect(getLocalVmForm()).toBe("vms");
    expect(getVmProvider()).toBe("qemu");
  });
});
