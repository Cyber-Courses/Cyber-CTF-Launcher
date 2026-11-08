// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MachineSetup } from "@/features/machine/machine-setup";
import { machineSteps } from "@/features/machine/setup-steps";
import { Onboarding } from "@/features/onboarding/onboarding";
import type { SystemReport } from "@/lib/tauri";
import { getLastTest, getVmProvider, setLastTest } from "@/lib/settings";
import { freshReport, hypervisorStatus as hv, readyReport, stoppedDockerReport } from "@/test/fixtures";
import { must } from "@/components/test-support";
import { installTauri } from "@/test/tauri";
import { button, clickEverything, flush } from "@/test/ui";

const enabled = () => screen.queryAllByRole("button").filter((b) => !b.hasAttribute("disabled"));
const text = (b: HTMLElement = document.body) => b.textContent.trim();
/** The path chooser's button on the row named `path` (Set up, or Review once the path is done). */
const pathButton = (path: string) => must(screen.getByText(path).closest("div.flex")?.querySelector("button"), `the ${path} path's button`);

/** Presses the forward button (Get started, Continue, Skip this step) until there is none; the
 *  names of the steps' forward buttons, in order. */
async function walk(user: ReturnType<typeof userEvent.setup>, max = 14) {
  const pressed: string[] = [];
  for (let n = 0; n < max; n++) {
    const forward = enabled().find((b) => /^(get started|continue|next)/i.test(text(b)));
    if (!forward) break;
    pressed.push(text(forward));
    await user.click(forward);
    await flush();
  }
  return pressed;
}

const MACHINES: [string, SystemReport][] = [
  ["a ready Mac", readyReport()],
  ["a fresh Mac", freshReport("macos")],
  ["a fresh Windows PC", freshReport("windows")],
  ["a fresh Linux box", freshReport("linux")],
  ["a Linux box whose Docker is stopped", stoppedDockerReport()],
];

