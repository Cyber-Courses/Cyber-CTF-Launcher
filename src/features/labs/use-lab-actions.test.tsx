// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OfflineStartPrompt } from "@/features/labs/offline-start-prompt";
import { PortModePrompt } from "@/features/labs/port-mode-prompt";
import { NodeKeysPrompt } from "@/features/servers/node-keys-prompt";
import { useLabActions } from "@/features/labs/use-lab-actions";
import type { Lab } from "@/features/labs/use-labs";
import { getDeploySnapshot } from "@/features/labs/runs/deploy-runs";
import { getLastPlace, getLastRun, setLastRun } from "@/features/labs/runs/last-run";
import { SIGNED_OUT_EVENT } from "@/features/account/signed-out";
import { setLocalePreference } from "@/lib/i18n";
import type { CommandError } from "@/lib/errors";
import { setPortMode, setVmProvider, type PortMode } from "@/lib/settings";
import { commands, installTauri, rejects, type Call, type Handler } from "@/test/tauri";
import { flush } from "@/test/ui";
import { must, startInAct } from "@/components/test-support";
import { labStatus, rejectWith } from "@/features/labs/labs-test-kit";
import { CALLED_OFF } from "@/features/labs/lab-operations";
import type { LabPlacement } from "@/lib/tauri";

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

/** The placement the `lab_launch` call sent, read as where it runs: the lab, the hypervisor
 *  here, the host, the lab's own ports; and one VM per machine (`perMachineVm`, in `rawLaunchArgs`). */
function flatLaunch(c: Call | undefined) {
  if (!c) return undefined;
  const p = c.args?.placement as LabPlacement;
  return {
    labId: c.args?.labId,
    provider: p.kind === "vms" ? p.provider : null,
    host: p.kind === "server" ? p.host : p.kind === "cloud" ? p.account : null,
    defaultPorts: p.kind === "docker" && p.defaultPorts,
    perMachineVm: p.kind === "vms" || (p.kind === "cloud" && p.perMachine),
  };
}
const rawLaunchArgs = (calls: Call[]) => flatLaunch(calls.find((x) => x.cmd === "lab_launch"));
const launchArgs = (calls: Call[]) => {
  const flat = rawLaunchArgs(calls);
  if (!flat) return undefined;
  const { labId, provider, host, defaultPorts } = flat;
  return { labId, provider, host, defaultPorts };
};
/** What the launch asked the backend's rule (`lab_placement`) for (lab `id`'s, when given). */
const choiceSent = (calls: Call[], id?: string) => calls.find((x) => x.cmd === "lab_placement" && (id === undefined || x.args?.id === id))?.args?.choice;
const logsOf = (id: string) => {
  return getDeploySnapshot().runs[id]?.logs ?? [];
};

function setup(overrides: Parameters<typeof installTauri>[0] = {}) {
  const calls = installTauri(overrides);
  const refresh = vi.fn();
  const hook = renderHook(() => useLabActions(refresh));
  return { calls, refresh, hook };
}

/** Starts launching `lab` here (a dialog may then ask the player): the launch, to await once
 *  answered. */
