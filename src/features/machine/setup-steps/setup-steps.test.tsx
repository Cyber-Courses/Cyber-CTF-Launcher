// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { MachineSetup } from "@/features/machine/machine-setup";
import { SetupChecklist } from "@/features/machine/setup-checklist";
import { MachineStepBody, SetupOutcome, useMachineSetup, type MachineStep, type MachineSetupState } from "@/features/machine/setup-steps";
import { wslState } from "@/features/machine/setup-steps/steps";
import { getAttackImage, getAutoAttackBox, setAttackImage } from "@/lib/settings";
import type { SystemReport, WslState } from "@/lib/tauri";
import { freshReport, hypervisorStatus as hv, readyReport } from "@/test/fixtures";
import { commands, installTauri, rejectWith, stream } from "@/test/tauri";
import { button, flush, settle } from "@/test/ui";
import { radioWith } from "@/features/machine/machine-test-kit";

const text = () => document.body.textContent;

/** One step body with a live setup state; `onSetup` sees each render's state. */
function Step({
  step,
  report,
  onRefresh = () => {},
  onSetup,
}: {
  step: MachineStep;
  report: SystemReport;
  onRefresh?: () => void;
  onSetup?: (s: MachineSetupState) => void;
}) {
  const setup = useMachineSetup(report, onRefresh);
  onSetup?.(setup);
  return (
    <>
      <MachineStepBody step={step} report={report} setup={setup} />
      <SetupOutcome report={report} setup={setup} />
    </>
  );
}

/* ------------------------------------------------------------------ package manager, WSL */

describe("package manager step", () => {
  it("shows an installed package manager", () => {
    installTauri();
    render(<Step step="pkgmgr" report={readyReport()} />);
    expect(text()).toContain("brew4.4.0");
  });

  it("warns Intel Mac players that Homebrew installs may compile from source, and nobody else", () => {
    const { unmount } = render(<Step step="pkgmgr" report={{ ...readyReport(), os: "macos", arch: "x86_64" }} />);
    expect(text()).toContain("Intel Mac");
    unmount();
    const arm = render(<Step step="pkgmgr" report={{ ...freshReport("macos"), arch: "aarch64" }} />);
    expect(text()).not.toContain("Intel Mac");
    arm.unmount();
    render(<Step step="pkgmgr" report={{ ...freshReport("linux"), arch: "x86_64" }} />);
    expect(text()).not.toContain("Intel Mac");
  });

  it("gives Homebrew's install line on a Mac, and copies it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const writeText = vi.fn<(s: string) => Promise<void>>(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<Step step="pkgmgr" report={freshReport("macos")} />);
    expect(text()).toContain("Run this in Terminal");
    await user.click(button(/^\s*copy$/i));
    await flush();
    expect(writeText.mock.calls[0][0]).toContain("Homebrew/install");
    expect(text()).toContain("Copied");
    await user.click(button(/brew\.sh/));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args).toMatchObject({ url: "https://brew.sh" });
  });

  it("links to App Installer on Windows and names the Linux package manager", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const r = render(<Step step="pkgmgr" report={freshReport("windows")} />);
    await user.click(button(/^\s*get$/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url).toMatch(/apps\.microsoft\.com/);
    r.unmount();
    const linux = render(<Step step="pkgmgr" report={freshReport("linux")} />);
    expect(text()).toContain("package manager (apt)");
    linux.unmount();
    render(<Step step="pkgmgr" report={{ ...freshReport("linux"), pkgManager: { name: "", installed: false, version: null } }} />);
    expect(text()).toContain("Install your distribution’s package manager to use");
  });
});

describe("WSL step", () => {
  it("turns on WSL, then asks for a restart", async () => {
    const calls = installTauri({ install_dependency: (a) => (stream(a, "Enabling feature"), null) });
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    render(<Step step="virtualization" report={freshReport("windows")} onRefresh={onRefresh} />);
    expect(screen.queryByRole("status")).toBeNull();
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "wsl" });
    expect(text()).toContain("Enabling feature");
    expect(screen.getByRole("status").textContent).toContain("Restart Windows to finish");
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("shows a failed WSL install in its log", async () => {
    installTauri({
      install_dependency: () => {
        rejectWith("elevation refused");
      },
    });
    const user = userEvent.setup();
    render(<Step step="virtualization" report={freshReport("windows")} />);
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(text()).toContain("✗ elevation refused");
  });
});

