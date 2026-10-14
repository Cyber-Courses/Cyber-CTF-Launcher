// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OfflineStartPrompt } from "@/features/labs/offline-start-prompt";
import { PortModePrompt } from "@/features/labs/port-mode-prompt";
import { useLabActions } from "@/features/labs/use-lab-actions";
import type { Lab } from "@/features/labs/use-labs";
import { getDeploySnapshot } from "@/lib/deploy-store";
import { setAttackBox, setAttackImage, setAutoAttackBox, setPortMode, setVmProvider } from "@/lib/settings";
import { readyReport as armReport } from "@/test/fixtures";
import { installTauri, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";

const dockerLab: Lab = {
  id: "act-docker",
  slug: "act-docker",
  title: "Invoice portal API",
  description: null,
  question: null,
  difficulty: 1,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: ["x86_64", "arm64"], providers: ["aws", "virtualbox"] },
  skills: [],
};
const vmLab: Lab = {
  ...dockerLab,
  id: "act-vm",
  slug: "act-vm",
  title: "GOAD Light",
  runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi", "proxmox"] },
};

const launchArgs = (calls: Call[]) => {
  const c = calls.find((x) => x.cmd === "lab_launch");
  if (!c) return undefined;
  const { labId, provider, host, attackboxImage, defaultPorts } = c.args ?? {};
  return { labId, provider, host, attackboxImage, defaultPorts };
};
// The dev mock's machine is an arm64 Mac; most tests here want an x86 one, where the VM labs run natively.
const readyReport = () => ({ ...armReport(), arch: "x86_64" });
const logsOf = (id: string) => getDeploySnapshot().runs[id]?.logs ?? [];

function setup(overrides: Parameters<typeof installTauri>[0] = {}) {
  const calls = installTauri(overrides);
  const refresh = vi.fn();
  const hook = renderHook(() => useLabActions(refresh));
  return { calls, refresh, hook };
}

afterEach(() => {
  setPortMode(null);
  setVmProvider(null);
});

