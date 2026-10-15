// @vitest-environment jsdom
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";
import { commands, installTauri, type Handler } from "@/test/tauri";
import { containerMachine, rejectWith, startIn, upStatus } from "@/features/labs/labs-test-kit";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { flush } from "@/test/ui";

// Behavior of the lab page's actions: what each button calls, with what, and what it shows.

const dockerRuntime: LabRuntimeInfo = { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws", "virtualbox"] };
const dockerLab: Lab = {
  id: "lab-invoice",
  slug: "invoice-portal-api",
  title: "Invoice portal API",
  description: "A billing portal exposes a REST API guarded by a static key.",
  question: null,
  difficulty: 1,
  category: "Web",
  runtime: dockerRuntime,
  skills: [],
};
const vmLab: Lab = {
  ...dockerLab,
  id: "lab-goad",
  slug: "goad-light",
  title: "GOAD Light",
  runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi"] },
};

const machine = containerMachine;
const status = (p: Partial<LabStatus> = {}) => upStatus([machine("api")], p);
const stopped = status({ running: false, machines: [], url: null, place: null, provider: null });
const vmRunning = status({ machines: [machine("DC01", "running", { image: "" })], place: "local_vm", provider: "virtualbox", url: null });

// Settled: nothing deploying, no servers (Start then starts right away for a Docker lab).
const quiet: Record<string, Handler> = {
  deploying_labs: () => [],
  active_operations: () => [],
  server_list: () => ({ default: null, hosts: [] }),
};

// One saved server, an ESXi host, set as the default.
const defaultEsx: Record<string, Handler> = {
  server_list: () => ({ default: "esx", hosts: [{ id: "esx", name: "esx-lab", provider: "vmware_esxi", host: "10.0.0.2" }] }),
};

/** The page of `dockerLab` with nothing ready for a VM here, `props` over it: its handlers,
 *  the asynchronous ones settling at once as the screen's do. */
function renderDetail(props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const settled = () => vi.fn(() => Promise.resolve());
  const h = { onBack: vi.fn(), onStart: vi.fn(), onStop: settled(), onPark: settled(), onResume: settled(), onProvision: settled(), onLogin: settled() };
  renderLabPage(dockerLab, { readyVms: [], ...h, ...props });
  return h;
}

describe("LabDetail: starting", () => {
  /** The page of a stopped lab, `props` over it, once Start lab was clicked. */
  const clickStart = async (props: Partial<Parameters<typeof LabDetail>[0]> = {}) => {
    const user = userEvent.setup();
    const h = renderDetail({ status: stopped, ...props });
    await flush();
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    return { user, h };
  };

  it("starts a Docker lab here at once when there is nowhere else to run it", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ status: stopped });
    await flush();
    const start = screen.getByRole("button", { name: /Start lab/ });
    expect(start.getAttribute("aria-haspopup")).toBeNull();
    await user.click(start);
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("asks where to run a container lab and starts it in a VM here", async () => {
    installTauri(quiet);
    const { user, h } = await clickStart({ readyVms: ["virtualbox"] });
    const dialog = screen.getByRole("dialog");
    // The port choice shows only for this machine's containers.
    expect(within(dialog).getByText("Ports on this machine")).toBeTruthy();
    await user.click(within(dialog).getByText("This machine, in a VM"));
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local-vm", provider: "virtualbox" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("passes the ports picked in the dialog for containers here, and Cancel starts nothing", async () => {
    installTauri(quiet);
    const { user, h } = await clickStart({ readyVms: ["virtualbox"] });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    await startIn(user, "The lab's default ports");
    expect(h.onStart).toHaveBeenCalledTimes(1);
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", ports: "default" });
  });

  it("defaults a VM lab with no hypervisor here to the default server", async () => {
    installTauri({
      ...quiet,
      ...defaultEsx,
    });
    const { user, h } = await clickStart({ lab: vmLab, readyVms: [] });
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    expect(within(dialog).getByText("On your system, in a VM")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "host", id: "esx" });
  });

  it("keeps a VM lab here when a local hypervisor can run it, even with a default server", async () => {
    installTauri({
      ...quiet,
      ...defaultEsx,
    });
    const { user, h } = await clickStart({ lab: vmLab, readyVms: ["virtualbox"] });
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local" });
  });

  it("starts a hosted run, follows it, and shows a failed stop", async () => {
    const LAUNCHED = {
      id: "s1",
      labId: "lab-invoice",
      target: "hosted",
      state: "RUNNING",
      endpoints: [{ port: 80, url: "https://s1.example" }],
      message: null,
      reapError: null,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
    };
    const calls = installTauri({
      ...quiet,
      api_query: (a) => {
        const q = String(a?.operation);
        if (q.includes("launchHosted")) return { requestLabLaunch: LAUNCHED };
        if (q.includes("stopSession")) rejectWith("backend down");
        return {};
      },
    });
    const { user, h } = await clickStart({ lab: { ...dockerLab, runtime: { ...dockerRuntime, hosted: true } } });
    await startIn(user, "Hosted by Cyber CTF");
    await flush();
    expect(h.onStart).not.toHaveBeenCalled();
    expect(calls.find((c) => String(c.args?.operation).includes("launchHosted"))?.args?.variables).toEqual({ labId: "lab-invoice" });
    expect(screen.getByText("Running, hosted by Cyber CTF")).toBeTruthy();
    // One run at a time: Start waits for the hosted one to end.
    const start = screen.getByRole("button", { name: /Start lab/ });
    expect(start.hasAttribute("disabled")).toBe(true);
    expect(start.getAttribute("title")).toBe("It's running hosted by Cyber CTF; stop it first");
    await user.click(screen.getByRole("button", { name: /^\s*Stop$/ }));
    await flush();
    // Regression: the stop's error was set but the panel was hidden, so the lab kept running unseen.
    expect(screen.getByText("Couldn't stop the hosted lab: backend down")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(/Couldn't stop the hosted lab/)).toBeNull();
    expect(screen.getByRole("button", { name: /Start lab/ }).hasAttribute("disabled")).toBe(false);
  });

  it("hides the hosted panel after a stop that succeeds", async () => {
    const LAUNCHED = {
      id: "s2",
      labId: "lab-invoice",
      target: "hosted",
      state: "RUNNING",
      endpoints: [],
      message: null,
      reapError: null,
      expiresAt: new Date(Date.now() + 3600_000).toISOString(),
    };
    installTauri({
      ...quiet,
      api_query: (a) => (String(a?.operation).includes("activeSession") ? { myActiveLabSession: LAUNCHED } : {}),
    });
    const user = userEvent.setup();
    renderDetail({ lab: { ...dockerLab, runtime: { ...dockerRuntime, hosted: true } }, status: stopped });
    await flush();
    expect(screen.getByText("Running, hosted by Cyber CTF")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^\s*Stop$/ }));
    await flush();
    expect(screen.queryByText("Running, hosted by Cyber CTF")).toBeNull();
    expect(screen.queryByText(/Couldn't stop/)).toBeNull();
  });

  it("can't start a lab without a runtime", async () => {
    installTauri(quiet);
    renderDetail({ lab: { ...dockerLab, runtime: null }, status: undefined });
    await flush();
    const start = screen.getByRole("button", { name: /Start lab/ });
    expect(start.hasAttribute("disabled")).toBe(true);
    expect(start.getAttribute("title")).toBe("No runtime for this lab yet");
  });

  it("signs a signed-out player in from the header, or greys Start without a way to", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ loggedIn: false, status: stopped });
    await flush();
    await user.click(screen.getByRole("button", { name: /Sign in to start/ }));
    expect(h.onLogin).toHaveBeenCalled();
    expect(screen.getByText("Sign in to run labs on this machine.")).toBeTruthy();
  });

  it("explains where a lab can run and when it runs emulated", async () => {
    installTauri(quiet);
    renderDetail({ lab: vmLab, status: stopped, hostArch: "arm64" });
    await flush();
    expect(screen.getByText("Where it runs")).toBeTruthy();
    expect(screen.getByText(/Built for another CPU: install QEMU/)).toBeTruthy();
  });

  it("says containers run emulated on another CPU", async () => {
    installTauri(quiet);
    renderDetail({ lab: { ...dockerLab, runtime: { ...dockerRuntime, architectures: ["x86_64"] } }, status: stopped, hostArch: "arm64" });
    await flush();
    expect(screen.getByText("Emulated on your CPU (slower than native).")).toBeTruthy();
  });
});

