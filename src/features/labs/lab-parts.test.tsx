// @vitest-environment jsdom
import type { ComponentProps } from "react";
import { act, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HealthBanner, useLabCheck } from "@/features/labs/lab-health";
import { LabRow } from "@/features/labs/lab-row";
import { askPortMode, PortModePrompt } from "@/features/labs/port-mode-prompt";
import { RunOnDialog, RunOnPicker, type Platform, type RunTarget } from "@/features/labs/run-on";
import type { Lab } from "@/features/labs/use-labs";
import { getPortMode, setPortMode } from "@/lib/settings";
import type { LabCheck, LabStatus, ServerHost } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";
import { must, startInAct } from "@/components/test-support";
import { invoiceRowLab, rejectWith } from "@/features/labs/labs-test-kit";

afterEach(() => {
  setPortMode(null);
});

// ---------------------------------------------------------------------------------------------
describe("RunOnPicker", () => {
  const host = (p: Partial<ServerHost>): ServerHost => ({ id: "h", name: "h", provider: "vmware_esxi", host: "10.0.0.2", ...p }) as ServerHost;
  const hosts = [
    host({ id: "esx", name: "esx-lab", provider: "vmware_esxi", host: "10.0.0.2" }),
    host({ id: "pve", name: "homelab-pve", provider: "proxmox", host: "192.168.1.20" }),
    host({ id: "aws", name: "AWS sandbox", provider: "aws", host: "eu-west-3" }),
    host({ id: "down", name: "old-esx", provider: "vmware_esxi", host: "10.0.0.9" }),
  ];

  function renderPicker(p: Partial<Parameters<typeof RunOnPicker>[0]> = {}) {
    const onPickPlatform = vi.fn();
    const onPickForm = vi.fn();
    const onPickHypervisor = vi.fn();
    const onPickEngine = vi.fn();
    render(
      <RunOnPicker
        title="GOAD Light"
        hosts={hosts}
        hostOk={(h) => h.provider !== "proxmox"}
        localNote="On your system, as containers"
        dockerRunning
        value={{ kind: "local" }}
        disabled={false}
        onPickPlatform={onPickPlatform}
        onPickForm={onPickForm}
        onPickHypervisor={onPickHypervisor}
        onPickEngine={onPickEngine}
        {...p}
      />,
    );
    return { onPickPlatform, onPickForm, onPickHypervisor, onPickEngine };
  }

  it("lists this machine, servers and cloud accounts as platforms, with each host's live check", async () => {
    const calls = installTauri({
      server_test: (a) => {
        if (a?.id === "down") throw new Error("connection refused");
        return a?.id === "aws"
          ? { ok: false, reachable: true, authenticated: false, latencyMs: 40, message: "Credentials expired", checks: [] }
          : { ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] };
      },
    });
    const user = userEvent.setup();
    const { onPickPlatform } = renderPicker();
    expect(screen.getByText("Where should it run?")).toBeTruthy();
    expect(screen.getAllByText("Checking…").length).toBe(3);
    await flush();
    // Only the hosts the lab supports are tested.
    expect(calls.filter((c) => c.cmd === "server_test").map((c) => c.args?.id)).toEqual(["esx", "pve", "aws", "down"]);
    expect(screen.getByText("Online")).toBeTruthy();
    expect(screen.getByText("Needs attention")).toBeTruthy();
    expect(screen.getByText("Unreachable")).toBeTruthy();
    expect(screen.getByText("Running")).toBeTruthy();
    expect(screen.getByText("Servers")).toBeTruthy();
    expect(screen.getByText("Cloud")).toBeTruthy();
    expect(screen.getByText(/billed to you · eu-west-3/)).toBeTruthy();
    // Proxmox is shown, disabled, saying why.
    const pve = must(screen.getByText("homelab-pve").closest<HTMLElement>("[role=radio]"), "Proxmox's option");
    expect(pve.getAttribute("aria-disabled") === "true" || pve.hasAttribute("disabled")).toBe(true);
    expect(within(pve).getByText("Not supported by this lab")).toBeTruthy();
    // Picking a platform (a host runs one way): the platform is chosen, no form step.
    await user.click(screen.getByText("esx-lab"));
    expect(onPickPlatform).toHaveBeenCalledWith({ kind: "host", id: "esx" } satisfies Platform);
  });

  it("shows Container and VM as two rows for a VM-capable container lab", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickPlatform } = renderPicker({ hosts: [], dockerRunning: false, vmForms: [{ kind: "local-vm", provider: "virtualbox" }] });
    // Two rows under This machine, each its own kind of run here: no inline switches, no misleading Docker logo.
    expect(screen.getByText("Container")).toBeTruthy();
    expect(screen.getByText("Engine stopped")).toBeTruthy();
    expect(screen.getByText("VM")).toBeTruthy();
    expect(screen.getByText("Containers in one VM")).toBeTruthy();
    await user.click(screen.getByText("Container"));
    expect(onPickPlatform).toHaveBeenLastCalledWith({ kind: "containers" } satisfies Platform);
    await user.click(screen.getByText("VM"));
    expect(onPickPlatform).toHaveBeenLastCalledWith({ kind: "vm" } satisfies Platform);
    expect(screen.queryByText("Servers")).toBeNull();
  });

  it("names the container engine on the Container row instead of showing Docker", () => {
    installTauri();
    renderPicker({ hosts: [], dockerRunning: true, engine: "orbstack" });
    expect(screen.getByText("Container")).toBeTruthy();
    // The engine it will run on, named; the row carries no Docker logo.
    expect(screen.getByText("OrbStack")).toBeTruthy();
    expect(screen.queryByAltText("Docker")).toBeNull();
  });

  it("shows the dual-form lab's VM row leading to the form step, with no inline switch", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickPlatform } = renderPicker({
      hosts: [],
      dockerRunning: true,
      vmForms: [
        { kind: "local-vm", provider: "qemu" },
        { kind: "local-vms", provider: "qemu" },
      ],
    });
    // Two VM forms: the row says so and carries no inline switch (the forms are picked in step 2).
    expect(screen.getByText("VM")).toBeTruthy();
    expect(screen.getByText("In one VM, or one per machine")).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Hypervisor" })).toBeNull();
    await user.click(screen.getByText("VM"));
    expect(onPickPlatform).toHaveBeenLastCalledWith({ kind: "vm" } satisfies Platform);
  });

  it("offers the two VM forms as full rows (no switches) on the form step", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickForm } = renderPicker({
      step: "form",
      hosts: [],
      vmForms: [
        { kind: "local-vm", provider: "qemu" },
        { kind: "local-vms", provider: "qemu" },
      ],
    });
    expect(screen.getByText("How should it run?")).toBeTruthy();
    // No Containers on the form step: it is a leaf on step 1. Only the VM forms here, as full rows.
    expect(screen.queryByText("Container")).toBeNull();
    expect(screen.getByText("In one VM")).toBeTruthy();
    expect(screen.getByText("One VM per machine")).toBeTruthy();
    expect(screen.queryByRole("radiogroup", { name: "Hypervisor" })).toBeNull();
    await user.click(screen.getByText("One VM per machine"));
    expect(onPickForm).toHaveBeenLastCalledWith({ kind: "local-vms", provider: "qemu" } satisfies RunTarget);
  });

  it("offers a cloud account's two forms on the form step, under its name", async () => {
    installTauri();
    const user = userEvent.setup();
    const account = host({ id: "aws1", name: "AWS sandbox", provider: "aws", host: "eu-west-3" });
    const { onPickForm } = renderPicker({
      step: "form",
      hosts: [],
      value: { kind: "host", id: "aws1" },
      formAccount: account,
      formTargets: [
        { kind: "host", id: "aws1" },
        { kind: "host", id: "aws1", perMachine: true },
      ],
    });
    expect(screen.getByText("How should it run?")).toBeTruthy();
    // The account's own name labels its forms; the two rows are one VM (cloud-docker) and one VM
    // per machine (cloud-vm), no hypervisor to pick.
    expect(screen.getByText("AWS sandbox")).toBeTruthy();
    expect(screen.getByText("One VM")).toBeTruthy();
    expect(screen.getByText("Your containers on one cloud VM")).toBeTruthy();
    expect(screen.getByText("One VM per machine")).toBeTruthy();
    expect(screen.queryByText("Container")).toBeNull();
    await user.click(screen.getByText("One VM per machine"));
    expect(onPickForm).toHaveBeenLastCalledWith({ kind: "host", id: "aws1", perMachine: true } satisfies RunTarget);
  });

  it("lists the hypervisors as full rows on the hypervisor step, the default marked and preselected", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickHypervisor } = renderPicker({
      step: "hypervisor",
      hosts: [],
      value: { kind: "local-vm", provider: "utm" },
      hypervisors: ["utm", "qemu"],
      defaultHypervisor: "utm",
    });
    expect(screen.getByText("Which hypervisor?")).toBeTruthy();
    const utm = must(screen.getByText("UTM").closest<HTMLElement>("[role=radio]"), "UTM's row");
    expect(utm.getAttribute("aria-checked")).toBe("true");
    expect(within(utm).getByText("Default")).toBeTruthy();
    const qemu = must(screen.getByText("QEMU").closest<HTMLElement>("[role=radio]"), "QEMU's row");
    expect(within(qemu).getByText("Ready")).toBeTruthy();
    await user.click(screen.getByText("QEMU"));
    expect(onPickHypervisor).toHaveBeenCalledWith("qemu");
  });

  it("lists the engines as full rows on the engine step, the current one marked Default and preselected", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickEngine } = renderPicker({
      step: "engine",
      hosts: [],
      engine: "docker-desktop",
      engines: ["docker-desktop", "orbstack"],
      defaultEngine: "docker-desktop",
    });
    expect(screen.getByText("Which engine?")).toBeTruthy();
    const dd = must(screen.getByText("Docker Desktop").closest<HTMLElement>("[role=radio]"), "Docker Desktop's row");
    expect(dd.getAttribute("aria-checked")).toBe("true");
    expect(within(dd).getByText("Default")).toBeTruthy();
    await user.click(screen.getByText("OrbStack"));
    expect(onPickEngine).toHaveBeenCalledWith("orbstack");
  });

  it("shows no VM row for a container lab with no ready hypervisor", () => {
    installTauri();
    const { onPickPlatform } = renderPicker({ hosts: [], dockerRunning: true });
    expect(screen.getByText("Container")).toBeTruthy();
    expect(screen.queryByText("VM")).toBeNull();
    expect(onPickPlatform).not.toHaveBeenCalled();
  });

  it("badges a cloud with no real-run evidence as Beta, and leaves a proven one (AWS) unbadged", async () => {
    // The badge is data-driven from Isoloom's tier evidence (the dev mock mirrors it): Azure is
    // validated-only, AWS has real runs.
    installTauri();
    renderPicker({
      hosts: [
        host({ id: "az", name: "Azure sub", provider: "azure", host: "westeurope" }),
        host({ id: "aws", name: "AWS sandbox", provider: "aws", host: "eu-west-3" }),
      ],
      hostOk: () => true,
    });
    await flush();
    const az = must(screen.getByText("Azure sub").closest<HTMLElement>("[role=radio]"), "Azure's option");
    expect(within(az).getByText("Beta")).toBeTruthy();
    const aws = must(screen.getByText("AWS sandbox").closest<HTMLElement>("[role=radio]"), "AWS's option");
    expect(within(aws).queryByText("Beta")).toBeNull();
    expect(screen.queryByText("Experimental")).toBeNull();
  });

  it("shows a VM lab only the In a VM row, and nothing selectable while busy", async () => {
    installTauri();
    const user = userEvent.setup();
    const { onPickPlatform } = renderPicker({ hosts: [], offerContainers: false, dockerRunning: null, disabled: true });
    // A VM lab runs here only as VMs: one row, no Containers and no engine status.
    expect(screen.queryByText("Container")).toBeNull();
    expect(screen.queryByText("Running")).toBeNull();
    expect(screen.queryByText("Engine stopped")).toBeNull();
    await user.click(screen.getByText("VM"));
    expect(onPickPlatform).not.toHaveBeenCalled();
  });
});