/** Windows, with the machine check's WSL answer. */
const windows = (state: WslState | null, reopensAfterRestart = true, virtualizationOff = false): SystemReport => ({
  ...freshReport("windows"),
  wsl: state ? { state, reopensAfterRestart, virtualizationOff } : null,
});

/** The setup window at its WSL step (the second, after the package manager). */
function SetupAtWsl({ report }: { report: SystemReport }) {
  return <MachineSetup report={report} onRefresh={() => {}} onClose={() => {}} startAt={{ step: "virtualization", nonce: 1 }} />;
}

describe("WSL step, from the machine check", () => {
  it("is simply done when WSL 2 is on", () => {
    installTauri();
    render(<SetupAtWsl report={windows("ready")} />);
    expect(text()).toContain("On and running");
    expect(screen.queryByRole("button", { name: /turn on wsl 2|restart now|done this/i })).toBeNull();
    expect(button(/^continue$/i)).toBeTruthy();
  });

  it("is done when Docker already runs, whatever the WSL check could tell (#224)", () => {
    // Windows 11 with Docker Desktop running on WSL 2: onboarding asked to turn WSL 2 on (a UAC
    // prompt and a restart for nothing) while the Machine checklist said it was done.
    for (const state of [null, "unknown", "missing"] as const) {
      installTauri();
      const report = { ...windows(state), docker: { ...windows(state).docker, installed: true }, dockerRunning: true };
      render(<SetupAtWsl report={report} />);
      expect(screen.queryByRole("button", { name: /turn on wsl 2|restart now|done this/i })).toBeNull();
      expect(button(/^continue$/i)).toBeTruthy();
      expect(wslState(report)).toBe("ready");
      cleanup();
    }
  });

  it("offers a restart when WSL 2 waits for one, with a way to restart later", () => {
    installTauri();
    render(<SetupAtWsl report={windows("needsReboot")} />);
    expect(screen.getByRole("status").textContent).toContain("Restart Windows to finish");
    expect(button(/restart now/i)).toBeTruthy();
    expect(button(/restart later/i).hasAttribute("disabled")).toBe(false);
    expect(screen.queryByRole("button", { name: /turn on wsl 2|done this/i })).toBeNull();
    // The manual way stays.
    expect(text()).toContain("Do it by hand instead");
  });

  it("offers to turn WSL 2 on when it is missing, or skip", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    render(<Step step="virtualization" report={windows("missing")} onRefresh={onRefresh} />);
    expect(button(/turn on wsl 2/i)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /restart now/i })).toBeNull();
    expect(text()).not.toContain("couldn’t tell");
    await user.click(button(/re-check/i));
    expect(onRefresh).toHaveBeenCalledOnce();
    expect(commands(calls)).not.toContain("machine_restart");
  });

  it("asks the player only when the check can't tell", () => {
    installTauri();
    const r = render(<SetupAtWsl report={windows("unknown")} />);
    expect(text()).toContain("couldn’t tell whether WSL 2 is on");
    expect(button(/i’ve done this/i)).toBeTruthy();
    r.unmount();
    render(<SetupAtWsl report={windows("missing")} />);
    expect(screen.queryByRole("button", { name: /i’ve done this/i })).toBeNull();
    expect(button(/skip for now/i)).toBeTruthy();
  });

  it("offers the restart once Turn on WSL 2 finished, whatever the last check said", async () => {
    installTauri({ install_dependency: (a) => (stream(a, "The requested operation is successful."), null) });
    const user = userEvent.setup();
    render(<SetupAtWsl report={windows("missing")} />);
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(button(/restart now/i)).toBeTruthy();
    expect(button(/restart later/i)).toBeTruthy();
    expect(text()).toContain("The requested operation is successful.");
  });

  it("a failed Turn on WSL 2 doesn't offer a restart", async () => {
    installTauri({
      install_dependency: () => {
        rejectWith("elevation refused");
      },
    });
    const user = userEvent.setup();
    render(<SetupAtWsl report={windows("missing")} />);
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(screen.queryByRole("button", { name: /restart now/i })).toBeNull();
  });

  it("restarts only after the player confirms, saying what happens", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="virtualization" report={windows("needsReboot")} />);
    await user.click(button(/restart now/i));
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.textContent).toContain("open apps close without saving");
    expect(dialog.textContent).toContain("Cyber CTF opens again when you sign back in");
    // Cancel keeps everything.
    await user.click(button(/cancel/i));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(commands(calls)).not.toContain("machine_restart");
    await user.click(button(/restart now/i));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: /restart now/i }));
    await flush();
    expect(calls.find((c) => c.cmd === "machine_restart")?.args).toEqual({ reason: "Cyber CTF: restarting to finish turning on WSL 2." });
    expect(screen.getByRole("status").textContent).toContain("Restarting in a few seconds");
    expect(button(/restart now/i).hasAttribute("disabled")).toBe(true);
  });

  it("says to reopen Cyber CTF by hand when it can't reopen itself, and shows a failed restart", async () => {
    installTauri({
      machine_restart: () => {
        rejectWith("Access is denied.");
      },
    });
    const user = userEvent.setup();
    render(<Step step="virtualization" report={windows("needsReboot", false)} />);
    await user.click(button(/restart now/i));
    expect(screen.getByRole("alertdialog").textContent).toContain("open Cyber CTF again");
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: /restart now/i }));
    await flush();
    // The button comes back, to try again.
    expect(button(/restart now/i).hasAttribute("disabled")).toBe(false);
    expect(screen.getByRole("status").textContent).not.toContain("Restarting");
  });
});