describe("MachineSetup window", () => {
  it("waits for the machine check", () => {
    installTauri();
    render(<MachineSetup report={null} onRefresh={vi.fn()} onClose={vi.fn()} />);
    expect(screen.queryByRole("heading")).toBeNull();
  });

  it("walks a ready machine's container path from where it stands to the end, and closes", async () => {
    installTauri();
    const user = userEvent.setup();
    const onClose = vi.fn();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={onClose} />);
    await flush();
    // The choice of path first: each lab type, where this machine stands on it.
    expect(screen.getByText("What do you want to run?")).toBeTruthy();
    await user.click(pathButton("Container labs"));
    await flush();
    // Package manager and engine are in: the path opens at the test, which runs on its own.
    expect(text()).toContain("Step 3 of 5");
    expect(text()).toContain("Test container labs");
    const pressed = await walk(user);
    expect(pressed).toEqual(["Continue", "Continue"]);
    expect(text()).toContain("You’re set up");
    await user.click(screen.getByRole("button", { name: /done|start/i }));
    expect(onClose).toHaveBeenCalled();
    // And back again.
    await user.click(screen.getByRole("button", { name: /back/i }));
    await flush();
  });

  it("a reopened setup on a machine with Docker installed and tested starts past the Docker steps", async () => {
    installTauri();
    setLastTest("docker", "ok");
    const user = userEvent.setup();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    // Nothing left on that path: its row says so, and Review goes straight to the summary.
    expect(pathButton("Container labs").textContent).toMatch(/review/i);
    await user.click(pathButton("Container labs"));
    await flush();
    expect(text()).toContain("You’re set up");
    expect(text()).toContain("Tested and ready to run.");
    expect(text()).not.toContain("Container engine");
    // Only the container path's outcome: VM labs were not this walk.
    expect(text()).not.toContain("VM labs");
    // Back still reaches the steps, done as they are.
    await user.click(screen.getByRole("button", { name: /back/i }));
    await flush();
    expect(text()).toContain("Step 4 of 5");
    expect(text()).toContain("Attack machine");
  });

  it("each path lists only its own steps", async () => {
    installTauri();
    const user = userEvent.setup();
    const report = freshReport("linux");
    const { rerender } = render(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    // The container path on a fresh box: the package manager, the engine, its test, the attack box.
    await user.click(pathButton("Container labs"));
    await flush();
    expect(text()).toContain("Step 1 of 5");
    expect(text()).not.toContain("Virtual machines");
    expect(text()).not.toContain("Cloud labs");
    // Its own steps, opened directly.
    rerender(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vm", nonce: 1 }} />);
    expect(text()).toContain("Step 2 of 5");
    expect(text()).toContain("Virtual machines");
    expect(text()).not.toContain("Container engine");
  });

  it("offers this machine's providers only: container labs and VM labs, nothing about the cloud", async () => {
    installTauri();
    render(<MachineSetup report={freshReport("macos")} onRefresh={vi.fn()} onClose={vi.fn()} />);
    await flush();
    expect(text()).toContain("Container labs");
    expect(text()).toContain("Containers on this machine, with the attack box next to them.");
    expect(text()).toContain("VM labs");
    expect(text()).not.toMatch(/cloud/i);
    expect(screen.getAllByRole("button", { name: /^set up$/i })).toHaveLength(2);
  });

  it("Add a provider lists what this Mac can run but doesn't have, and installs one with its log", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<MachineSetup report={freshReport("macos")} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Add a provider");
    // Not the lab-type guide.
    expect(text()).not.toContain("What do you want to run?");
    // This Mac's engines, none found; its hypervisors (an Intel Mac: VirtualBox and VMware, not UTM), none installed.
    expect(text()).toMatch(/Container engines.*Docker Desktop.*OrbStack.*Colima.*Hypervisors.*VirtualBox.*VMware Fusion/);
    expect(text()).not.toContain("UTM");
    // The recommended engine installs from here; the others link out.
    await user.click(button(/install docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "docker" });
    expect(text()).toContain("Installed");
    expect(button(/get orbstack/i)).toBeTruthy();
    await user.click(button(/install virtualbox/i));
    await flush();
    expect(calls.filter((c) => c.cmd === "install_dependency").at(-1)?.args).toMatchObject({ dependency: "virtualbox" });
  });

  it("Add a provider says so when everything this machine can run is installed", async () => {
    installTauri();
    const onClose = vi.fn();
    const user = userEvent.setup();
    const report = {
      ...readyReport(),
      dockerEngines: ["docker-desktop", "orbstack", "colima"].map((e) => ({ engine: e, context: e, running: true, current: e === "docker-desktop" })),
    };
    report.vmProviders = report.vmProviders.map((p) => (p.remote ? p : { ...p, hypervisor: true }));
    render(<MachineSetup report={report as SystemReport} onRefresh={vi.fn()} onClose={onClose} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Everything macOS can run is installed.");
    expect(screen.queryByRole("button", { name: /install|get /i })).toBeNull();
    await user.click(button(/^done$/i));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("an engine installed from the picker stays listed: its own test, then Use by default", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    const before = { ...readyReport(), dockerEngines: [{ engine: "docker-desktop", context: "desktop-linux", running: true, current: true }] } as SystemReport;
    const { rerender } = render(<MachineSetup report={before} onRefresh={onRefresh} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    expect(text()).toMatch(/OrbStack.*Colima/);
    expect(text()).not.toContain("Docker Desktop");
    // The machine check finds OrbStack (the player installed it from its site and re-checked).
    const after = {
      ...before,
      dockerEngines: [...before.dockerEngines, { engine: "orbstack" as const, context: "orbstack", running: true, current: false }],
    };
    rerender(<MachineSetup report={after} onRefresh={onRefresh} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Installed. Test it, then make it the default.");
    await user.click(button(/^test$/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "docker", context: "orbstack" });
    expect(getLastTest("docker", "orbstack")?.result).toBe("ok");
    // Not the current engine: the container labs' own record is untouched.
    expect(getLastTest("docker")).toBeNull();
    await user.click(button(/use by default/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_use_engine")?.args).toEqual({ engine: "orbstack" });
    expect(onRefresh).toHaveBeenCalled();
  });

  it("a hypervisor installed from the picker asks for Vagrant and its plugin, then tests and becomes the default", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const libvirt = hv({ provider: "libvirt", hypervisor: false, available: false, plugin: "vagrant-libvirt", pluginInstalled: false });
    const before = { ...freshReport("linux"), vmProviders: [libvirt] };
    const { rerender } = render(<MachineSetup report={before} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    await user.click(button(/install libvirt/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "libvirt" });
    // Installed: Vagrant and the plugin are next, nothing to test yet.
    const installed = { ...before, vmProviders: [{ ...libvirt, hypervisor: true }] };
    rerender(<MachineSetup report={installed} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    expect(button(/install vagrant/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^test$/i })).toBeNull();
    // Vagrant and the plugin in: the test, on that hypervisor, then the Settings choice.
    const ready = {
      ...installed,
      vagrant: { installed: true, version: "2.4.3" },
      vmProviders: [{ ...libvirt, hypervisor: true, available: true, pluginInstalled: true }],
    };
    rerender(<MachineSetup report={ready} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "add", nonce: 1 }} />);
    await flush();
    await user.click(button(/^test$/i));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_selftest")?.args).toMatchObject({ kind: "vm", provider: "libvirt" });
    await user.click(button(/use by default/i));
    expect(getVmProvider()).toBe("libvirt");
  });

  for (const [name, report] of MACHINES) {
    it(`shows what ${name} needs at each step`, async () => {
      const calls = installTauri({ system_check: () => report });
      const user = userEvent.setup();
      const onRefresh = vi.fn();
      for (const [n, step] of machineSteps(report).entries()) {
        const { unmount } = render(<MachineSetup report={report} onRefresh={onRefresh} onClose={vi.fn()} startAt={{ step, nonce: n + 1 }} />);
        await flush();
        await clickEverything(user, { skip: /^(back|continue|done)$/i, max: 10 });
        unmount();
      }
      expect(calls.length).toBeGreaterThan(0);
    });
  }

  it("opens at a requested step, on its path", async () => {
    installTauri();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Step 3 of 5");
    expect(text()).toContain("Provisioning");
  });
});

describe("Onboarding", () => {
  it("walks from welcome to done on a ready machine", async () => {
    installTauri();
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(<Onboarding onComplete={onComplete} />);
    await flush();
    const pressed = await walk(user);
    expect(pressed[0]).toMatch(/get started/i);
    await clickEverything(user, { skip: /skip setup|back/i, max: 10 });
    await flush();
    expect(onComplete).toHaveBeenCalled();
  });

  it("signs in from the sign-in step and reports a failed sign-in", async () => {
    let fail = true;
    installTauri({
      auth_status: () => ({ loggedIn: false, name: null, email: null }),
      auth_login: () => {
        if (fail) {
          fail = false;
          throw new Error("timed out waiting for the browser");
        }
        return { loggedIn: true, name: "Player", email: "p@example.com" };
      },
      system_check: () => freshReport("linux"),
    });
    const user = userEvent.setup();
    render(<Onboarding onComplete={vi.fn()} />);
    await flush();
    await user.click(screen.getByRole("button", { name: /get started/i }));
    await flush();
    const signIn = () =>
      must(
        enabled().find((b) => /sign in|log in/i.test(text(b))),
        "the sign-in button",
      );
    await user.click(signIn());
    await flush();
    expect(screen.getAllByText(/timed out/).length).toBeGreaterThan(0);
    await user.click(signIn());
    await flush();
    expect(screen.getAllByText(/Player/).length).toBeGreaterThan(0);
    await walk(user);
  });

  it("can be skipped", async () => {
    installTauri();
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(<Onboarding onComplete={onComplete} />);
    await user.click(screen.getByRole("button", { name: /skip/i }));
    expect(onComplete).toHaveBeenCalled();
  });
});
