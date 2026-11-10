// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabRunOptions, LabRunPath, LabStatus, Provider } from "@/lib/tauri";
import { commands, installTauri, type Handler } from "@/test/tauri";
import { NOTHING, VMS_HERE, containerMachine, rejectWith, startIn, upStatus } from "@/features/labs/labs-test-kit";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { flush } from "@/test/ui";
import { getDockerEngine, getLocalVmForm, getVmProvider, setDockerEngine, setLocalVmForm, setPortMode, setVmProvider } from "@/lib/settings";

afterEach(() => {
  setPortMode(null);
  setDockerEngine(null);
  setLocalVmForm(null);
  setVmProvider(null);
});

// Behavior of the lab page's actions: what each button calls, with what, and what it shows.

const SHA = "0123456789abcdef0123456789abcdef01234567";
const SOURCE = { repository: "CyberCTF/lab", commit: SHA };
const dockerRuntime: LabRuntimeInfo = { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws", "virtualbox"], ...SOURCE };
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
  runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi"], ...SOURCE },
};

/** A launch path on `provider`, unbadged. */
const path = (provider: Provider): LabRunPath => ({ provider, badge: "none", evidence: "" });

/** The backend's run options with only `paths` set. */
const runOptions = (paths: Partial<LabRunOptions>): LabRunOptions => ({
  docker: path("virtualbox"),
  dockerVm: [],
  vms: [],
  servers: [],
  cloudDocker: [],
  cloudVm: [],
  cloudServices: [],
  refused: [],
  ...paths,
});

/** A container lab with VM recipes too, as the backend reads it: in one VM or one VM per machine
 *  here, and on every cloud in both forms. */
const bothForms = () =>
  runOptions({
    dockerVm: [path("virtualbox")],
    vms: [path("virtualbox")],
    cloudDocker: ["aws", "azure"].map((c) => path(c as Provider)),
    cloudVm: ["aws", "azure"].map((c) => path(c as Provider)),
  });

const machine = containerMachine;
const status = (p: Partial<LabStatus> = {}) => upStatus([machine("api")], p);
const stopped = status({ running: false, machines: [], url: null, place: null, provider: null });
const vmRunning = status({ machines: [machine("DC01", "running", { image: "" })], place: "local_vm", provider: "virtualbox", url: null, controls: VMS_HERE });

/** A container lab with VM recipes too, `id`, whose one saved place is cloud `account` (its test
 *  answering at once), the backend's answers installed. */
function dualLabOnCloud(id: string, account: { id: string; name: string; provider: Provider; host: string }): Lab {
  installTauri({
    ...quiet,
    server_list: () => ({ default: null, hosts: [account] }),
    server_test: () => ({ ok: true, reachable: true, authenticated: true, latencyMs: 9, message: "Connected", checks: [] }),
    lab_run_options: bothForms,
  });
  return { ...dockerLab, id, runtime: { runtime: "DOCKER", architectures: [], providers: [account.provider], docker: true, vm: true, ...SOURCE } };
}

// Settled: nothing deploying, no servers (Start then starts right away for a Docker lab).
const quiet: Record<string, Handler> = {
  deploying_labs: () => [],
  active_operations: () => [],
  server_list: () => ({ default: null, hosts: [] }),
};

/** A container lab's attack box in its VM on a server, as the runner reports it. */
const remoteBox = (up: boolean) => ({
  reachable: true,
  running: up,
  ip: up ? "10.30.0.77" : "",
  labNetwork: up ? "lab" : "",
  networks: ["lab"],
  defaultNetwork: "lab",
  vm: false,
});

// One saved server, an ESXi host, set as the default.
const defaultEsx: Record<string, Handler> = {
  server_list: () => ({ default: "esx", hosts: [{ id: "esx", name: "esx-lab", provider: "vmware_esxi", host: "10.0.0.2" }] }),
};

/** The page of `dockerLab` with nothing ready for a VM here, `props` over it: its handlers,
 *  the asynchronous ones settling at once as the screen's do. */