describe("WSL step, restarting once", () => {
  /** The step drawn afresh on each `round`, as the onboarding does, with one setup state. */
  function Remounting({ round, report }: { round: number; report: SystemReport }) {
    const setup = useMachineSetup(report, () => {});
    return <MachineStepBody key={round} step="virtualization" report={report} setup={setup} />;
  }

  it("a remounted step can't ask for a second restart during the countdown", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const report = windows("needsReboot");
    const r = render(<Remounting round={1} report={report} />);
    await user.click(button(/restart now/i));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: /restart now/i }));
    await flush();
    r.rerender(<Remounting round={2} report={report} />);
    expect(screen.getByRole("status").textContent).toContain("Restarting in a few seconds");
    expect(button(/restart now/i).hasAttribute("disabled")).toBe(true);
    expect(commands(calls).filter((c) => c === "machine_restart")).toHaveLength(1);
  });

  it("says virtualization looks off in the firmware, instead of asking for another restart", () => {
    installTauri();
    render(<SetupAtWsl report={windows("unknown", true, true)} />);
    expect(screen.getByRole("status").textContent).toContain("Virtualization is probably off in your PC’s firmware");
    expect(text()).not.toContain("couldn’t tell");
    expect(screen.queryByRole("button", { name: /restart now/i })).toBeNull();
    expect(button(/i’ve done this/i)).toBeTruthy();
  });
});

describe("guided setup checklist, WSL line", () => {
  // A fresh Windows machine: only the attack machine (a default) is done besides WSL.
  it.each([
    ["ready", "WSL 2", 2],
    ["needsReboot", "restart needed", 1],
    ["missing", "not installed", 1],
    [null, "not checked", 1],
  ] as const)("%s reads as %s", (state, meta, done) => {
    installTauri();
    render(<SetupChecklist report={windows(state)} last={{ docker: null, vm: null }} onFix={vi.fn()} />);
    expect(text()).toContain(`Enable virtualization (WSL 2)${meta}`);
    expect(text()).toContain(`${String(done)} of 8 done`);
  });
});

/* ------------------------------------------------------------------ engine */

/** The engine step on a fresh Mac, with its calls and its re-check. */
function freshMacEngineStep() {
  const calls = installTauri();
  const onRefresh = vi.fn();
  const user = userEvent.setup();
  render(<Step step="docker" report={freshReport("macos")} onRefresh={onRefresh} />);
  return { calls, onRefresh, user };
}

