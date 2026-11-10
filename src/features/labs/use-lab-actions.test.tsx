// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OfflineStartPrompt } from "@/features/labs/offline-start-prompt";
import { PortModePrompt } from "@/features/labs/port-mode-prompt";
import { useLabActions } from "@/features/labs/use-lab-actions";
import type { Lab } from "@/features/labs/use-labs";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { SIGNED_OUT_EVENT } from "@/features/account/signed-out";
import { setLocalePreference } from "@/lib/i18n";
import type { CommandError } from "@/lib/errors";
import { setPortMode, setVmProvider, type PortMode } from "@/lib/settings";
import { readyReport as armReport } from "@/test/fixtures";
import { commands, installTauri, rejects, type Call } from "@/test/tauri";
import { flush } from "@/test/ui";
import { must, startInAct } from "@/components/test-support";
import { rejectWith } from "@/features/labs/labs-test-kit";
import type { ServerHost } from "@/lib/tauri";

const notified = vi.hoisted(() => vi.fn<(title: string, body?: string) => Promise<void>>(() => Promise.resolve()));
vi.mock("@/lib/notify", () => ({ notify: notified }));

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

const SHA = "0123456789abcdef0123456789abcdef01234567";
/** The VM lab with its source at a pinned commit, as the catalogue lists it. */
const sourcedVmLab: Lab = { ...vmLab, runtime: { ...must(vmLab.runtime), repository: "CyberCTF/goad", commit: SHA } };

/** The `lab_launch` call's arguments as sent (the flags `launchArgs` leaves out included). */
const rawLaunchArgs = (calls: Call[]) => calls.find((x) => x.cmd === "lab_launch")?.args;
const launchArgs = (calls: Call[]) => {
  const c = calls.find((x) => x.cmd === "lab_launch");
  if (!c) return undefined;
  const { labId, provider, host, defaultPorts } = c.args ?? {};
  return { labId, provider, host, defaultPorts };
};
// The dev mock's machine is an arm64 Mac; most tests here want an x86 one, where the VM labs run natively.
const readyReport = () => ({ ...armReport(), arch: "x86_64" });
const logsOf = (id: string) => {
  return getDeploySnapshot().runs[id]?.logs ?? [];
};

function setup(overrides: Parameters<typeof installTauri>[0] = {}, hosts: ServerHost[] = []) {
  const calls = installTauri(overrides);
  const refresh = vi.fn();
  const hook = renderHook(() => useLabActions(refresh, hosts));
  return { calls, refresh, hook };
}

/** A saved host `id` on `provider`. */
const savedHost = (id: string, provider: ServerHost["provider"]) => ({ id, name: id, provider }) as ServerHost;

/** Starts launching `lab` here (a dialog may then ask the player): the launch, to await once
 *  answered. */
function launching(hook: ReturnType<typeof setup>["hook"], lab: Lab, ports?: PortMode) {
  return startInAct(() => hook.result.current.launch(lab, null, undefined, readyReport(), { ports }));
}