describe("useLabActions: launch", () => {
  it("starts a cloud lab only on the cloud account picked, without an attack box", async () => {
    const cloudLab: Lab = { ...dockerLab, id: "act-cloud", slug: "act-cloud", runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"] } };
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch(cloudLab, null, undefined, readyReport()));
    expect(launchArgs(calls)).toBeUndefined();
    expect(logsOf("act-cloud").at(-1)).toMatch(/^✗ .*pick one of your cloud accounts/);
    await act(() => hook.result.current.launch(cloudLab, "aws1", undefined, readyReport()));
    expect(launchArgs(calls)).toEqual({ labId: "act-cloud", provider: null, host: "aws1", attackboxImage: null, defaultPorts: false });
    expect(logsOf("act-cloud").at(-1)).toBe("✓ Lab is running");
  });

  it("starts a container lab here on the ports chosen in the dialog, without an attack box image", async () => {
    const { calls, refresh, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined, readyReport(), "default"));
    expect(launchArgs(calls)).toEqual({ labId: "act-docker", provider: null, host: null, attackboxImage: null, defaultPorts: true });
    expect(logsOf("act-docker")).toEqual(["==> api: Creating", "✓ Lab ready", "✓ Lab is running"]);
    expect(refresh).toHaveBeenCalledWith(dockerLab, { fresh: true });
    expect(getDeploySnapshot().runs["act-docker"].busy).toBe(false);
    expect(localStorage.getItem("cyberctf.lastrun.act-docker")).not.toBeNull();
  });

  it("uses the port mode remembered in Settings without asking", async () => {
    setPortMode("random");
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined, readyReport()));
    expect(launchArgs(calls)?.defaultPorts).toBe(false);
  });

  it("asks for the ports and starts nothing when the player cancels", async () => {
    const { calls, hook } = setup();
    const user = userEvent.setup();
    render(<PortModePrompt />);
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.launch({ ...dockerLab, id: "act-cancel" }, null, undefined, readyReport());
    });
    await flush();
    expect(screen.getByText("Which ports should Invoice portal API use?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await act(() => done);
    expect(launchArgs(calls)).toBeUndefined();
    expect(getDeploySnapshot().runs["act-cancel"]).toBeUndefined();
  });

  /** A `lab_launch` failing as the backend does when Cyber CTF can't be reached: the second
   *  time, offline, it runs. Its calls' `offline` flags land in `asked`. */
  function unreachableBackend(reason: string, asked: unknown[]) {
    return {
      lab_launch: (args: Record<string, unknown> | undefined) => {
        asked.push(args?.offline);
        if (!args?.offline) throw reason;
        return null;
      },
    };
  }

  it("offers an offline start when Cyber CTF can't be reached and the lab is installed, and starts it on yes", async () => {
    const offline: unknown[] = [];
    const { hook } = setup(
      unreachableBackend("API unreachable: offline. This lab is installed here, so it can start offline, without saving progress.", offline),
    );
    const user = userEvent.setup();
    render(<OfflineStartPrompt />);
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.launch({ ...dockerLab, id: "act-offline" }, null, undefined, readyReport(), "random");
    });
    await flush();
    expect(screen.getByText("Start Invoice portal API offline?")).toBeTruthy();
    expect(screen.getByText(/progress and evidence aren't saved/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Start offline" }));
    await act(() => done);
    expect(offline).toEqual([false, true]);
    expect(logsOf("act-offline").at(-1)).toBe("✓ Lab is running");
  });

  it("starts nothing offline when the player declines, and says why the launch failed", async () => {
    const offline: unknown[] = [];
    const { hook } = setup(
      unreachableBackend("API unreachable: offline. This lab is installed here, so it can start offline, without saving progress.", offline),
    );
    const user = userEvent.setup();
    render(<OfflineStartPrompt />);
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.launch({ ...dockerLab, id: "act-decline" }, null, undefined, readyReport(), "random");
    });
    await flush();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await act(() => done);
    expect(offline).toEqual([false]);
    expect(logsOf("act-decline").at(-1)).toMatch(/^✗ API unreachable/);
  });

  it("never offers an offline start for a refusal or a lab that can't start offline", async () => {
    for (const reason of ["You don't have access to this lab.", "API unreachable: offline", "You're signed out. Sign in again to continue."]) {
      const offline: unknown[] = [];
      const { hook } = setup(unreachableBackend(reason, offline));
      render(<OfflineStartPrompt />);
      await act(() => hook.result.current.launch({ ...dockerLab, id: "act-refused" }, null, undefined, readyReport(), "random"));
      expect(screen.queryByRole("alertdialog")).toBeNull();
      expect(offline).toEqual([false]);
    }
  });

  it("asks for the ports, remembers them, and starts on the lab's own ports", async () => {
    const { calls, hook } = setup();
    const user = userEvent.setup();
    render(<PortModePrompt />);
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.launch({ ...dockerLab, id: "act-docker-2" }, null, undefined, readyReport());
    });
    await flush();
    await user.click(screen.getByText("The lab's default ports"));
    expect(screen.getByText(/may already be taken/)).toBeTruthy();
    await user.click(screen.getByRole("switch", { name: "Use this for every lab" }));
    await user.click(screen.getByRole("button", { name: "Start lab" }));
    await act(() => done);
    expect(launchArgs(calls)?.defaultPorts).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
    // Remembered: the next start doesn't ask.
    const { getPortMode } = await import("@/lib/settings");
    expect(getPortMode()).toBe("default");
  });

  it("runs a container lab on a server with the container attack box from Settings", async () => {
    setAttackImage("my/attack:latest");
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-remote" }, "aws-1", undefined, readyReport()));
    expect(launchArgs(calls)).toEqual({ labId: "act-remote", provider: null, host: "aws-1", attackboxImage: "my/attack:latest", defaultPorts: false });
  });

  it("runs a container lab inside a local VM on the chosen hypervisor", async () => {
    setAttackImage("");
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-in-vm" }, null, "virtualbox", readyReport()));
    const args = launchArgs(calls)!;
    expect(args.provider).toBe("virtualbox");
    expect(args.host).toBeNull();
    expect(typeof args.attackboxImage).toBe("string");
    expect(args.attackboxImage).not.toBe("");
  });

  it("runs a VM lab here on the ready hypervisor, with the attack VM box when auto-start is on", async () => {
    setAttackBox("kalilinux/rolling");
    setAutoAttackBox(true);
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-here" }, null, undefined, readyReport()));
    expect(launchArgs(calls)).toEqual({ labId: "act-vm-here", provider: "virtualbox", host: null, attackboxImage: "kalilinux/rolling", defaultPorts: false });
  });

  it("runs a VM lab here without an attack VM when auto-start is off", async () => {
    setAutoAttackBox(false);
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-noauto" }, null, undefined, readyReport()));
    expect(launchArgs(calls)?.attackboxImage).toBeNull();
    setAutoAttackBox(true);
  });

  it("sends a VM lab to the default server when it supports that server's hypervisor", async () => {
    const servers = () => ({ default: "esx", hosts: [{ id: "esx", name: "esx", provider: "vmware_esxi", host: "10.0.0.2" }] });
    const { calls, hook } = setup({ server_list: servers });
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-def" }, undefined, undefined, readyReport()));
    expect(launchArgs(calls)).toMatchObject({ provider: null, host: "esx" });
  });

  it("never sends a VM lab to a default Proxmox server implicitly", async () => {
    // The dev mock's default host is Proxmox.
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-pve" }, undefined, undefined, readyReport()));
    expect(launchArgs(calls)).toMatchObject({ provider: "virtualbox", host: null });
  });

  it("runs a VM lab here when the server list can't be read", async () => {
    const { calls, hook } = setup({
      server_list: () => {
        throw new Error("no servers");
      },
    });
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-nolist" }, undefined, undefined, readyReport()));
    expect(launchArgs(calls)).toMatchObject({ provider: "virtualbox", host: null });
  });

  it("says no hypervisor is installed instead of starting a VM lab", async () => {
    const report = readyReport();
    report.vmProviders = report.vmProviders.map((p) => ({ ...p, available: false }));
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-none" }, null, undefined, report));
    expect(launchArgs(calls)).toBeUndefined();
    expect(logsOf("act-vm-none").at(-1)).toMatch(/^✗ Error: No hypervisor for this lab is installed on this machine \(it runs on virtualbox\)/);
  });

  it("names 'none' when the lab lists no local hypervisor", async () => {
    const report = readyReport();
    const { hook } = setup();
    const lab: Lab = { ...vmLab, id: "act-vm-cloudonly", runtime: { runtime: "VM", architectures: [], providers: ["aws"] } };
    await act(() => hook.result.current.launch(lab, null, undefined, report));
    expect(logsOf("act-vm-cloudonly").at(-1)).toContain("(it runs on none)");
  });

  it("explains a VM lab built for another CPU on Apple Silicon", async () => {
    const report = { ...readyReport(), arch: "aarch64" };
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-arm" }, null, undefined, report));
    expect(launchArgs(calls)).toBeUndefined();
    expect(logsOf("act-vm-arm").at(-1)).toContain("built for x86_64 and this machine is aarch64");
  });

  it("starts an x86 VM lab emulated on QEMU when it is ready", async () => {
    const report = { ...readyReport(), arch: "aarch64" };
    report.vmProviders = [
      ...report.vmProviders,
      { provider: "qemu", remote: false, available: true, hypervisor: true, plugin: null, pluginInstalled: true, reason: null },
    ];
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-qemu" }, null, undefined, report));
    expect(launchArgs(calls)?.provider).toBe("qemu");
  });

  it("picks the Settings hypervisor before the machine report has loaded", async () => {
    setVmProvider("vmware_esxi");
    const { calls, hook } = setup();
    const lab: Lab = { ...vmLab, id: "act-vm-noreport", runtime: { runtime: "VM", architectures: [], providers: ["virtualbox", "vmware_desktop"] } };
    await act(() => hook.result.current.launch(lab, null, undefined, null));
    expect(launchArgs(calls)?.provider).toBe("virtualbox");
  });

  it("puts a failed start on the log's last line and ends the run", async () => {
    const { hook } = setup({
      lab_launch: () => {
        throw "Port 8000 is already in use";
      },
    });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-fail" }, null, undefined, readyReport(), "random"));
    expect(logsOf("act-fail")).toEqual(["✗ Port 8000 is already in use"]);
    expect(getDeploySnapshot().runs["act-fail"].busy).toBe(false);
  });

  it("does nothing for a lab without a runtime", async () => {
    const { calls, hook } = setup();
    const lab = { ...dockerLab, id: "act-none", runtime: null };
    await act(() => hook.result.current.launch(lab, null));
    await act(() => hook.result.current.stop(lab));
    expect(calls.filter((c) => c.cmd.startsWith("lab_"))).toEqual([]);
  });
});