function launching(hook: ReturnType<typeof setup>["hook"], lab: Lab, ports?: PortMode) {
  return startInAct(() => hook.result.current.launch(lab, null, undefined, { ports }));
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

/** The SSH host keys of the node of the Proxmox server "pve", never confirmed. */
const NODE_KEYS = {
  pattern: "10.0.0.5",
  keys: [{ algorithm: "ssh-ed25519", fingerprint: "SHA256:Coh2bRLeV1HsLRUtSn+G8PFvq/E3/excrj4hN1k1YEQ" }],
  state: "new",
};
/** The node offering other keys than the confirmed ones. */
const REFUSED: CommandError = {
  code: "sshHostKeyRefused",
  args: { host: "10.0.0.5" },
  message: "The SSH host key of 10.0.0.5 isn't the one confirmed for it (or none was confirmed yet), so the connection was refused.",
};

describe("useLabActions: launch", () => {
  it("starts a cloud lab on the cloud account picked, without an attack box, and says the backend's refusal without one", async () => {
    const cloudLab: Lab = { ...dockerLab, id: "act-cloud", slug: "act-cloud", runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"] } };
    const { calls, hook } = setup();
    // No account picked: the rule refuses it, and nothing is launched.
    await act(() => hook.result.current.launch(cloudLab, null, undefined));
    expect(choiceSent(calls)).toMatchObject({ host: null });
    expect(commands(calls)).not.toContain("lab_launch");
    expect(logsOf("act-cloud").at(-1)).toMatch(/^✗ .*pick one of your cloud accounts/);
    await act(() => hook.result.current.launch(cloudLab, "aws1", undefined));
    expect(rawLaunchArgs(calls)).toEqual({ labId: "act-cloud", provider: null, host: "aws1", defaultPorts: false, perMachineVm: false });
    expect(logsOf("act-cloud").at(-1)).toBe("✓ Lab is running");
  });

  it("starts a container lab here on the ports chosen in the dialog, without an attack box image", async () => {
    const { calls, refresh, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined, { ports: "default" }));
    expect(launchArgs(calls)).toEqual({ labId: "act-docker", provider: null, host: null, defaultPorts: true });
    expect(logsOf("act-docker")).toEqual(["==> api: Creating", "✓ Lab ready", "✓ Lab is running"]);
    expect(refresh).toHaveBeenCalledWith(dockerLab, { fresh: true });
    expect(getDeploySnapshot().runs["act-docker"]?.busy).toBe(false);
    expect(getLastRun("act-docker")).not.toBeNull();
  });

  it("uses the port mode remembered in Settings without asking", async () => {
    setPortMode("random");
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch(dockerLab, null, undefined));
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
      await act(() => hook.result.current.launch({ ...dockerLab, id: "act-refused" }, null, undefined, { ports: "random" }));
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
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-remote" }, "aws-1", undefined));
    expect(launchArgs(calls)).toEqual({ labId: "act-remote", provider: null, host: "aws-1", defaultPorts: false });
  });

  describe("on a Proxmox server reached through its node", () => {
    /** A launch of the VM lab as `id` on the server "pve", whose node's keys `keys` answers. */
    async function onPve(id: string, keys: Handler) {
      const { calls, hook } = setup({ server_node_host_keys: keys });
      const user = userEvent.setup();
      render(<NodeKeysPrompt />);
      const done = startInAct(() => hook.result.current.launch({ ...vmLab, id }, "pve", undefined));
      await flush();
      return { calls, user, done };
    }

    it("starts at once when the node's keys are the confirmed ones", async () => {
      const { calls, done } = await onPve("act-pve-known", () => null);
      await act(() => done);
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(commands(calls)).not.toContain("server_trust_node_host_keys");
      expect(launchArgs(calls)?.host).toBe("pve");
    });

    it("asks to confirm keys never confirmed, as the setup does, then starts", async () => {
      const { calls, user, done } = await onPve("act-pve-new", () => NODE_KEYS);
      const dialog = screen.getByRole("dialog", { name: "Confirm this server's identity" });
      expect(dialog.textContent).toContain(NODE_KEYS.keys[0]?.fingerprint);
      expect(launchArgs(calls)).toBeUndefined();
      await user.click(screen.getByRole("button", { name: "They match: trust this server" }));
      await act(() => done);
      expect(calls.find((c) => c.cmd === "server_trust_node_host_keys")?.args).toEqual({ id: "pve", fingerprints: [NODE_KEYS.keys[0]?.fingerprint] });
      expect(commands(calls).indexOf("server_trust_node_host_keys")).toBeLessThan(commands(calls).indexOf("lab_launch"));
      expect(logsOf("act-pve-new").at(-1)).toBe("✓ Lab is running");
    });

    it("starts nothing and fails nothing when the player declines", async () => {
      const { calls, user, done } = await onPve("act-pve-declined", () => NODE_KEYS);
      await user.click(screen.getByRole("button", { name: "Cancel" }));
      await act(() => done);
      expect(commands(calls)).not.toContain("server_trust_node_host_keys");
      expect(launchArgs(calls)).toBeUndefined();
      expect(getDeploySnapshot().runs["act-pve-declined"]).toBeUndefined();
    });

    it("refuses a node offering other keys than the confirmed ones, in the host key words", async () => {
      const { calls, done } = await onPve("act-pve-changed", rejects(REFUSED));
      await act(() => done);
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(launchArgs(calls)).toBeUndefined();
      expect(logsOf("act-pve-changed").at(-1)).toMatch(/^✗ The SSH host key of 10\.0\.0\.5 isn't the one confirmed for it/);
    });
  });

  it("says a lab is ready on the server or in the cloud account its placement names", async () => {
    // Regression: a container lab on an AWS account was announced as "running on your server".
    const { hook } = setup();
    notified.mockClear();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-in-cloud" }, "aws1", undefined));
    expect(notified).toHaveBeenLastCalledWith("Lab ready", "Invoice portal API is running in your cloud account.");
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-on-pve" }, "pve", undefined));
    expect(notified).toHaveBeenLastCalledWith("Lab ready", "GOAD Light is running on your server.");
  });

  it("runs a container lab given a hypervisor here as one VM per machine there, never its containers in a VM", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-in-vm" }, null, { provider: "virtualbox" }));
    const args = must(launchArgs(calls), "the lab_launch call");
    expect(args.provider).toBe("virtualbox");
    expect(args.host).toBeNull();
    expect(rawLaunchArgs(calls)?.perMachineVm).toBe(true);
  });

  it("runs a container lab with VM recipes as one VM per machine on the chosen hypervisor", async () => {
    const { calls, hook } = setup();
    const both: Lab = { ...dockerLab, id: "act-vms", runtime: { ...must(dockerLab.runtime, "runtime"), docker: true, vm: true } };
    await act(() => hook.result.current.launch(both, null, { provider: "qemu", perMachine: true }));
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
    await act(() => hook.result.current.launch(both, "aws1", { perMachine: true }));
    const args = must(launchArgs(calls), "the lab_launch call");
    expect(rawLaunchArgs(calls)?.perMachineVm).toBe(true);
    expect(args.provider).toBeNull();
    expect(args.host).toBe("aws1");
  });

  it("runs a VM lab picked here where the backend's rule places it, sending that placement as it is", async () => {
    const { calls, hook } = setup({ lab_placement: () => ({ kind: "vms", provider: "parallels" }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-here" }, null, undefined));
    // This machine picked, its hypervisor left to the rule (the saved one, else the first ready).
    expect(calls.find((c) => c.cmd === "lab_placement")?.args).toEqual({
      id: "act-vm-here",
      runtime: "VM",
      source: { repository: "CyberCTF/goad", commit: SHA },
      choice: { here: true, host: null, provider: null, perMachineVm: false, engine: null, defaultPorts: null },
    });
    expect(rawLaunchArgs(calls)).toEqual({ labId: "act-vm-here", provider: "parallels", host: null, defaultPorts: false, perMachineVm: true });
    expect(calls.find((c) => c.cmd === "lab_launch")?.args?.placement).toEqual({ kind: "vms", provider: "parallels" });
  });

  it("runs a VM lab started again here on the hypervisor it ran on", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-again" }, null, { provider: "libvirt" }));
    expect(choiceSent(calls)).toMatchObject({ here: true, provider: "libvirt" });
    expect(launchArgs(calls)).toMatchObject({ provider: "libvirt", host: null });
  });

  it("starts a lab with nothing picked where the saved preference places it", async () => {
    const { calls, hook } = setup({ lab_placement: () => ({ kind: "server", host: "pve" }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-def" }, undefined, undefined));
    expect(choiceSent(calls)).toEqual({ here: false, host: null, provider: null, perMachineVm: false, engine: null, defaultPorts: null });
    expect(launchArgs(calls)).toMatchObject({ provider: null, host: "pve" });
  });

  describe("started again with nothing newly picked", () => {
    it("goes back where it was last started: that placement sent as the player's choice", async () => {
      setLastRun("act-again-vms", { kind: "vms", provider: "libvirt" });
      setLastRun("act-again-pve", { kind: "server", host: "pve" });
      const { calls, hook } = setup({
        lab_placement: (a) => (a?.id === "act-again-pve" ? { kind: "server", host: "pve" } : { kind: "vms", provider: "libvirt" }),
      });
      await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-again-vms" }, undefined, undefined));
      await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-again-pve" }, undefined, undefined));
      expect(choiceSent(calls, "act-again-vms")).toEqual({
        here: true,
        host: null,
        provider: "libvirt",
        perMachineVm: false,
        engine: null,
        defaultPorts: null,
      });
      expect(choiceSent(calls, "act-again-pve")).toEqual({ here: false, host: "pve", provider: null, perMachineVm: false, engine: null, defaultPorts: null });
      const launched = calls.filter((c) => c.cmd === "lab_launch").map((c) => c.args?.placement);
      expect(launched).toEqual([
        { kind: "vms", provider: "libvirt" },
        { kind: "server", host: "pve" },
      ]);
    });

    it("keeps a container lab's form, engine and ports, without asking the ports again", async () => {
      setLastRun("act-again-docker", { kind: "docker", engine: "colima", defaultPorts: true });
      setLastRun("act-again-pervm", { kind: "vms", provider: "qemu" });
      setLastRun("act-again-cloud", { kind: "cloud", account: "aws1", perMachine: true });
      const { calls, hook } = setup();
      for (const id of ["act-again-docker", "act-again-pervm", "act-again-cloud"]) await act(() => hook.result.current.launch({ ...dockerLab, id }));
      expect(choiceSent(calls, "act-again-docker")).toEqual({
        here: true,
        host: null,
        provider: null,
        perMachineVm: false,
        engine: "colima",
        defaultPorts: true,
      });
      expect(choiceSent(calls, "act-again-pervm")).toMatchObject({ here: true, provider: "qemu", perMachineVm: true });
      expect(choiceSent(calls, "act-again-cloud")).toMatchObject({ here: false, host: "aws1", perMachineVm: true });
      expect(calls.find((c) => c.cmd === "lab_launch" && c.args?.labId === "act-again-docker")?.args?.placement).toEqual({
        kind: "docker",
        engine: "colima",
        defaultPorts: true,
      });
    });

    it("is refused, launching nothing, when the rule says it can't run there now", async () => {
      setLastRun("act-again-gone", { kind: "vms", provider: "parallels" });
      const { calls, hook } = setup({ lab_placement: () => rejectWith({ code: "noHypervisor", message: "No hypervisor is ready" }) });
      await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-again-gone" }, undefined, undefined));
      expect(choiceSent(calls)).toMatchObject({ here: true, provider: "parallels" });
      expect(commands(calls)).not.toContain("lab_launch");
    });

    it("notes where each launch went; a pick made anew still wins over it", async () => {
      setLastRun("act-again-pick", { kind: "server", host: "pve" });
      const { calls, hook } = setup({ lab_placement: () => ({ kind: "vms", provider: "virtualbox" }) });
      await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-again-pick" }, null, undefined));
      expect(choiceSent(calls)).toMatchObject({ here: true, host: null });
      expect(getLastPlace("act-again-pick")).toEqual({ kind: "vms", provider: "virtualbox" });
    });
  });

  it("asks the rule for a lab with no source known, which then reads the catalogue's", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...vmLab, id: "act-vm-unsourced" }, "pve", undefined));
    expect(calls.find((c) => c.cmd === "lab_placement")?.args?.source).toBeNull();
    expect(launchArgs(calls)).toMatchObject({ host: "pve" });
  });

  it("fails the start, rather than guess, when the rule can't place the lab", async () => {
    const { calls, hook } = setup({ lab_placement: () => rejectWith("couldn't read the lab's spec: HTTP 404") });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-nospec" }, undefined, undefined));
    expect(commands(calls)).not.toContain("lab_launch");
    expect(logsOf("act-vm-nospec").at(-1)).toContain("couldn't read the lab's spec");
  });

  it("sends a container lab's engine and ports picked here for the rule to keep", async () => {
    const { calls, hook } = setup();
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-engine" }, null, undefined, { ports: "default", engine: "colima" }));
    expect(choiceSent(calls)).toMatchObject({ here: true, engine: "colima", defaultPorts: true });
    expect(calls.find((c) => c.cmd === "lab_launch")?.args?.placement).toEqual({ kind: "docker", engine: "colima", defaultPorts: true });
  });

  it("says the rule's refusal of a VM lab with no hypervisor here, and launches nothing", async () => {
    const { calls, hook } = setup({ lab_placement: rejects({ code: "noHypervisor", message: "No hypervisor ready on this machine runs this lab." }) });
    await act(() => hook.result.current.launch({ ...sourcedVmLab, id: "act-vm-none" }, null, undefined));
    expect(commands(calls)).not.toContain("lab_launch");
    expect(logsOf("act-vm-none").at(-1)).toMatch(/^✗ No hypervisor ready on this machine runs this lab\. Install one from the Machine page/);
  });

  it("puts a failed start on the log's last line and ends the run", async () => {
    const { hook } = setup({
      lab_launch: () => rejectWith("Port 8000 is already in use"),
    });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-fail" }, null, undefined, { ports: "random" }));
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
    const hook = renderHook(() => useLabActions(refresh));
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

describe("useLabActions: stop, resume and setup again on a Proxmox server reached through its node", () => {
  /** What each operation runs, and its log's last line once it went through. */
  const OPS = {
    stop: { cmd: "lab_stop", done: "✓ Lab stopped" },
    resume: { cmd: "lab_resume", done: "✓ Lab is running" },
    provision: { cmd: "lab_provision", done: "✓ DC01 provisioned" },
  } as const;
  type Op = keyof typeof OPS;
  /** Runs `op` on `lab` (setup again on its machine DC01). */
  const run = (hook: ReturnType<typeof setup>["hook"], op: Op, lab: Lab) =>
    op === "provision" ? hook.result.current.provision(lab, "DC01") : hook.result.current[op](lab);

  /** `op` on the VM lab as `id`, running on the server "pve" as its status says, whose node's
   *  keys `keys` answers: the hook's answer, to await once asked. */
  async function onPve(op: Op, id: string, keys: Handler) {
    const { calls, hook, refresh } = setup({
      server_node_host_keys: keys,
      lab_status: () => labStatus({ running: true, host: "pve", hostId: "pve", place: "server", provider: "proxmox" }),
    });
    const user = userEvent.setup();
    render(<NodeKeysPrompt />);
    const done = startInAct(() => run(hook, op, { ...vmLab, id }));
    await flush();
    return { calls, user, done, refresh };
  }

  /** `op` goes through at once: the node's keys are the confirmed ones, nothing asked. */
  async function goesAtOnce(op: Op, id: string) {
    const { calls, done } = await onPve(op, id, () => null);
    await act(() => done);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls.find((c) => c.cmd === "server_node_host_keys")?.args).toEqual({ id: "pve" });
    expect(commands(calls)).not.toContain("server_trust_node_host_keys");
    expect(logsOf(id).at(-1)).toBe(OPS[op].done);
  }

  /** `op` asks the setup's question for keys never confirmed, then, confirmed, goes through. */
  async function asksThenGoes(op: Op, id: string) {
    const { calls, user, done } = await onPve(op, id, () => NODE_KEYS);
    expect(screen.getByRole("dialog", { name: "Confirm this server's identity" }).textContent).toContain(NODE_KEYS.keys[0]?.fingerprint);
    expect(commands(calls)).not.toContain(OPS[op].cmd);
    await user.click(screen.getByRole("button", { name: "They match: trust this server" }));
    await act(() => done);
    expect(calls.find((c) => c.cmd === "server_trust_node_host_keys")?.args).toEqual({ id: "pve", fingerprints: [NODE_KEYS.keys[0]?.fingerprint] });
    expect(commands(calls).indexOf("server_trust_node_host_keys")).toBeLessThan(commands(calls).indexOf(OPS[op].cmd));
    expect(logsOf(id).at(-1)).toBe(OPS[op].done);
  }

  /** `op` asked, the player declines: called off (Reset reads it: no start after a stop called
   *  off), nothing run, no run left failed, the lab's status read again as it is. */
  async function declined(op: Op, id: string) {
    const { calls, user, done, refresh } = await onPve(op, id, () => NODE_KEYS);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(await act(() => done)).toBe(CALLED_OFF);
    expect(commands(calls)).not.toContain("server_trust_node_host_keys");
    expect(commands(calls)).not.toContain(OPS[op].cmd);
    expect(getDeploySnapshot().runs[id]).toBeUndefined();
    expect(refresh).toHaveBeenCalledWith(expect.objectContaining({ id }), { fresh: true });
  }

  /** `op` through a node offering other keys than the confirmed ones: refused, in the host key
   *  words, nothing asked and nothing run. */
  async function refused(op: Op, id: string) {
    const { calls, done } = await onPve(op, id, rejects(REFUSED));
    await act(() => done);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(commands(calls)).not.toContain(OPS[op].cmd);
    expect(logsOf(id).at(-1)).toMatch(/^✗ The SSH host key of 10\.0\.0\.5 isn't the one confirmed for it/);
  }

  it("stops at once when the node's keys are the confirmed ones", () => goesAtOnce("stop", "act-stop-known"));
  it("asks to confirm keys never confirmed before a stop, as the setup does, then stops", () => asksThenGoes("stop", "act-stop-new"));
  it("leaves the lab as it was when the player declines a stop's question: not stopped, not failed", () => declined("stop", "act-stop-declined"));
  it("refuses a stop through a node offering other keys than the confirmed ones, in the host key words", () => refused("stop", "act-stop-changed"));
  it("resumes at once when the node's keys are the confirmed ones", () => goesAtOnce("resume", "act-resume-known"));
  it("asks to confirm keys never confirmed before a resume, as the setup does, then resumes", () => asksThenGoes("resume", "act-resume-new"));
  it("resumes nothing and fails nothing when the player declines", () => declined("resume", "act-resume-declined"));
  it("refuses a resume through a node offering other keys than the confirmed ones, in the host key words", () => refused("resume", "act-resume-changed"));
  it("sets a machine up again at once when the node's keys are the confirmed ones", () => goesAtOnce("provision", "act-prov-known"));
  it("asks to confirm keys never confirmed before setting a machine up again, as the setup does, then runs it", () =>
    asksThenGoes("provision", "act-prov-new"));
  it("sets nothing up again and fails nothing when the player declines", () => declined("provision", "act-prov-declined"));
  it("refuses a setup again through a node offering other keys than the confirmed ones, in the host key words", () => refused("provision", "act-prov-changed"));

  it("asks nothing of a lab running here or on a cloud account", async () => {
    const inCloud = labStatus({ running: true, host: "aws1", hostId: "aws1", place: "cloud", provider: "aws" });
    const { calls, hook } = setup({ lab_status: (a) => (a?.id === "act-stop-cloud" ? inCloud : labStatus({ running: true, place: "local_vm" })) });
    await act(() => hook.result.current.stop({ ...vmLab, id: "act-stop-cloud" }));
    await act(() => hook.result.current.resume({ ...vmLab, id: "act-resume-here" }));
    await act(() => hook.result.current.provision({ ...vmLab, id: "act-prov-here" }, null));
    expect(commands(calls)).not.toContain("server_node_host_keys");
    expect(commands(calls)).toEqual(expect.arrayContaining(["lab_stop", "lab_resume", "lab_provision"]));
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
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-port-fr" }, null, undefined));
    const run = getDeploySnapshot().runs["act-port-fr"];
    expect(run?.logs.at(-1)).toMatch(/^✗ Le port 8080 de l'hôte est déjà utilisé/);
    expect(run?.failureCode).toBe("portInUse");
  });

  it("tells the shell the session is gone from a signed-out error's code", async () => {
    const heard = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, heard);
    const { hook } = setup({ lab_launch: rejects({ code: "sessionExpired", message: "Your session expired" }) });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-signed-out" }, null, undefined));
    expect(heard).toHaveBeenCalledTimes(1);
    window.removeEventListener(SIGNED_OUT_EVENT, heard);
  });

  it("doesn't take another failure that mentions signing out for one", async () => {
    const heard = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, heard);
    const { hook } = setup({ lab_launch: rejects({ code: "other", message: "ansible: user signed out of the DC" }) });
    await act(() => hook.result.current.launch({ ...dockerLab, id: "act-not-signed-out" }, null, undefined));
    expect(heard).not.toHaveBeenCalled();
    window.removeEventListener(SIGNED_OUT_EVENT, heard);
  });
});