/** A launch of the Docker lab as `id` with no port mode remembered, once the ports are asked. */
async function portsAsked(id: string) {
  const { calls, hook } = setup();
  const user = userEvent.setup();
  render(<PortModePrompt />);
  const done = launching(hook, { ...dockerLab, id });
  await flush();
  return { calls, user, done };
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
    expect(launchArgs(calls)).toEqual({ labId: "act-cloud", provider: null, host: "aws1", defaultPorts: false });
    expect(logsOf("act-cloud").at(-1)).toBe("✓ Lab is running");
  });

  it("starts a container lab here on the ports chosen in the dialog, without an attack box image", async () => {
    const { calls, refresh, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined, readyReport(), { ports: "default" }));
    expect(launchArgs(calls)).toEqual({ labId: "act-docker", provider: null, host: null, defaultPorts: true });
    expect(logsOf("act-docker")).toEqual(["==> api: Creating", "✓ Lab ready", "✓ Lab is running"]);
    expect(refresh).toHaveBeenCalledWith(dockerLab, { fresh: true });
    expect(getDeploySnapshot().runs["act-docker"]?.busy).toBe(false);
    expect(localStorage.getItem("cyberctf.lastrun.act-docker")).not.toBeNull();
  });

  it("uses the port mode remembered in Settings without asking", async () => {
    setPortMode("random");
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined, readyReport()));
    expect(launchArgs(calls)?.defaultPorts).toBe(false);
  });

  it("asks for the ports and starts nothing when the player cancels", async () => {
    const { calls, user, done } = await portsAsked("act-cancel");
    expect(screen.getByText("Which ports should Invoice portal API use?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await act(() => done);
    expect(launchArgs(calls)).toBeUndefined();
    expect(getDeploySnapshot().runs["act-cancel"]).toBeUndefined();
  });

  /** A `lab_launch` failing as the backend does when Cyber CTF can't be reached: the second
   *  time, offline, it runs. Its calls' `offline` flags land in `asked`. */
  function unreachableBackend(reason: CommandError, asked: unknown[]) {
    return {
      lab_launch: (args: Record<string, unknown> | undefined) => {
        asked.push(args?.offline);
        if (!args?.offline) rejectWith(reason);
        return null;
      },
    };
  }

  /** A launch of `id` while Cyber CTF can't be reached and the lab is installed, once the
   *  offline start is offered: the `offline` flags asked so far, and the launch to await. */
  async function offlineOffered(id: string) {
    const offline: unknown[] = [];
    const { hook } = setup(
      unreachableBackend(
        {
          code: "offlineReady",
          args: { detail: "API unreachable: offline" },
          message: "Cyber CTF can't be reached (API unreachable: offline). This lab is installed here, so it can start offline, without saving progress.",
        },
        offline,
      ),
    );
    const user = userEvent.setup();
    render(<OfflineStartPrompt />);
    const done = launching(hook, { ...dockerLab, id }, "random");
    await flush();
    return { offline, user, done };
  }

  it("offers an offline start when Cyber CTF can't be reached and the lab is installed, and starts it on yes", async () => {
    const { offline, user, done } = await offlineOffered("act-offline");
    expect(screen.getByText("Start Invoice portal API offline?")).toBeTruthy();
    expect(screen.getByText(/progress and evidence aren't saved/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Start offline" }));
    await act(() => done);
    expect(offline).toEqual([false, true]);
    expect(logsOf("act-offline").at(-1)).toBe("✓ Lab is running");
  });

  it("starts nothing offline when the player declines, and says why the launch failed", async () => {
    const { offline, user, done } = await offlineOffered("act-decline");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await act(() => done);
    expect(offline).toEqual([false]);
    expect(logsOf("act-decline").at(-1)).toMatch(/^✗ Cyber CTF can't be reached \(API unreachable: offline\)/);
  });

  it("never offers an offline start for a refusal or a lab that can't start offline", async () => {
    const refusals: CommandError[] = [
      { code: "other", message: "You don't have access to this lab." },
      { code: "unreachable", args: { detail: "API unreachable: offline" }, message: "Cyber CTF can't be reached (API unreachable: offline)." },
      { code: "signedOut", message: "You're signed out. Sign in again to continue." },
      // The words without the code: the app goes by the code.
      { code: "other", message: "This lab is installed here, so it can start offline, without saving progress." },
    ];
    for (const reason of refusals) {
      const offline: unknown[] = [];
      const { hook } = setup(unreachableBackend(reason, offline));
      render(<OfflineStartPrompt />);
      await act(() => hook.result.current.launch({ ...dockerLab, id: "act-refused" }, null, undefined, readyReport(), { ports: "random" }));
      expect(screen.queryByRole("alertdialog")).toBeNull();
      expect(offline).toEqual([false]);
    }
  });

  it("asks for the ports, remembers them, and starts on the lab's own ports", async () => {
    const { calls, user, done } = await portsAsked("act-docker-2");
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

  it("runs a container lab on a server", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-remote" }, "aws-1", undefined, readyReport()));
    expect(launchArgs(calls)).toEqual({ labId: "act-remote", provider: null, host: "aws-1", defaultPorts: false });
  });

  it("says a container lab is ready on the server or in the cloud account it runs on", async () => {
    // Regression: a container lab on an AWS account was announced as "running on your server".
    const { hook } = setup({}, [savedHost("pve", "proxmox"), savedHost("aws1", "aws")]);
    notified.mockClear();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-in-cloud" }, "aws1", undefined, readyReport()));
    expect(notified).toHaveBeenLastCalledWith("Lab ready", "Invoice portal API is running in your cloud account.");
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-on-pve" }, "pve", undefined, readyReport()));
    expect(notified).toHaveBeenLastCalledWith("Lab ready", "Invoice portal API is running on your server.");
  });

  it("runs a container lab inside a local VM on the chosen hypervisor", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-in-vm" }, null, { provider: "virtualbox" }, readyReport()));
    const args = must(launchArgs(calls), "the lab_launch call");
    expect(args.provider).toBe("virtualbox");
    expect(args.host).toBeNull();
    expect(rawLaunchArgs(calls)?.perMachineVm).toBe(false);
  });

  it("runs a container lab with VM recipes as one VM per machine on the chosen hypervisor", async () => {
    const { calls, hook } = setup();
    const both: Lab = { ...dockerLab, id: "act-vms", runtime: { ...must(dockerLab.runtime, "runtime"), docker: true, vm: true } };
    await act(() => hook.result.current.launch(both, null, { provider: "qemu", perMachine: true }, readyReport()));
    const args = must(launchArgs(calls), "the lab_launch call");
    // The VM form on that hypervisor, flagged for the backend; no port question (no containers
    // here).
    expect(rawLaunchArgs(calls)?.perMachineVm).toBe(true);
    expect(args.provider).toBe("qemu");
    expect(args.host).toBeNull();
  });

  it("runs a dual-form container lab on a cloud account as one VM per machine (cloud-vm)", async () => {
    const { calls, hook } = setup();
    const both: Lab = { ...dockerLab, id: "act-cloud-vms", runtime: { ...must(dockerLab.runtime, "runtime"), providers: ["aws"], docker: true, vm: true } };
    // The cloud account's one-VM-per-machine form: `perMachine` with no local hypervisor; the
    // backend reads the flag with the host and starts cloud-vm.
    await act(() => hook.result.current.launch(both, "aws1", { perMachine: true }, readyReport()));
    const args = must(launchArgs(calls), "the lab_launch call");
    expect(rawLaunchArgs(calls)?.perMachineVm).toBe(true);
    expect(args.provider).toBeNull();
    expect(args.host).toBe("aws1");
  });

  it("runs a VM lab picked here on the hypervisor the backend picks for it there", async () => {
    const { calls, hook } = setup({ lab_default_place: () => ({ host: "pve", provider: null, perMachineVm: false, hypervisor: "parallels" }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-here" }, null, undefined, readyReport()));
    expect(launchArgs(calls)).toEqual({ labId: "act-vm-here", provider: "parallels", host: null, defaultPorts: false });
  });

  it("runs a VM lab started again here on the hypervisor it ran on, without asking the backend", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-again" }, null, { provider: "libvirt" }, readyReport()));
    expect(commands(calls)).not.toContain("lab_default_place");
    expect(launchArgs(calls)).toMatchObject({ provider: "libvirt", host: null });
  });

  it("starts a VM lab with no place picked on the server the backend picks for it", async () => {
    const { calls, hook } = setup({ lab_default_place: () => ({ host: "pve", provider: null, perMachineVm: false, hypervisor: null }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-def" }, undefined, undefined, readyReport()));
    expect(calls.find((c) => c.cmd === "lab_default_place")?.args).toEqual({
      id: "act-vm-def",
      runtime: "VM",
      source: { repository: "CyberCTF/goad", commit: SHA },
      preferred: null,
    });
    expect(launchArgs(calls)).toMatchObject({ provider: null, host: "pve" });
  });

  it("runs a VM lab here on the hypervisor the backend picks, with the Settings choice for it to weigh", async () => {
    // Regression: the backend's hypervisor was dropped for a VM lab and the webview picked its
    // own (the Settings choice first). Two hypervisors ready and the Settings choice not the
    // backend's: only the backend's pick passes.
    setVmProvider("virtualbox");
    const report = readyReport();
    report.readyHypervisors = ["virtualbox", "parallels"];
    const { calls, hook } = setup({ lab_default_place: () => ({ host: null, provider: "parallels", perMachineVm: false, hypervisor: "parallels" }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-here" }, undefined, undefined, report));
    expect(calls.find((c) => c.cmd === "lab_default_place")?.args?.preferred).toBe("virtualbox");
    expect(launchArgs(calls)).toMatchObject({ provider: "parallels", host: null });
  });

  it("fails the start, rather than guess, when the default server can't be decided", async () => {
    const { calls, hook } = setup({ lab_default_place: () => rejectWith("couldn't read the lab's spec: HTTP 404") });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-nospec" }, undefined, undefined, readyReport()));
    expect(commands(calls)).not.toContain("lab_launch");
    expect(logsOf("act-vm-nospec").at(-1)).toContain("couldn't read the lab's spec");
  });

  it("starts a container lab with no place picked where the backend says it last ran", async () => {
    // Regression: Home's "start again" ran a container lab on Docker here whatever the lab last
    // ran on, where the lab page's Reset takes it back there.
    const sourced = { ...sourcedVmLab, id: "act-web-again", runtime: { ...must(sourcedVmLab.runtime), runtime: "DOCKER" as const } };
    const inVm = setup({ lab_default_place: () => ({ host: null, provider: "utm", perMachineVm: false }) });
    await act(() => inVm.hook.result.current.launch(sourced, undefined, undefined, readyReport(), { ports: "random" }));
    expect(launchArgs(inVm.calls)).toMatchObject({ provider: "utm", host: null });
    const onServer = setup({ lab_default_place: () => ({ host: "pve", provider: null, perMachineVm: false }) });
    await act(() => onServer.hook.result.current.launch({ ...sourced, id: "act-web-pve" }, undefined, undefined, readyReport(), { ports: "random" }));
    expect(launchArgs(onServer.calls)).toMatchObject({ provider: null, host: "pve" });
  });

  it("asks for no default place when a place is picked", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-picked" }, "pve", undefined, readyReport()));
    expect(commands(calls)).not.toContain("lab_default_place");
  });

  it("says no hypervisor is installed instead of starting a VM lab", async () => {
    const { calls, hook } = setup({ lab_default_place: () => ({ host: null, provider: null, perMachineVm: false, hypervisor: null }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-none" }, null, undefined, readyReport()));
    expect(launchArgs(calls)).toBeUndefined();
    expect(logsOf("act-vm-none").at(-1)).toMatch(/^✗ No hypervisor for this lab is installed on this machine \(it runs on virtualbox\)/);
  });

  it("names 'none' when the lab lists no local hypervisor", async () => {
    const { hook } = setup();
    const lab: Lab = { ...vmLab, id: "act-vm-cloudonly", runtime: { runtime: "VM", architectures: [], providers: ["aws"] } };
    await act(() => hook.result.current.launch(lab, null, undefined, readyReport()));
    expect(logsOf("act-vm-cloudonly").at(-1)).toContain("(it runs on none)");
  });

  it("explains a VM lab built for another CPU on Apple Silicon", async () => {
    const report = { ...readyReport(), arch: "aarch64" };
    const { calls, hook } = setup({ lab_default_place: () => ({ host: null, provider: null, perMachineVm: false, hypervisor: null }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-arm" }, null, undefined, report));
    expect(launchArgs(calls)).toBeUndefined();
    expect(logsOf("act-vm-arm").at(-1)).toContain("built for x86_64 and this machine is aarch64");
  });

  it("puts a failed start on the log's last line and ends the run", async () => {
    const { hook } = setup({
      lab_launch: () => rejectWith("Port 8000 is already in use"),
    });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-fail" }, null, undefined, readyReport(), { ports: "random" }));
    expect(logsOf("act-fail")).toEqual(["✗ Port 8000 is already in use"]);
    expect(getDeploySnapshot().runs["act-fail"]?.busy).toBe(false);
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
    expect(getDeploySnapshot().runs["act-vm"]?.op).toBe("stop");
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
    const hook = renderHook(() => useLabActions(refresh, []));
    let done!: Promise<unknown>;
    act(() => {
      done = hook.result.current.stop({ ...vmLab, id: "act-settle" });
    });
    await flush();
    expect(getDeploySnapshot().runs["act-settle"]?.busy).toBe(true);
    expect(hook.result.current.runs["act-settle"]?.busy).toBe(true);
    await act(async () => {
      release();
      await done;
    });
    expect(getDeploySnapshot().runs["act-settle"]?.busy).toBe(false);
  });
});

describe("useLabActions: a failure the core knows by its code", () => {
  // Settings' port choice made, so a start here doesn't stop to ask.
  beforeEach(() => {
    setPortMode("random");
  });
  afterEach(() => {
    setLocalePreference("en");
  });

  it("ends the log on the error in the player's language, with its code kept for the hints", async () => {
    setLocalePreference("fr");
    const { hook } = setup({ lab_launch: rejects({ code: "portInUse", args: { port: 8080 }, message: "Host port 8080 is already in use on this machine" }) });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-port-fr" }, null, undefined, readyReport()));
    const run = getDeploySnapshot().runs["act-port-fr"];
    expect(run?.logs.at(-1)).toMatch(/^✗ Le port 8080 de l'hôte est déjà utilisé/);
    expect(run?.failureCode).toBe("portInUse");
  });

  it("tells the shell the session is gone from a signed-out error's code", async () => {
    const heard = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, heard);
    const { hook } = setup({ lab_launch: rejects({ code: "sessionExpired", message: "Your session expired" }) });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-signed-out" }, null, undefined, readyReport()));
    expect(heard).toHaveBeenCalledTimes(1);
    window.removeEventListener(SIGNED_OUT_EVENT, heard);
  });

  it("doesn't take another failure that mentions signing out for one", async () => {
    const heard = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, heard);
    const { hook } = setup({ lab_launch: rejects({ code: "other", message: "ansible: user signed out of the DC" }) });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-not-signed-out" }, null, undefined, readyReport()));
    expect(heard).not.toHaveBeenCalled();
    window.removeEventListener(SIGNED_OUT_EVENT, heard);
  });
});