describe("RunOnDialog", () => {
  it("closes on Escape and on a click outside, not on a click inside", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(
      <RunOnDialog onClose={onClose} footer={<button>Go</button>}>
        <p>inside</p>
      </RunOnDialog>,
    );
    expect(screen.getByRole("dialog", { name: "Where should the lab run?" })).toBeTruthy();
    await user.click(screen.getByText("inside"));
    expect(onClose).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.pointer({ keys: "[MouseLeft]", target: must(screen.getByRole("dialog").parentElement, "the backdrop") });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

// ---------------------------------------------------------------------------------------------
describe("PortModePrompt", () => {
  it("answers the recommended random ports by default", async () => {
    const user = userEvent.setup();
    render(<PortModePrompt />);
    const answer = startInAct(() => askPortMode("Blind orders"));
    expect(screen.getByText("Which ports should Blind orders use?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Start lab" }));
    await expect(answer).resolves.toBe("random");
    // Not remembered unless asked.
    expect(getPortMode()).toBeNull();
  });

  it("cancels on Escape, on the backdrop, and when a second start asks again", async () => {
    const user = userEvent.setup();
    render(<PortModePrompt />);
    const first = startInAct(() => askPortMode("A"));
    const second = startInAct(() => askPortMode("B"));
    await expect(first).resolves.toBeNull();
    expect(screen.getByText("Which ports should B use?")).toBeTruthy();
    await user.keyboard("{Escape}");
    await expect(second).resolves.toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    const third = startInAct(() => askPortMode("C"));
    await user.pointer({ keys: "[MouseLeft]", target: must(screen.getByRole("dialog").parentElement, "the backdrop") });
    await expect(third).resolves.toBeNull();
  });
});

// ---------------------------------------------------------------------------------------------
describe("useLabCheck and HealthBanner", () => {
  const solvable: LabCheck = { available: true, ok: true, output: "", results: [] };

  it("checks once per incident whether the lab is still solvable", async () => {
    const calls = installTauri({ lab_check: () => solvable });
    const { result, rerender } = renderHook((p: { downCount: number; running: boolean }) => useLabCheck("lab-x", { ...p, enabled: true }), {
      initialProps: { downCount: 0, running: true },
    });
    expect(result.current[0]).toBeNull();
    rerender({ downCount: 1, running: true });
    expect(result.current[0]).toBe("checking");
    await flush();
    expect(result.current[0]).toEqual(solvable);
    expect(calls.find((c) => c.cmd === "lab_check")?.args).toEqual({ id: "lab-x", runtime: "DOCKER" });
    // The same incident: not checked again; cleared by hand.
    rerender({ downCount: 1, running: true });
    act(() => {
      result.current[1]();
    });
    expect(result.current[0]).toBeNull();
    // A new incident after recovery is checked again.
    rerender({ downCount: 0, running: true });
    rerender({ downCount: 1, running: true });
    await flush();
    expect(calls.filter((c) => c.cmd === "lab_check")).toHaveLength(2);
  });

  it("reports a check that errors as not solvable", async () => {
    installTauri({
      lab_check: () => rejectWith("docker unreachable"),
    });
    const { result } = renderHook(() => useLabCheck("lab-y", { running: true, downCount: 1, enabled: true }));
    await flush();
    expect(result.current[0]).toMatchObject({ ok: false, output: "docker unreachable" });
  });

  it("doesn't check a VM lab", async () => {
    const calls = installTauri();
    renderHook(() => useLabCheck("lab-z", { running: true, downCount: 2, enabled: false }));
    await flush();
    expect(commands(calls)).not.toContain("lab_check");
  });

  // Regression: a second machine going down while the check was in flight cancelled it and the
  // banner said "Checking…" forever.
  it("settles the check when another machine goes down while it runs", async () => {
    let answer!: (c: LabCheck) => void;
    installTauri({ lab_check: () => new Promise<LabCheck>((r) => (answer = r)) });
    const { result, rerender } = renderHook((p: { downCount: number }) => useLabCheck("lab-race", { running: true, downCount: p.downCount, enabled: true }), {
      initialProps: { downCount: 1 },
    });
    expect(result.current[0]).toBe("checking");
    rerender({ downCount: 2 });
    await flush();
    act(() => {
      answer({ available: true, ok: false, output: "", results: [] });
    });
    await flush();
    expect(result.current[0]).not.toBe("checking");
  });

  it("names the machines down, the failed checks, and offers reset and start", async () => {
    const user = userEvent.setup();
    const onReset = vi.fn();
    const onResume = vi.fn();
    const failing: LabCheck = {
      available: true,
      ok: false,
      output: "",
      results: [
        { name: "api", from: "attacker", ok: false, reason: "connection refused" },
        { name: "db", from: "attacker", ok: false, reason: "" },
        { name: "web", from: "attacker", ok: true, reason: "" },
      ],
    };
    const { rerender } = render(<HealthBanner down={["api", "db"]} check={failing} busy={false} resetting={false} onReset={onReset} onResume={onResume} />);
    expect(screen.getByText("api, db are down. The lab may not work.")).toBeTruthy();
    expect(screen.getByText("It can no longer be solved. Reset it to get a clean lab.")).toBeTruthy();
    expect(screen.getByText("api: connection refused")).toBeTruthy();
    expect(screen.getByText("db")).toBeTruthy();
    expect(screen.queryByText("web")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Start them/ }));
    await user.click(screen.getByRole("button", { name: /Reset lab/ }));
    expect(onResume).toHaveBeenCalled();
    expect(onReset).toHaveBeenCalled();
    rerender(<HealthBanner down={["api"]} check="checking" busy={false} resetting onReset={onReset} />);
    expect(screen.getByText("api is down. The lab may not work.")).toBeTruthy();
    expect(screen.getByText("Checking whether it's still solvable…")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Resetting…/ }).hasAttribute("disabled")).toBe(true);
    expect(screen.queryByRole("button", { name: /Start them/ })).toBeNull();
    rerender(<HealthBanner down={["api"]} check={{ available: true, ok: true, output: "", results: [] }} busy={false} resetting={false} onReset={onReset} />);
    expect(screen.getByText("Still solvable.")).toBeTruthy();
    rerender(<HealthBanner down={["api"]} check={{ available: false, ok: false, output: "", results: [] }} busy={false} resetting={false} onReset={onReset} />);
    expect(screen.queryByText("Still solvable.")).toBeNull();
    expect(screen.queryByText(/no longer be solved/)).toBeNull();
  });
});