function renderDetail(props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const settled = () => vi.fn(() => Promise.resolve());
  const h = {
    onBack: vi.fn(),
    onStart: vi.fn(),
    onStop: settled(),
    onPark: settled(),
    onResume: settled(),
    onProvision: settled(),
    onLogin: vi.fn(() => Promise.resolve(true)),
  };
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
    // Two rows: Containers and In a VM. The port choice is NOT on this step.
    expect(within(dialog).getByText("Container")).toBeTruthy();
    expect(within(dialog).getByText("VM")).toBeTruthy();
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    await user.click(within(dialog).getByText("VM"));
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local-vm", provider: "virtualbox" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it('keeps a hypervisor picked without "Don\'t ask again" to that launch: the next one asks again', async () => {
    installTauri({ ...quiet, lab_run_options: () => runOptions({ dockerVm: [path("virtualbox"), path("qemu")] }) });
    const { user } = await clickStart({ readyVms: ["virtualbox", "qemu"] });
    await user.click(within(screen.getByRole("dialog")).getByText("VM"));
    await user.click(within(screen.getByRole("dialog")).getByText("QEMU"));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    // Settings' hypervisor is untouched, so the step is asked again.
    expect(getVmProvider()).toBeNull();
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    await user.click(within(screen.getByRole("dialog")).getByText("VM"));
    expect(within(screen.getByRole("dialog")).getByText("Which hypervisor?")).toBeTruthy();
  });

  it('saves the hypervisor and the VM form picked with "Don\'t ask again"', async () => {
    installTauri({ ...quiet, lab_run_options: () => runOptions({ dockerVm: [path("virtualbox"), path("qemu")] }) });
    const { user, h } = await clickStart({ readyVms: ["virtualbox", "qemu"] });
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByText("VM"));
    await user.click(within(dialog).getByText("QEMU"));
    await user.click(within(dialog).getByRole("switch", { name: "Don't ask again" }));
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local-vm", provider: "qemu" });
    expect(getVmProvider()).toBe("qemu");
    expect(getLocalVmForm()).toBe("vm");
  });

  it("reads the lab's run options and the saved servers once for the page and its picker", async () => {
    // Regression: Where it runs and the launch picker each read both, and a failed read was told twice.
    const calls = installTauri({ ...quiet, lab_run_options: () => runOptions({ vms: [path("virtualbox")] }) });
    renderDetail({ lab: vmLab, status: stopped, readyVms: ["virtualbox"] });
    await flush();
    expect(commands(calls).filter((c) => c === "lab_run_options")).toHaveLength(1);
    expect(commands(calls).filter((c) => c === "server_list")).toHaveLength(1);
  });

  it("sends Containers to a ports step, and Back returns to the places", async () => {
    installTauri(quiet);
    const { user } = await clickStart({ readyVms: ["virtualbox"] });
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText("Where should it run?")).toBeTruthy();
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    await user.click(within(dialog).getByText("Container"));
    // The ports step: the choice is here, not with the places.
    expect(within(dialog).getByText("Ports on this machine")).toBeTruthy();
    expect(within(dialog).getByText("The lab's default ports")).toBeTruthy();
    expect(within(dialog).queryByText("VM")).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Back" }));
    expect(within(dialog).getByText("Where should it run?")).toBeTruthy();
    expect(within(dialog).getByText("VM")).toBeTruthy();
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
  });

  it("passes the ports picked in the dialog for containers here, and Cancel starts nothing", async () => {
    installTauri(quiet);
    const { user, h } = await clickStart({ readyVms: ["virtualbox"] });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    // Containers takes a step to its ports, then the lab's default ports.
    await user.click(within(screen.getByRole("dialog")).getByText("Container"));
    await startIn(user, "The lab's default ports");
    expect(h.onStart).toHaveBeenCalledTimes(1);
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", ports: "default" });
  });

  it("sends a dual-form lab's In a VM row to a form step, and Back returns to the places", async () => {
    installTauri({ ...quiet, lab_run_options: bothForms });
    const both: Lab = { ...dockerLab, id: "lab-both", runtime: { ...dockerRuntime, docker: true, vm: true } };
    const { user } = await clickStart({ lab: both, readyVms: ["virtualbox"] });
    const dialog = screen.getByRole("dialog");
    // Step 1: two rows; the VM forms are a step in.
    expect(within(dialog).getByText("Where should it run?")).toBeTruthy();
    expect(within(dialog).getByText("Container")).toBeTruthy();
    expect(within(dialog).getByText("VM")).toBeTruthy();
    expect(within(dialog).queryByText("One VM per machine")).toBeNull();
    await user.click(within(dialog).getByText("VM"));
    // Step 2: only the two VM forms, no Containers.
    expect(within(dialog).getByText("How should it run?")).toBeTruthy();
    expect(within(dialog).getByText("In one VM")).toBeTruthy();
    expect(within(dialog).getByText("One VM per machine")).toBeTruthy();
    expect(within(dialog).queryByText("Container")).toBeNull();
    // Back returns to the places.
    await user.click(within(dialog).getByRole("button", { name: "Back" }));
    expect(within(dialog).getByText("Where should it run?")).toBeTruthy();
    expect(within(dialog).getByText("Container")).toBeTruthy();
    expect(within(dialog).queryByText("One VM per machine")).toBeNull();
  });

  it("offers a dual-form container lab on a cloud the two cloud forms, and starts one VM per machine", async () => {
    const bothCloud = dualLabOnCloud("lab-cloud", { id: "aws1", name: "AWS sandbox", provider: "aws", host: "eu-west-3" });
    const { user, h } = await clickStart({ lab: bothCloud, readyVms: [] });
    const dialog = screen.getByRole("dialog");
    // Pick the AWS account → the form step ("How should it run?"), with the two cloud forms as rows.
    await user.click(within(dialog).getByText("AWS sandbox"));
    expect(within(dialog).getByText("How should it run?")).toBeTruthy();
    expect(within(dialog).getByText("One VM")).toBeTruthy();
    expect(within(dialog).getByText("One VM per machine")).toBeTruthy();
    await user.click(within(dialog).getByText("One VM per machine"));
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "host", id: "aws1", perMachine: true });
  });

  it("offers a saved server only of a kind the lab's spec runs on, as the backend says", async () => {
    const calls = installTauri({
      ...quiet,
      server_list: () => ({ default: null, hosts: [{ id: "pve", name: "homelab", provider: "proxmox", host: "10.0.0.5" }] }),
      lab_run_options: () => runOptions({ docker: null, vms: [path("virtualbox")], servers: [path("vmware_esxi")] }),
    });
    // The catalogue lists Proxmox; the lab's targets leave it out.
    const lab: Lab = {
      ...vmLab,
      runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "proxmox"], repository: "CyberCTF/goad", commit: SHA },
    };
    await clickStart({ lab, readyVms: ["virtualbox"] });
    expect(calls.find((c) => c.cmd === "lab_run_options")?.args).toEqual({
      id: "lab-goad",
      runtime: "VM",
      source: { repository: "CyberCTF/goad", commit: SHA },
    });
    expect(within(screen.getByRole("dialog")).getByTitle("This lab doesn't support Proxmox")).toBeTruthy();
  });

  it("offers the cloud form step on every cloud, not only AWS", async () => {
    const bothCloud = dualLabOnCloud("lab-cloud2", { id: "az1", name: "Azure sub", provider: "azure", host: "westeurope" });
    const { user } = await clickStart({ lab: bothCloud, readyVms: [] });
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByText("Azure sub"));
    // Azure has a cloud-vm module too: one VM, or one VM per machine.
    expect(within(dialog).getByText("How should it run?")).toBeTruthy();
  });

  it("picks a container engine when several answer, remembers it, and starts on it", async () => {
    installTauri(quiet);
    const { user, h } = await clickStart({ dockerEngine: "docker-desktop", dockerEnginesRunning: ["docker-desktop", "orbstack"] });
    const dialog = screen.getByRole("dialog");
    // Two engines answer, so the dialog opens even with nowhere else to run; Container leads to an
    // engine step ("Which engine?") of full rows, then the ports step.
    await user.click(within(dialog).getByText("Container"));
    expect(within(dialog).getByText("Which engine?")).toBeTruthy();
    expect(within(dialog).getByText("Docker Desktop")).toBeTruthy();
    // No port choice on the engine step, and no "Don't ask again" until the last step.
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    expect(within(dialog).queryByRole("switch", { name: "Don't ask again" })).toBeNull();
    await user.click(within(dialog).getByText("OrbStack"));
    // The ports step, the last one: the engine's choice carries over and "Don't ask again" appears.
    expect(within(dialog).getByText("Ports on this machine")).toBeTruthy();
    await user.click(within(dialog).getByRole("switch", { name: "Don't ask again" }));
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", ports: "random", engine: "orbstack" });
    // Remembered: the choice is saved for next time.
    expect(getDockerEngine()).toBe("orbstack");
  });

  it("starts straight on a remembered engine without opening the dialog", async () => {
    installTauri(quiet);
    setDockerEngine("orbstack");
    const { h } = await clickStart({ dockerEngine: "docker-desktop", dockerEnginesRunning: ["docker-desktop", "orbstack"] });
    // The engine is settled (remembered) and there is nowhere else to run, so Start runs here at
    // once on that engine; no dialog.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", engine: "orbstack" });
  });

  it("points Start where the backend starts the lab with nothing picked: the default server", async () => {
    installTauri({
      ...quiet,
      ...defaultEsx,
      lab_default_place: () => ({ host: "esx", provider: null, perMachineVm: false }),
    });
    const { user, h } = await clickStart({ lab: vmLab, readyVms: [] });
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByText("Ports on this machine")).toBeNull();
    expect(within(dialog).getByText("On your system, in a VM")).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "host", id: "esx" });
  });

  it("points Start here when the backend keeps the lab here, even with a default server", async () => {
    installTauri({
      ...quiet,
      ...defaultEsx,
      lab_default_place: () => ({ host: null, provider: "virtualbox", perMachineVm: false, hypervisor: "virtualbox" }),
    });
    const { user, h } = await clickStart({ lab: vmLab, readyVms: ["virtualbox"] });
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: /^Start$/ }));
    // On the hypervisor the backend picked.
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", provider: "virtualbox" });
  });

  it("can't start a lab without a runtime", async () => {
    installTauri(quiet);
    renderDetail({ lab: { ...dockerLab, runtime: null }, status: undefined });
    await flush();
    const start = screen.getByRole("button", { name: /Start lab/ });
    expect(start.hasAttribute("disabled")).toBe(true);
    expect(start.getAttribute("title")).toBe("No runtime for this lab yet");
  });

  it("signs a signed-out player in from the header, then starts the lab here when there is nowhere else", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ loggedIn: false, status: stopped });
    await flush();
    await user.click(screen.getByRole("button", { name: /Sign in to start/ }));
    expect(h.onLogin).toHaveBeenCalled();
    expect(screen.getByText("Sign in to run labs on this machine.")).toBeTruthy();
    // Signed in: the start the player asked for goes on by itself.
    await flush();
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local" });
  });

  it("a sign-in from the header that was cancelled, or a lab that is busy, starts nothing", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const h = renderDetail({ loggedIn: false, status: stopped, onLogin: vi.fn(() => Promise.resolve(false)) });
    await flush();
    await user.click(screen.getByRole("button", { name: /Sign in to start/ }));
    await flush();
    expect(h.onStart).not.toHaveBeenCalled();
  });

  it("asks where to run once a sign-in from the header went through, when there is a choice", async () => {
    installTauri(quiet);
    const user = userEvent.setup();
    const onLogin = vi.fn(() => Promise.resolve(true));
    const onStart = vi.fn();
    const page = (loggedIn: boolean) => (
      <LabDetail
        lab={dockerLab}
        status={stopped}
        busy={false}
        logs={[]}
        times={[]}
        loggedIn={loggedIn}
        hostArch="x86_64"
        readyVms={["virtualbox"]}
        dockerRunning
        onBack={vi.fn()}
        onStart={onStart}
        onStop={vi.fn(() => Promise.resolve())}
        onLogin={onLogin}
      />
    );
    const { rerender } = render(page(false));
    await flush();
    await user.click(screen.getByRole("button", { name: /Sign in to start/ }));
    // The shell learns of the sign-in and the page re-renders signed in.
    rerender(page(true));
    await flush();
    expect(onStart).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Where should the lab run?" })).toBeTruthy();
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
    expect(calls.find((c) => c.cmd === "terminal_window")?.args).toEqual({ id: "lab-goad", runtime: "VM", title: "GOAD Light" });
    expect(screen.getByText("no attack VM")).toBeTruthy();
  });

  it("reads a container lab's box on a server: no shell before the box runs", async () => {
    const calls = installTauri({ ...quiet, attack_box_status: () => remoteBox(false) });
    renderDetail({ status: status({ place: "server", host: "aws-1", controls: NOTHING, url: null }) });
    await flush();
    expect(calls.find((c) => c.cmd === "attack_box_status")?.args).toMatchObject({ runtime: "DOCKER" });
    // The lab running says nothing of its box: no "running on" and no shell before it runs.
    expect(screen.queryByText("running on aws-1")).toBeNull();
    expect(screen.queryByRole("button", { name: /Open attacker shell/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Start attack box/ })).toBeTruthy();
  });

  it("shows a cloud lab's address without its scheme, so it stays on one line, and copies it whole", async () => {
    installTauri({ ...quiet, attack_box_status: () => remoteBox(false) });
    renderDetail({ status: status({ place: "server", host: "aws-1", controls: NOTHING, url: "http://13.39.163.133:8180/", machines: [] }) });
    await flush();
    const address = screen.getByRole("button", { name: /^13\.39\.163\.133:8180/ });
    expect(address.getAttribute("title")).toContain("http://13.39.163.133:8180/");
  });

  it("opens the shell into a container lab's box on a server once the box runs", async () => {
    const calls = installTauri({ ...quiet, attack_box_status: () => remoteBox(true) });
    const user = userEvent.setup();
    renderDetail({ status: status({ place: "server", host: "aws-1", controls: NOTHING, url: null }) });
    await flush();
    expect(screen.getByText("running on aws-1")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Open attacker shell/ }));
    await flush();
    expect(calls.find((c) => c.cmd === "terminal_window")?.args).toMatchObject({ runtime: "DOCKER" });
    expect(commands(calls)).not.toContain("attack_box_attach");
    // No shut down or pause for a lab on a server.
    expect(screen.queryByRole("button", { name: /Shut down/ })).toBeNull();
  });

  it("reads a VM lab's box on a server as on this machine, and leaves adding it to the auto-start with the lab", async () => {
    for (const [provider, reachable] of [
      ["proxmox", true],
      ["vmware_esxi", false],
    ] as const) {
      const calls = installTauri({ ...quiet, attack_box_status: () => ({ ...remoteBox(false), vm: true, reachable }) });
      renderDetail({ lab: vmLab, status: status({ place: "server", host: "srv-1", controls: NOTHING, provider, url: null }) });
      await flush();
      expect(
        calls.some((c) => c.cmd === "attack_box_status" && (c.args as { runtime?: string }).runtime === "VM"),
        provider,
      ).toBe(true);
      expect(commands(calls), provider).not.toContain("attack_box_attach");
      cleanup();
    }
  });

  it("offers no attacker shell on a server or cloud account until the lab is up", async () => {
    installTauri(quiet);
    // Still starting there (the infrastructure being created): no shell into a box not up yet.
    renderDetail({ status: status({ running: false, machines: [], place: "server", host: "aws-1", controls: NOTHING, url: null }), busy: true });
    await flush();
    expect(screen.queryByRole("button", { name: /Open attacker shell/ })).toBeNull();
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
        controls: VMS_HERE,
      }),
    });
    await flush();
    await user.click(screen.getByRole("button", { name: /Start them/ }));
    expect(h.onResume).toHaveBeenCalled();
  });

  it("offers Start them where the backend can bring machines back: a container lab run as one VM per machine, not one in a helper VM", async () => {
    installTauri(quiet);
    const machines = [machine("web", "running", { image: "" }), machine("db", "poweroff", { image: "" })];
    const perMachine = status({ machines, place: "local_vm", provider: "virtualbox", perMachine: true, url: null, controls: VMS_HERE });
    renderDetail({ status: perMachine });
    await flush();
    expect(screen.getByRole("button", { name: /Start them/ })).toBeTruthy();
    cleanup();
    renderDetail({ status: { ...perMachine, perMachine: false, controls: ["pause"] } });
    await flush();
    expect(screen.queryByRole("button", { name: /Start them/ })).toBeNull();
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
