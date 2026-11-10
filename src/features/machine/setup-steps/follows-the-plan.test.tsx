// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { MachineStepBody, useMachineSetup, type MachineStep } from "@/features/machine/setup-steps";
import { providerLabel } from "@/features/machine/hypervisors";
import { radioWith } from "@/features/machine/machine-test-kit";
import type { SetupAction, SystemReport } from "@/lib/tauri";
import { SETUP_SHAPES } from "@/lib/tauri/generated/SetupShapes";
import { answersDialog, commands, dialogsAsked, installTauri } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// The setup window renders the core's plan: each machine here is one the core built, with the
// setup options it worked out for it (Rust's `machine::setup::shapes`, written by `cargo test`), so
// what the window offers is held to the core's plan itself, not to a stand-in of it.

const text = () => document.body.textContent;

type Shape = keyof typeof SETUP_SHAPES;
const shape = (name: Shape): SystemReport => structuredClone(SETUP_SHAPES[name]);

function Step({ step, report }: { step: MachineStep; report: SystemReport }) {
  const setup = useMachineSetup(report, () => {});
  return <MachineStepBody step={step} report={report} setup={setup} />;
}

describe("the setup window follows the core's plan", () => {
  it("turns WSL 2 on on a bare Windows, then offers the restart the plan asks for", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="virtualization" report={shape("bareWindows")} />);
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "wsl" });
    cleanup();
    render(<Step step="virtualization" report={shape("windowsWslWaitsForRestart")} />);
    expect(text()).toContain("Restart Windows to finish turning it on.");
    expect(button(/restart now/i)).toBeTruthy();
  });

  it("installs the engine the plan installs, starts the one it starts, switches to the one it switches to", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="docker" report={shape("bareDebian")} />);
    await user.click(button(/install docker engine/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "docker" });
    cleanup();
    render(<Step step="docker" report={shape("windowsDockerStopped")} />);
    await user.click(button(/start docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_start_engine")?.args).toMatchObject({ engine: "docker-desktop" });
    cleanup();
    render(<Step step="docker" report={shape("macTwoEngines")} />);
    await user.click(radioWith("Docker Desktop"));
    await user.click(button(/use docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_use_engine")?.args).toMatchObject({ engine: "docker-desktop" });
  });

  it("on a Linux without apt, asks the player to install each tool themselves instead of offering a failing install", async () => {
    installTauri();
    const user = userEvent.setup();
    const fedora = shape("bareFedora");
    render(<Step step="docker" report={fedora} />);
    expect(text()).toContain("This distribution isn't supported by the guided setup yet: install Docker Engine yourself, then re-check.");
    expect(screen.queryByRole("button", { name: /install docker engine/i })).toBeNull();
    cleanup();
    render(<Step step="vm" report={fedora} />);
    await user.click(radioWith("VirtualBox"));
    expect(radioWith("VirtualBox").textContent).toContain("Install it yourself.");
    expect(text()).toContain("install VirtualBox yourself, then re-check.");
    expect(screen.queryByRole("button", { name: /install virtualbox/i })).toBeNull();
    expect(button(/get virtualbox/i)).toBeTruthy();
  });

  it("turns Hyper-V on, which the plan offers instead of a link, then asks for the restart that starts it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="vm" report={shape("bareWindows")} />);
    await user.click(radioWith("Hyper-V"));
    await user.click(button(/install hyper-v/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "hyperv" });
    cleanup();
    render(<Step step="vm" report={shape("windowsHypervWaitsForRestart")} />);
    await user.click(radioWith("Hyper-V"));
    expect(text()).toContain("Hyper-V is turned on. Restart Windows to finish turning it on.");
    await user.click(button(/restart now/i));
    await flush();
    // Asked in the system's own dialog first, as WSL 2's restart is.
    expect(dialogsAsked(calls)[0]?.message).toContain("open apps close without saving");
    expect(calls.find((c) => c.cmd === "machine_restart")?.args).toEqual({ reason: "Cyber CTF: restarting to finish turning on Hyper-V.", feature: "hyperv" });
  });

  it("restarts for Hyper-V only on the system dialog's yes", async () => {
    const calls = installTauri({ "plugin:dialog|message": answersDialog(false) });
    const user = userEvent.setup();
    render(<Step step="vm" report={shape("windowsHypervWaitsForRestart")} />);
    await user.click(radioWith("Hyper-V"));
    await user.click(button(/restart now/i));
    await flush();
    expect(dialogsAsked(calls)).toHaveLength(1);
    expect(commands(calls)).not.toContain("machine_restart");
  });

  it("says libvirt waits for a new login once the player joined its group, as the plan does", () => {
    installTauri();
    render(<Step step="vm" report={shape("debianLibvirtAwaitsRelogin")} />);
    expect(text()).toContain("You were added to the libvirt group since you logged in: log out and back in (or restart) for it to apply.");
  });

  it("says why an installed hypervisor can't run, as the plan does", () => {
    installTauri();
    render(<Step step="vm" report={shape("debianLibvirtWithoutKvm")} />);
    expect(text()).toContain("KVM isn't available on this machine (no /dev/kvm)");
    expect(radioWith("libvirt").textContent).toContain("Installed, but it can't run here.");
  });

  it("offers what Vagrant needs as the plan lists it: Vagrant first, then the plugin and VMware's utility", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="vagrant" report={shape("macDockerWithoutVagrant")} />);
    expect(Array.from(document.querySelectorAll(".btn-jewel")).map((b) => b.textContent)).toEqual(["Install Vagrant"]);
    cleanup();
    render(<Step step="vagrant" report={shape("macVmwareWithoutItsParts")} />);
    // The plan's own hypervisor is VMware here (installed), its two parts left.
    expect(document.querySelectorAll('[data-requirement="todo"]')).toHaveLength(2);
    await user.click(button(/install plugin/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_vagrant_plugin")?.args).toMatchObject({ plugin: "vagrant-vmware-desktop" });
    await user.click(button(/^install$/i));
    await flush();
    expect(calls.filter((c) => c.cmd === "install_dependency").at(-1)?.args).toMatchObject({ dependency: "vmware-utility" });
  });

  // Every hypervisor of every machine the core built: the VM step offers what the plan says gets it.
  for (const [name, report] of Object.entries(SETUP_SHAPES)) {
    it(`offers each hypervisor as the plan gets it on ${name}`, async () => {
      installTauri();
      const user = userEvent.setup();
      render(<Step step="vm" report={structuredClone(report)} />);
      for (const option of report.setup.hypervisors) {
        const status = report.vmProviders.find((p) => p.provider === option.provider);
        if (!status) throw new Error(`${option.provider} has no status`);
        const label = providerLabel(status, report.os);
        await user.click(radioWith(label));
        expect(offered(label), `${name}: ${label}`).toBe(expected(option.get[0], label));
      }
    });
  }
});

/** What the VM step offers for the hypervisor `label` right now. */
function offered(label: string): string {
  if (screen.queryByRole("button", { name: new RegExp(`^install ${label}$`, "i") })) return "install";
  if (text().includes("Restart Windows to finish turning it on.")) return "restart";
  if (text().includes(`install ${label} yourself`)) return "distribution";
  if (text().includes(`Install ${label}, then re-check.`)) return "vendor";
  if (text().includes("log out and back in (or restart) for it to apply")) return "relogin";
  if (text().includes("Installed, but it can't run here.")) return "blocked";
  return "nothing";
}

/** What the plan's first action for a hypervisor makes the VM step offer. */
function expected(action: SetupAction | undefined, label: string): string {
  if (!action) return "nothing";
  if (action.kind === "manual") return action.reason;
  if (action.kind === "install" || action.kind === "restart" || action.kind === "blocked" || action.kind === "relogin") return action.kind;
  throw new Error(`${label}: a hypervisor gets no ${action.kind}`);
}