describe("engine step", () => {
  it("says nothing more when the chosen engine is in use", () => {
    installTauri();
    render(<Step step="docker" report={readyReport()} />);
    expect(radioWith("Docker Desktop").getAttribute("aria-checked")).toBe("true");
    expect(text()).toContain("in use");
    expect(screen.queryByRole("button", { name: /re-check|install|start|use /i })).toBeNull();
  });

  it("switches Docker to another running engine", async () => {
    const calls = installTauri();
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    render(<Step step="docker" report={{ ...readyReport(), dockerEnginesRunning: ["docker-desktop", "orbstack"] }} onRefresh={onRefresh} />);
    await user.click(radioWith("OrbStack"));
    expect(text()).toContain("OrbStack is running, but Docker uses Docker Desktop right now.");
    await user.click(button(/use orbstack/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_use_engine")?.args).toMatchObject({ engine: "orbstack" });
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("names another engine when Docker uses one without a tile", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<Step step="docker" report={{ ...readyReport(), dockerEngine: "podman", dockerEnginesRunning: ["podman", "colima"] }} />);
    await user.click(radioWith("Colima"));
    expect(text()).toContain("Docker uses another engine right now.");
  });

  it("starts a stopped engine, and offers its site when that fails", async () => {
    let fail = false;
    const calls = installTauri({
      docker_start_engine: () => {
        if (fail) rejectWith("Docker Desktop is not installed");
        return null;
      },
    });
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    render(<Step step="docker" report={{ ...readyReport(), dockerRunning: false, dockerEngine: null, dockerEnginesRunning: [] }} onRefresh={onRefresh} />);
    expect(text()).toContain("Docker Desktop is installed but not running.");
    await user.click(button(/start docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_start_engine")?.args).toMatchObject({ engine: "docker-desktop" });
    expect(onRefresh).toHaveBeenCalledOnce();
    fail = true;
    await user.click(button(/start docker desktop/i));
    await flush();
    expect(text()).toContain("Docker Desktop is not installed");
    await user.click(button(/get docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args).toMatchObject({ url: "https://www.docker.com/products/docker-desktop/" });
  });

  it("installs the recommended engine, then asks to finish in its installer", async () => {
    const { calls, onRefresh, user } = freshMacEngineStep();
    expect(text()).toContain("Cyber CTF can install Docker Desktop for you.");
    await user.click(button(/install docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "docker" });
    expect(text()).toContain("Finish in Docker’s installer");
    await user.click(button(/re-check/i));
    expect(onRefresh).toHaveBeenCalledTimes(2);
  });

  it("links out for an engine Cyber CTF does not install", async () => {
    const { calls, onRefresh, user } = freshMacEngineStep();
    await user.click(radioWith("Colima"));
    expect(text()).toContain("Install Colima, start it, then re-check.");
    await user.click(button(/get colima/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args).toMatchObject({ url: "https://github.com/abiosoft/colima" });
    await user.click(button(/re-check/i));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("explains the Linux post-install and the Windows reboot", async () => {
    installTauri();
    const user = userEvent.setup();
    const r = render(<Step step="docker" report={freshReport("linux")} />);
    expect(radioWith("Docker Engine").getAttribute("aria-checked")).toBe("true");
    expect(text()).toContain("sudo usermod -aG docker $USER");
    // Docker Desktop on Linux: no post-install for Docker Engine.
    await user.click(radioWith("Docker Desktop"));
    expect(text()).not.toContain("sudo systemctl enable --now docker");
    r.unmount();
    render(<Step step="docker" report={freshReport("windows")} />);
    expect(text()).toContain("A reboot may be needed after enabling WSL.");
  });

  it("asks to finish another engine's install after Cyber CTF opened its installer", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<Step step="docker" report={freshReport("linux")} />);
    await user.click(button(/install docker engine/i));
    await flush();
    expect(text()).toContain("Finish installing Docker Engine, start it, then press Re-check.");
  });
});

/* ------------------------------------------------------------------ attack box */

describe("attack box step", () => {
  it("picks an image, toggles the auto-start and warns without an engine", async () => {
    installTauri({ image_download_size: () => null });
    const user = userEvent.setup();
    render(<Step step="attack" report={freshReport("linux")} />);
    await flush();
    expect(text()).toContain("large download");
    expect(text()).toContain("It runs on your container engine");
    await user.click(radioWith("Parrot"));
    expect(getAttackImage()).toBe("parrotsec/security");
    await user.click(screen.getByRole("switch", { name: /start it with each lab/i }));
    expect(getAutoAttackBox()).toBe(false);
  });

  it("names a custom image from Settings", async () => {
    installTauri({ image_download_size: () => 1_200_000_000 });
    setAttackImage("ghcr.io/me/box");
    render(<Step step="attack" report={readyReport()} />);
    await flush();
    expect(text()).toContain("Using a custom image from Settings: ghcr.io/me/box");
    expect(text()).not.toContain("It runs on your container engine");
  });
});

/* ------------------------------------------------------------------ hypervisor and Vagrant */

describe("VM step", () => {
  it("says no local hypervisor applies", () => {
    installTauri();
    render(<Step step="vm" report={{ ...readyReport(), vmProviders: [hv({ provider: "proxmox", remote: true })] }} />);
    expect(text()).toContain("No local hypervisor applies to this machine.");
  });

  it("installs a hypervisor Cyber CTF can install", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="vm" report={{ ...freshReport("linux") }} />);
    await user.click(radioWith("UTM"));
    expect(text()).toContain("Cyber CTF can install UTM for you.");
    await user.click(radioWith("VirtualBox"));
    expect(text()).toContain("Cyber CTF can install VirtualBox for you.");
    await user.click(button(/install virtualbox/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "virtualbox" });
    expect(text()).toContain("Installing VirtualBox…");
  });

  it("links to a vendor's site, and re-checks", async () => {
    const calls = installTauri();
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    render(<Step step="vm" report={freshReport("windows")} onRefresh={onRefresh} />);
    await user.click(radioWith("VMware Workstation"));
    expect(text()).toContain("Install VMware Workstation, then re-check.");
    await user.click(button(/get vmware workstation/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url).toMatch(/vmware\.com/);
    await user.click(button(/re-check/i));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("says why an installed hypervisor can't run here", () => {
    installTauri();
    render(
      <Step
        step="vm"
        report={{ ...readyReport(), os: "linux", vmProviders: [hv({ provider: "libvirt", available: false, reason: "not in the kvm group" })] }}
      />,
    );
    expect(text()).toContain("Installed, but it can't run here.");
    expect(text()).toContain("Not in the kvm group.");
  });
});

describe("Vagrant step", () => {
  it("says there is nothing for Vagrant to drive without a local hypervisor", () => {
    installTauri();
    render(<Step step="vagrant" report={{ ...readyReport(), vmProviders: [] }} />);
    expect(text()).toContain("nothing for Vagrant to drive here");
  });

  it("installs Vagrant, then the hypervisor's plugin", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const report: SystemReport = {
      ...readyReport(),
      os: "windows",
      vagrant: { installed: false, version: null },
      vmProviders: [hv({ provider: "vmware_desktop", available: false, plugin: "vagrant-vmware-desktop", pluginInstalled: false })],
    };
    const r = render(<Step step="vagrant" report={report} />);
    expect(text()).toContain("vagrant-vmware-desktop, once Vagrant is installed.");
    expect(button(/install plugin/i).hasAttribute("disabled")).toBe(true);
    expect(text()).toContain("Vagrant VMware Utility");
    await user.click(button(/install vagrant/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "vagrant" });
    await user.click(button(/^\s*get$/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:opener|open_url")?.args?.url).toMatch(/vagrant\/install\/vmware/);
    r.unmount();
    render(<Step step="vagrant" report={{ ...report, vagrant: { installed: true, version: null } }} />);
    expect(text()).toContain("Installed");
    await user.click(button(/install plugin/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_vagrant_plugin")?.args).toMatchObject({ plugin: "vagrant-vmware-desktop" });
    expect(text()).toContain("Installing the Vagrant plugin vagrant-vmware-desktop…");
  });

  it("shows a failed plugin install", async () => {
    installTauri({
      install_vagrant_plugin: () => {
        rejectWith("network unreachable");
      },
    });
    const user = userEvent.setup();
    render(
      <Step
        step="vagrant"
        report={{ ...readyReport(), os: "linux", vmProviders: [hv({ provider: "qemu", available: false, plugin: "vagrant-qemu", pluginInstalled: false })] }}
      />,
    );
    await user.click(button(/install plugin/i));
    await flush();
    expect(text()).toContain("✗ network unreachable");
  });
});

/* ------------------------------------------------------------------ tests and the outcome */

describe("test steps and the outcome", () => {
  it("skips the tests on a machine with nothing to test", () => {
    installTauri();
    const r = render(<Step step="docker-test" report={freshReport("linux")} />);
    expect(text()).toContain("No container engine is running.");
    expect(text()).toContain("Set up a container engine to run them.");
    expect(text()).toContain("Install a hypervisor to run them.");
    r.unmount();
    render(<Step step="vm-test" report={freshReport("linux")} />);
    expect(text()).toContain("No hypervisor is installed.");
  });

  it("runs the container test and reports it in the outcome", async () => {
    installTauri();
    render(<Step step="docker-test" report={readyReport()} />);
    await flush();
    expect(text()).toContain("Tested and ready to run.");
    expect(text()).toContain("Not tested.");
  });

  it("reports a failed VM test in the outcome", async () => {
    installTauri({
      machine_selftest: () => {
        rejectWith("boot timed out");
      },
    });
    render(<Step step="vm-test" report={{ ...readyReport(), os: "linux" }} />);
    await flush();
    expect(text()).toContain("The test failed. Go back to run it again.");
    expect(text()).toContain("Engine running, not tested.");
  });

  it("prefetches each test's download once the machine can run it", async () => {
    const calls = installTauri();
    const { rerender } = render(<Step step="pkgmgr" report={freshReport("linux")} />);
    await flush();
    expect(commands(calls)).not.toContain("machine_selftest_prefetch");
    rerender(<Step step="pkgmgr" report={{ ...readyReport(), os: "linux" }} />);
    await flush();
    rerender(<Step step="pkgmgr" report={{ ...readyReport(), os: "linux", cpu: 1 } as SystemReport} />);
    await flush();
    const prefetch = calls.filter((c) => c.cmd === "machine_selftest_prefetch").map((c) => c.args?.kind);
    expect(prefetch).toEqual(["docker", "vm"]);
  });

  it("outcome works before the machine check answers", () => {
    installTauri();
    function Outcome() {
      const setup = useMachineSetup(null, () => {});
      return <SetupOutcome report={null} setup={setup} />;
    }
    render(<Outcome />);
    expect(text()).toContain("Set up a container engine to run them.");
  });
});

/* ------------------------------------------------------------------ setup window */

describe("MachineSetup window", () => {
  it("names the OS in its lead and ignores an unknown start step", async () => {
    installTauri();
    for (const [os, name] of [
      ["windows", "Windows"],
      ["linux", "Linux"],
      ["macos", "macOS"],
    ] as const) {
      const r = render(<MachineSetup report={{ ...readyReport(), os }} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "nope", nonce: 1 }} />);
      await flush();
      expect(text()).toContain(name);
      expect(text()).toMatch(/1\D+(8|9)/);
      r.unmount();
    }
  });

  it("holds the flow while an install runs", async () => {
    let finish: () => void = () => {};
    installTauri({
      install_dependency: () =>
        new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        ),
    });
    const user = userEvent.setup();
    render(<MachineSetup report={freshReport("macos")} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "docker", nonce: 1 }} />);
    await flush();
    await user.click(button(/install docker desktop/i));
    await flush();
    expect(button(/back/i).hasAttribute("disabled")).toBe(true);
    await settle(() => {
      finish();
    });
    await flush();
    expect(button(/back/i).hasAttribute("disabled")).toBe(false);
  });
});

describe("one jewel per view", () => {
  const jewels = () => Array.from(document.querySelectorAll(".btn-jewel"));

  it("a VM step with no hypervisor offers Skip for now as ghost glass, its Install as the jewel", async () => {
    installTauri();
    render(<MachineSetup report={freshReport("linux")} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vm", nonce: 1 }} />);
    await flush();
    expect(button(/skip for now/i).className).toContain("btn-glass");
    expect(jewels()).toHaveLength(1);
  });

  it("the WSL step's I've done this is ghost glass beside Turn on WSL 2", async () => {
    installTauri();
    render(<SetupAtWsl report={windows("unknown")} />);
    await flush();
    expect(button(/done this/i).className).toContain("btn-glass");
    expect(jewels().length).toBeLessThanOrEqual(1);
  });

  it("the Vagrant step has one jewel: Vagrant first, its plugin only once Vagrant is in", async () => {
    installTauri();
    const libvirt = hv({ provider: "libvirt", plugin: "vagrant-libvirt", pluginInstalled: false });
    const report = { ...freshReport("linux"), vmProviders: [libvirt] };
    const r = render(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    expect(jewels().map((b) => b.textContent)).toEqual(["Install Vagrant"]);
    r.unmount();
    render(
      <MachineSetup
        report={{ ...report, vagrant: { installed: true, version: "2.4.3" } }}
        onRefresh={vi.fn()}
        onClose={vi.fn()}
        startAt={{ step: "vagrant", nonce: 1 }}
      />,
    );
    await flush();
    expect(jewels().map((b) => b.textContent)).toEqual(["Install plugin"]);
  });
});
