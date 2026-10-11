// @vitest-environment jsdom
import { act, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostedSession } from "@/features/hosted/use-hosted-labs";
import { HostedSessionPanel } from "@/features/labs/hosted-session-panel";
import { HealthBanner, useLabCheck } from "@/features/labs/lab-health";
import { LabRow } from "@/features/labs/lab-row";
import { askPortMode, PortModePrompt } from "@/features/labs/port-mode-prompt";
import { RunOnDialog, RunOnPicker, type RunTarget } from "@/features/labs/run-on";
import type { Lab } from "@/features/labs/use-labs";
import { getPortMode, setPortMode } from "@/lib/settings";
import type { LabCheck, LabStatus, ServerHost } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

afterEach(() => setPortMode(null));

// ---------------------------------------------------------------------------------------------
describe("HostedSessionPanel", () => {
  const session = (p: Partial<HostedSession> = {}): HostedSession => ({
    id: "s1",
    labId: "lab-invoice",
    target: "hosted",
    state: "RUNNING",
    endpoints: [{ port: 8080, url: "https://s1.labs.cyberctf.org" }],
    message: null,
    expiresAt: new Date(Date.now() + 90 * 60_000 + 30_000).toISOString(),
    ...p,
  });

  it("opens a running session's single URL and stops it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const onStop = vi.fn();
    render(<HostedSessionPanel session={session()} starting={false} error={null} onStop={onStop} />);
    expect(screen.getByText("Running, hosted by Cyber CTF")).toBeTruthy();
    expect(screen.getByText(/Expires in 1h 30m\./)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^Open/ }));
    await flush();
    expect(calls.find((c) => c.cmd.includes("opener"))?.args).toMatchObject({ url: "https://s1.labs.cyberctf.org" });
    await user.click(screen.getByRole("button", { name: /Stop/ }));
    expect(onStop).toHaveBeenCalled();
  });

  it("names each port when a session has several, and no expiry once it has passed", () => {
    installTauri();
    render(
      <HostedSessionPanel
        session={session({
          endpoints: [
            { port: 80, url: "https://a" },
            { port: 8000, url: "https://b" },
          ],
          expiresAt: new Date(Date.now() - 1000).toISOString(),
        })}
        starting={false}
        error={null}
        onStop={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /Open :80$/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Open :8000/ })).toBeTruthy();
    expect(screen.queryByText(/Expires in/)).toBeNull();
  });

  it.each([
    ["REQUESTED", "Asking Cyber CTF for an instance… This usually takes about 30 seconds."],
    ["CLAIMED", "Preparing your instance… This usually takes about 30 seconds."],
    ["PULLING", "Starting the lab… This usually takes about 30 seconds."],
  ] as const)("shows the %s stage while it comes up", (state, text) => {
    render(<HostedSessionPanel session={session({ state, endpoints: [] })} starting={false} error={null} onStop={vi.fn()} />);
    expect(screen.getByText(text)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Stop/ })).toBeTruthy();
  });

  it("shows the request on its way before a session exists, without buttons", () => {
    const { rerender } = render(<HostedSessionPanel session={null} starting error={null} onStop={vi.fn()} />);
    expect(screen.getByText(/Asking Cyber CTF for an instance/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
    rerender(<HostedSessionPanel session={null} starting={false} error={null} onStop={vi.fn()} />);
    expect(screen.getByText(/^\s*Starting…/)).toBeTruthy();
  });

  it("shows a failed session with the backend's message, dismissable", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    render(<HostedSessionPanel session={session({ state: "FAILED", message: "No capacity" })} starting={false} error={null} onStop={onStop} />);
    expect(screen.getByText("Couldn’t start the hosted lab")).toBeTruthy();
    expect(screen.getByText("No capacity")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(onStop).toHaveBeenCalled();
  });

  it("shows a failed request's error, or asks to try again", () => {
    const { rerender } = render(<HostedSessionPanel session={null} starting={false} error="Error: not signed in" onStop={vi.fn()} />);
    expect(screen.getByText("This is taking longer than expected")).toBeTruthy();
    expect(screen.getByText("Error: not signed in")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeTruthy();
    rerender(<HostedSessionPanel session={session({ state: "FAILED", message: null })} starting={false} error={null} onStop={vi.fn()} />);
    expect(screen.getByText("Try again in a moment.")).toBeTruthy();
  });

  it("says when a session ended or expired", () => {
    const { rerender } = render(<HostedSessionPanel session={session({ state: "STOPPED" })} starting={false} error={null} onStop={vi.fn()} />);
    expect(screen.getByText("The hosted session ended.")).toBeTruthy();
    rerender(<HostedSessionPanel session={session({ state: "EXPIRED" })} starting={false} error={null} onStop={vi.fn()} />);
    expect(screen.getByText("The hosted session expired.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Dismiss" })).toBeTruthy();
  });
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
    const onChange = vi.fn();
    render(
      <RunOnPicker
        title="GOAD Light"
        hosts={hosts}
        hostOk={(h) => h.provider !== "proxmox"}
        localNote="On your system, as containers"
        dockerRunning
        localVm={null}
        hosted={false}
        value={{ kind: "local" }}
        onChange={onChange}
        disabled={false}
        {...p}
      />,
    );
    return onChange;
  }

  it("groups this machine, servers and cloud accounts with each host's live check", async () => {
    const calls = installTauri({
      server_test: (a) => {
        if (a?.id === "down") throw new Error("connection refused");
        return a?.id === "aws"
          ? { ok: false, reachable: true, authenticated: false, latencyMs: 40, message: "Credentials expired", checks: [] }
          : { ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] };
      },
    });
    const user = userEvent.setup();
    const onChange = renderPicker();
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
    const pve = screen.getByText("homelab-pve").closest("[role=radio]")!;
    expect(pve.getAttribute("aria-disabled") === "true" || pve.hasAttribute("disabled")).toBe(true);
    expect(within(pve as HTMLElement).getByText("Not supported by this lab")).toBeTruthy();
    await user.click(screen.getByText("esx-lab"));
    expect(onChange).toHaveBeenCalledWith({ kind: "host", id: "esx" } satisfies RunTarget);
  });

  it("offers a VM here and hosted runs, and warns when the engine is stopped", async () => {
    installTauri();
    const user = userEvent.setup();
    const onChange = renderPicker({ hosts: [], dockerRunning: false, localVm: "virtualbox", hosted: true, localExtra: <p>ports here</p> });
    expect(screen.getByText("Engine stopped")).toBeTruthy();
    expect(screen.getByText("This machine, in a VM")).toBeTruthy();
    expect(screen.getByText("VirtualBox · isolated from your system")).toBeTruthy();
    expect(screen.getByText("ports here")).toBeTruthy();
    await user.click(screen.getByText("This machine, in a VM"));
    expect(onChange).toHaveBeenLastCalledWith({ kind: "local-vm", provider: "virtualbox" });
    await user.click(screen.getByText("Hosted by Cyber CTF"));
    expect(onChange).toHaveBeenLastCalledWith({ kind: "hosted" });
    expect(screen.queryByText("Servers")).toBeNull();
  });

  it("shows no engine status for a VM lab, and nothing selectable while busy", async () => {
    installTauri();
    const user = userEvent.setup();
    const onChange = renderPicker({ hosts: [], dockerRunning: null, disabled: true });
    expect(screen.queryByText("Running")).toBeNull();
    expect(screen.queryByText("Engine stopped")).toBeNull();
    await user.click(screen.getAllByText("This machine")[1]);
    expect(onChange).not.toHaveBeenCalled();
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
    await user.pointer({ keys: "[MouseLeft]", target: screen.getByRole("dialog").parentElement! });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});