describe("LabDetail: a lab that is up", () => {
  it("opens a running lab's URL, shuts it down, and asks before removing it", async () => {
    const calls = installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ status: status() });
    await flush();
    await user.click(screen.getByRole("button", { name: /Open in browser/ }));
    await flush();
    expect(calls.find((c) => c.cmd.includes("opener"))?.args).toMatchObject({ url: "http://127.0.0.1:8080" });
    // A container lab here: Shut down, no Pause.
    expect(screen.queryByRole("button", { name: /^\s*Pause$/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: /Shut down/ }));
    expect(h.onPark).toHaveBeenCalledWith("shutdown");
    await user.click(screen.getByRole("button", { name: /Stop & remove/ }));
    expect(screen.getByText("Remove Invoice portal API?")).toBeTruthy();
    expect(screen.getByText(/To keep them, shut the lab down instead/)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(h.onStop).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: /Stop & remove/ }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(h.onStop).toHaveBeenCalledTimes(1);
  });

  it("pauses or shuts down a VM lab here, and re-runs its setup on one machine", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ lab: vmLab, status: vmRunning });
    await flush();
    // A VM lab here can be shut down too (only a container lab in a VM can't).
    expect(screen.getByRole("button", { name: /Shut down/ })).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /^\s*Pause$/ }));
    expect(h.onPark).toHaveBeenCalledWith("pause");
    const select = screen.getByRole("combobox");
    await user.selectOptions(select, "DC01");
    await user.click(screen.getByRole("button", { name: /Re-run setup/ }));
    expect(h.onProvision).toHaveBeenCalledWith("DC01");
  });

  it("re-runs the setup on all machines by default", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ lab: vmLab, status: vmRunning });
    await flush();
    await user.click(screen.getByRole("button", { name: /Re-run setup/ }));
    expect(h.onProvision).toHaveBeenCalledWith(null);
  });

  it("shows the pending action's busy word on its own button only", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    let release!: () => void;
    const onPark = vi.fn(() => new Promise<void>((r) => (release = r)));
    renderDetail({ lab: vmLab, status: vmRunning, onPark });
    await flush();
    await user.click(screen.getByRole("button", { name: /^\s*Pause$/ }));
    expect(screen.getByRole("button", { name: /Pausing…/ })).toBeTruthy();
    release();
    await flush();
    expect(screen.getByRole("button", { name: /^\s*Pause$/ })).toBeTruthy();
  });

  it("says Stopping… while a stop runs", async () => {
    installTauri(quiet);
    renderDetail({ status: status(), busy: true, logs: ["==> api: Stopping"], times: [1] });
    await flush();
    expect(screen.getAllByText(/Stopping|Starting/).length).toBeGreaterThan(0);
  });

  it("opens the attack VM's shell for a local VM lab and shows why it failed", async () => {
    const calls = installTauri({
      ...quiet,
      terminal_window: () => rejectWith("no attack VM"),
    });
    const user = userEvent.setup();
    renderDetail({ lab: vmLab, status: vmRunning });
    await flush();
    await user.click(screen.getByRole("button", { name: /Open attacker shell/ }));
    await flush();
    expect(calls.find((c) => c.cmd === "terminal_window")?.args).toEqual({ id: "lab-goad", kind: "attackVm", runtime: "VM", title: "GOAD Light" });
    expect(screen.getByText("no attack VM")).toBeTruthy();
  });

  it("opens the lab's own shell for a container lab on a server", async () => {
    const calls = installTauri(quiet);
    const user = userEvent.setup();
    renderDetail({ status: status({ place: "server", host: "aws-1", url: null }) });
    await flush();
    expect(screen.getByText("running on aws-1")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Open attacker shell/ }));
    await flush();
    expect(calls.find((c) => c.cmd === "terminal_window")?.args).toMatchObject({ kind: "lab", runtime: "DOCKER" });
    // No shut down or pause for a lab on a server.
    expect(screen.queryByRole("button", { name: /Shut down/ })).toBeNull();
    expect(commands(calls)).not.toContain("exegol_status");
  });

  it("resets a lab with a container down: stops it, then starts it again where it ran", async () => {
    installTauri({ ...quiet, lab_check: () => ({ available: true, ok: false, output: "", results: [] }) });
    const user = userEvent.setup();
    const order: string[] = [];
    /** A handler that notes `what` ran, then settles. */
    const noting = (what: string) => () => {
      order.push(what);
      return Promise.resolve();
    };
    const onStart = vi.fn(noting("start"));
    renderDetail({
      status: status({ machines: [machine("api"), machine("db", "exited")] }),
      onStop: vi.fn(noting("stop")),
      onStart,
    });
    await flush();
    expect(screen.getByText("db is down. The lab may not work.")).toBeTruthy();
    expect(screen.getByText(/can no longer be solved/)).toBeTruthy();
    // Container labs have no "Start them".
    expect(screen.queryByRole("button", { name: /Start them/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: /Reset lab/ }));
    await flush();
    expect(order).toEqual(["stop", "start"]);
    expect(onStart).toHaveBeenCalledWith({ kind: "local" });
  });

  it("brings a VM lab's machines back with Start them", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({
      lab: vmLab,
      status: status({
        machines: [machine("DC01", "running", { image: "" }), machine("SRV02", "poweroff", { image: "" })],
        place: "local_vm",
        provider: "virtualbox",
        url: null,
      }),
    });
    await flush();
    await user.click(screen.getByRole("button", { name: /Start them/ }));
    expect(h.onResume).toHaveBeenCalled();
  });
});

describe("LabDetail: parked and interrupted", () => {
  it("removes a parked lab after confirming", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ lab: vmLab, status: status({ running: false, parked: "shutdown", machines: [], place: "local_vm", url: null }) });
    await flush();
    expect(screen.getAllByText(/shut down; its machines keep their state/).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: /Stop & remove/ }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(h.onStop).toHaveBeenCalled();
  });

  it("cleans up a start that was interrupted", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ status: status({ running: false, machines: [machine("api", "exited")], url: null }) });
    await flush();
    const clean = screen.getByRole("button", { name: /Stop & clean up/ });
    expect(clean.getAttribute("title")).toMatch(/interrupted/);
    await user.click(clean);
    expect(h.onStop).toHaveBeenCalled();
  });
});