describe("useLabActions: operations on a built lab", () => {
  it("stops a lab with its runtime", async () => {
    const { calls, hook, refresh } = setup();
    await act(() => hook.result.current.stop(vmLab));
    expect(calls.find((c) => c.cmd === "lab_stop")?.args).toMatchObject({ id: "act-vm", runtime: "VM" });
    expect(logsOf("act-vm")).toEqual(["==> api: Stopping", "✓ Lab stopped"]);
    expect(getDeploySnapshot().runs["act-vm"].op).toBe("stop");
    expect(refresh).toHaveBeenCalledWith(vmLab, { fresh: true });
  });

  it("pauses and shuts down a lab", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.park(vmLab, "pause"));
    expect(logsOf("act-vm").at(-1)).toBe("✓ Lab paused");
    await act(() => hook.result.current.park(vmLab, "shutdown"));
    expect(logsOf("act-vm").at(-1)).toBe("✓ Lab shut down");
    expect(calls.filter((c) => c.cmd === "lab_park").map((c) => c.args?.mode)).toEqual(["pause", "shutdown"]);
  });

  it("resumes a lab", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.resume(vmLab));
    expect(calls.find((c) => c.cmd === "lab_resume")?.args).toMatchObject({ id: "act-vm", runtime: "VM" });
    expect(logsOf("act-vm")).toEqual(["==> DC01: Resuming", "✓ Lab is running"]);
  });

  it("re-runs the setup on one machine or on all of them", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.provision(vmLab, "DC01"));
    expect(logsOf("act-vm").at(-1)).toBe("✓ DC01 provisioned");
    await act(() => hook.result.current.provision(vmLab, null));
    expect(logsOf("act-vm").at(-1)).toBe("✓ Lab provisioned");
    expect(calls.filter((c) => c.cmd === "lab_provision").map((c) => c.args?.machine)).toEqual(["DC01", null]);
  });

  it("keeps the lab busy until its status after the operation has been read", async () => {
    installTauri();
    let release!: () => void;
    const refresh = vi.fn(() => new Promise<void>((r) => (release = r)));
    const hook = renderHook(() => useLabActions(refresh));
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.stop({ ...vmLab, id: "act-settle" });
    });
    await flush();
    expect(getDeploySnapshot().runs["act-settle"].busy).toBe(true);
    expect(hook.result.current.runs["act-settle"].busy).toBe(true);
    await act(async () => {
      release();
      await done;
    });
    expect(getDeploySnapshot().runs["act-settle"].busy).toBe(false);
  });
});