// ---------------------------------------------------------------------------------------------
describe("PortModePrompt", () => {
  it("answers the recommended random ports by default", async () => {
    const user = userEvent.setup();
    render(<PortModePrompt />);
    let answer: Promise<unknown>;
    act(() => {
      answer = askPortMode("Blind orders");
    });
    expect(screen.getByText("Which ports should Blind orders use?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Start lab" }));
    await expect(answer!).resolves.toBe("random");
    // Not remembered unless asked.
    expect(getPortMode()).toBeNull();
  });

  it("cancels on Escape, on the backdrop, and when a second start asks again", async () => {
    const user = userEvent.setup();
    render(<PortModePrompt />);
    let first: Promise<unknown>, second: Promise<unknown>;
    act(() => {
      first = askPortMode("A");
    });
    act(() => {
      second = askPortMode("B");
    });
    await expect(first!).resolves.toBeNull();
    expect(screen.getByText("Which ports should B use?")).toBeTruthy();
    await user.keyboard("{Escape}");
    await expect(second!).resolves.toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    let third: Promise<unknown>;
    act(() => {
      third = askPortMode("C");
    });
    await user.pointer({ keys: "[MouseLeft]", target: screen.getByRole("dialog").parentElement! });
    await expect(third!).resolves.toBeNull();
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
    act(() => result.current[1]());
    expect(result.current[0]).toBeNull();
    // A new incident after recovery is checked again.
    rerender({ downCount: 0, running: true });
    rerender({ downCount: 1, running: true });
    await flush();
    expect(calls.filter((c) => c.cmd === "lab_check")).toHaveLength(2);
  });

  it("reports a check that errors as not solvable", async () => {
    installTauri({
      lab_check: () => {
        throw "docker unreachable";
      },
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
    await act(async () => answer({ available: true, ok: false, output: "", results: [] }));
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
  const lab: Lab = {
    id: "lab-invoice",
    slug: "invoice-portal-api",
    title: "Invoice portal API",
    description: "A billing portal",
    question: null,
    difficulty: 2,
    category: "Web",
    runtime: { runtime: "DOCKER", architectures: ["x86_64"], providers: [] },
    skills: [],
  };
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
      attacker: null,
      ...p,
    }) as LabStatus;

  function renderRow(p: Partial<Parameters<typeof LabRow>[0]> = {}) {
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
    const onLogin = vi.fn(async () => {});
    renderRow({ loggedIn: false, onLogin });
    await user.click(screen.getByRole("button", { name: /Sign in/ }));
    expect(onLogin).toHaveBeenCalled();
  });

  it("opens the lab from the row with a click, Enter or Space", async () => {
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