// ---------------------------------------------------------------------------------------------
describe("LabRow", () => {
  const lab = invoiceRowLab;
  const vmLab: Lab = { ...lab, id: "lab-goad", title: "GOAD Light", runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox"] } };
  const running = (p: Partial<LabStatus> = {}): LabStatus =>
    ({
      running: true,
      parked: null,
      machines: [],
      networks: [],
      url: "http://127.0.0.1:8080",
      host: null,
      expiresAt: null,
      place: "container",
      provider: "docker",
      outputs: [],
      message: null,
      ...p,
    }) as LabStatus;

  function renderRow(p: Partial<ComponentProps<typeof LabRow>> = {}) {
    const h = { onOpen: vi.fn(), onStop: vi.fn(), onResume: vi.fn() };
    render(<LabRow lab={lab} loggedIn hostArch="x86_64" {...h} {...p} />);
    return h;
  }

  it("opens a running lab's URL and asks before stopping it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const h = renderRow({ status: running() });
    expect(screen.getByText("Running")).toBeTruthy();
    expect(screen.getByText("Medium")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^Open$/ }));
    await flush();
    expect(calls.find((c) => c.cmd.includes("opener"))?.args).toMatchObject({ url: "http://127.0.0.1:8080" });
    expect(h.onOpen).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByText("Remove Invoice portal API?")).toBeTruthy();
    expect(screen.getByText(/To keep them, open the lab and shut it down instead/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onStop).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(h.onStop).toHaveBeenCalledTimes(1);
    expect(h.onOpen).not.toHaveBeenCalled();
  });

  it("doesn't offer to keep a lab on a server or in a VM", async () => {
    installTauri();
    const user = userEvent.setup();
    renderRow({ status: running({ host: "pve", url: null }) });
    expect(screen.queryByRole("button", { name: /^Open$/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.queryByText(/To keep them/)).toBeNull();
  });

  it("offers to connect a cloud account a cloud lab lacks", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const cloudLab: Lab = { ...lab, id: "lab-goat", title: "EC2 SSRF", runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"] } };
    const h = renderRow({ lab: cloudLab, setup: "Needs an account on AWS", connectCloud: true });
    expect(screen.getByText("cloud")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Connect/ }));
    await flush();
    expect(calls.some((c) => c.cmd === "server_open_setup" && c.args?.kind === "cloud")).toBe(true);
    expect(h.onOpen).not.toHaveBeenCalled();
  });

  it("says stopping a cloud lab destroys its services in the account", async () => {
    installTauri();
    const user = userEvent.setup();
    const cloudLab: Lab = { ...lab, id: "lab-goat", title: "EC2 SSRF", runtime: { runtime: "CLOUD", architectures: [], providers: ["aws"] } };
    renderRow({ lab: cloudLab, status: running({ host: "AWS sandbox", url: null, place: "cloud", provider: "aws" }) });
    await user.click(screen.getByRole("button", { name: "Stop" }));
    expect(screen.getByText(/Its cloud services are destroyed in your account/)).toBeTruthy();
  });

  it("says what is happening while busy and disables Stop", () => {
    installTauri();
    renderRow({ status: running(), busy: true, operation: "shutdown" });
    expect(screen.getByRole("button", { name: "Shutting down…" }).hasAttribute("disabled")).toBe(true);
  });

  it("says Working… for an operation it has no word for", () => {
    installTauri();
    renderRow({ status: running(), busy: true, operation: "provision" });
    expect(screen.getByRole("button", { name: "Working…" })).toBeTruthy();
  });

  it("resumes a paused or shut down lab", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = renderRow({ lab: vmLab, status: running({ running: false, parked: "pause", url: null, place: "local_vm" }) });
    expect(screen.getByText("paused")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Resume" }));
    expect(h.onResume).toHaveBeenCalled();
    expect(h.onOpen).not.toHaveBeenCalled();
  });

  it("shows a shut down lab without Resume when it can't resume", () => {
    installTauri();
    renderRow({ lab: vmLab, status: running({ running: false, parked: "shutdown", url: null }), onResume: undefined });
    expect(screen.getByText("shut down")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
  });

  it("opens machine setup when the machine is missing something", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    renderRow({ setup: "Install Docker" });
    expect(screen.getByText("Install Docker")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Set up/ }));
    await flush();
    expect(commands(calls)).toContain("machine_open_setup");
  });

  it("signs a signed-out player in from the row", async () => {
    installTauri();
    const user = userEvent.setup();
    const onLogin = vi.fn(() => Promise.resolve(true));
    const h = renderRow({ loggedIn: false, onLogin });
    await user.click(screen.getByRole("button", { name: /Sign in/ }));
    expect(onLogin).toHaveBeenCalled();
    // Signed in: the lab's page opens (starting happens there), as the row is for.
    await flush();
    expect(h.onOpen).toHaveBeenCalledWith(lab);
    // A sign-in that didn't go through (cancelled) opens nothing.
    h.onOpen.mockClear();
    onLogin.mockResolvedValueOnce(false);
    await user.click(screen.getByRole("button", { name: /Sign in/ }));
    await flush();
    expect(h.onOpen).not.toHaveBeenCalled();
  });

  it("opens the lab from the row's title with a click, Enter or Space", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = renderRow();
    const row = screen.getByRole("button", { name: "Open Invoice portal API" });
    await user.click(row);
    row.focus();
    await user.keyboard("{Enter}");
    await user.keyboard(" ");
    await user.keyboard("a");
    expect(h.onOpen).toHaveBeenCalledTimes(3);
  });

  it("keeps the row's actions beside its open button, not inside it, and Tab reaches each", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = renderRow({ status: running() });
    const open = screen.getByRole("button", { name: "Open Invoice portal API" });
    // A real button holding only the title: no control is nested in another.
    expect(open.tagName).toBe("BUTTON");
    expect(open.textContent).toBe("Invoice portal API");
    expect(open.querySelector("button, a, [role=button], [tabindex]")).toBeNull();
    expect(document.querySelector("[role=button]")).toBeNull();
    const [openUrl, stop] = [screen.getByRole("button", { name: /^Open$/ }), screen.getByRole("button", { name: "Stop" })];
    expect(open.contains(openUrl) || open.contains(stop)).toBe(false);
    // Its ::after stretches over the row (the positioned box); the actions are drawn above it.
    expect(open.className).toContain("after:absolute");
    expect(open.closest(".relative")?.contains(stop)).toBe(true);
    expect(stop.closest(".z-\\[1\\]")).toBeTruthy();
    // Keyboard: the title first, then the row's actions, in order; Enter on the title opens.
    await user.tab();
    expect(document.activeElement).toBe(open);
    await user.keyboard("{Enter}");
    expect(h.onOpen).toHaveBeenCalledTimes(1);
    await user.tab();
    expect(document.activeElement).toBe(openUrl);
    await user.tab();
    expect(document.activeElement).toBe(stop);
    await user.keyboard("{Enter}");
    expect(screen.getByText("Remove Invoice portal API?")).toBeTruthy();
    expect(h.onOpen).toHaveBeenCalledTimes(1);
  });

  it("marks a lab built for another CPU as emulated, and a solved one", () => {
    installTauri();
    renderRow({ hostArch: "arm64", solved: true });
    expect(screen.getByText("emulated")).toBeTruthy();
    expect(screen.getByText("Solved")).toBeTruthy();
  });

  it("doesn't call a VM lab for another CPU emulated without an emulator", () => {
    installTauri();
    renderRow({ lab: vmLab, hostArch: "arm64" });
    expect(screen.queryByText("emulated")).toBeNull();
    expect(screen.getByText("Not started")).toBeTruthy();
  });

  it("shows a deploy running in the backend as starting", () => {
    installTauri();
    renderRow({ deploying: true });
    expect(screen.getByText("Starting…")).toBeTruthy();
  });
});
