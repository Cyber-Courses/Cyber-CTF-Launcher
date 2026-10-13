// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import { listen } from "@tauri-apps/api/event";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/features/settings/about-section";
import { AccountSection } from "@/features/settings/account-section";
import { AppearanceSection } from "@/features/settings/appearance-section";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { HypervisorRow } from "@/features/settings/hypervisor-row";
import { InstalledToolsPanel } from "@/features/settings/installed-tools-panel";
import { LabPortsRow } from "@/features/settings/lab-ports-row";
import { SharedFolderControl } from "@/features/settings/shared-folder";
import { ONBOARDED_KEY } from "@/features/settings/settings-screen";
import { getLocalePreference, setLocalePreference } from "@/lib/i18n";
import { getAttackBox, getAttackImage, getAutoAttackBox, getPortMode, getVmProvider, setAttackBox, setAttackImage, setVmProvider } from "@/lib/settings";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { installTauri, stream } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// A plain function, not a `vi.fn`: a spy's bookkeeping on a rejected answer fails the test.
const updater = vi.hoisted(() => ({ calls: 0, answer: (): Promise<unknown> => Promise.resolve(null) }));
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: () => {
    updater.calls++;
    return updater.answer();
  },
}));

const text = () => document.body.textContent ?? "";
const radioWith = (s: string) => screen.getAllByRole("radio").find((r) => r.textContent?.includes(s))!;

describe("AboutSection", () => {
  beforeEach(() => {
    updater.calls = 0;
    updater.answer = () => Promise.resolve(null);
  });

  it("shows the version, the system and the install id", () => {
    installTauri();
    render(<AboutSection version="1.4.0" report={readyReport()} agent={{ installId: "c0ffee", name: "mbp", arch: "arm64", capabilities: [] }} />);
    expect(text()).toContain("v1.4.0");
    expect(text()).toContain("macos arm64");
    expect(text()).toContain("c0ffee");
    expect(text()).toMatch(/install automatically/);
  });

  it("shows a placeholder without a version, report or agent", () => {
    installTauri();
    render(<AboutSection version={null} report={null} agent={null} />);
    expect(text()).toContain("…");
    expect(text()).not.toMatch(/System|Install ID/);
  });

  it("says when the app is up to date, with the time of the check", async () => {
    installTauri();
    updater.answer = async () => null;
    const user = userEvent.setup();
    render(<AboutSection version="1.4.0" report={null} agent={null} />);
    await user.click(button(/check for updates/i));
    await flush();
    expect(updater.calls).toBe(1);
    expect(text()).toMatch(/latest version, checked at/);
  });

  it("says when the update server can't be reached", async () => {
    installTauri();
    updater.answer = async () => {
      throw new Error("offline");
    };
    const user = userEvent.setup();
    render(<AboutSection version="1.4.0" report={null} agent={null} />);
    await user.click(button(/check for updates/i));
    await flush();
    expect(text()).toMatch(/Couldn’t reach the update server/);
    // Retry stays possible.
    expect(button(/check for updates/i).hasAttribute("disabled")).toBe(false);
  });

  it("installs an available update and restarts", async () => {
    const calls = installTauri();
    const downloadAndInstall = vi.fn(async () => {});
    updater.answer = async () => ({ version: "1.5.0", downloadAndInstall });
    const user = userEvent.setup();
    render(<AboutSection version="1.4.0" report={null} agent={null} />);
    await user.click(button(/check for updates/i));
    await flush();
    expect(text()).toContain("Version 1.5.0 is available.");
    expect(text()).toContain("Update available");
    await user.click(button(/install update/i));
    await flush();
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(text()).toMatch(/Version 1.5.0 is installed/);
    expect(calls.filter((c) => c.cmd === "restart_app")).toHaveLength(1);
    // The fallback restarts again.
    await user.click(button(/restart now/i));
    await flush();
    expect(calls.filter((c) => c.cmd === "restart_app")).toHaveLength(2);
  });

  it("offers the update again when its install fails", async () => {
    const calls = installTauri();
    updater.answer = async () => ({
      version: "1.5.0",
      downloadAndInstall: async () => {
        throw new Error("bad signature");
      },
    });
    const user = userEvent.setup();
    render(<AboutSection version="1.4.0" report={null} agent={null} />);
    await user.click(button(/check for updates/i));
    await flush();
    await user.click(button(/install update/i));
    await flush();
    expect(text()).toContain("Version 1.5.0 is available.");
    expect(button(/install update/i).hasAttribute("disabled")).toBe(false);
    expect(calls.map((c) => c.cmd)).not.toContain("restart_app");
  });

  it("copies English diagnostics for a bug report", async () => {
    installTauri();
    const writeText = vi.fn<(s: string) => Promise<void>>(async () => {});
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    setAttackImage("kalilinux/kali-rolling");
    setVmProvider("virtualbox");
    const report: SystemReport = { ...readyReport(), dockerEngine: null };
    render(<AboutSection version="1.4.0" report={report} agent={{ installId: "c0ffee", name: "mbp", arch: "arm64", capabilities: [] }} />);
    await user.click(button(/^\s*copy$/i));
    await flush();
    const copied = writeText.mock.calls[0][0];
    expect(copied).toContain("Cyber CTF v1.4.0");
    expect(copied).toContain("OS: macos arm64");
    expect(copied).toContain("Docker: running (unknown engine)");
    expect(copied).toContain("Vagrant: installed, Terraform: installed");
    expect(copied).toContain("Hypervisors ready: virtualbox");
    expect(copied).toContain("Attack box: kalilinux/kali-rolling");
    expect(copied).toContain("VM provider: virtualbox");
    expect(copied).toContain("Install ID: c0ffee");
    expect(text()).toContain("Copied");
  });

  it("copies what it knows on a machine without a report or sign-in", async () => {
    installTauri();
    const writeText = vi.fn<(s: string) => Promise<void>>(async () => {});
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<AboutSection version={null} report={null} agent={null} />);
    await user.click(button(/^\s*copy$/i));
    await flush();
    const copied = writeText.mock.calls[0][0];
    expect(copied).toContain("Cyber CTF (unknown version)");
    expect(copied).not.toContain("OS:");
    expect(copied).toContain("VM provider: automatic");
    expect(copied).toContain("Not signed in");
  });

  it("describes a stopped or missing Docker and no hypervisor in the diagnostics", async () => {
    installTauri();
    const writeText = vi.fn<(s: string) => Promise<void>>(async () => {});
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const base = readyReport();
    const stopped: SystemReport = {
      ...base,
      dockerRunning: false,
      vagrant: { installed: false, version: null },
      terraform: { installed: false, version: null },
      vmProviders: [],
    };
    const { unmount } = render(<AboutSection version="1" report={stopped} agent={null} />);
    await user.click(button(/^\s*copy$/i));
    await flush();
    expect(writeText.mock.calls[0][0]).toContain("Docker: installed, not running");
    expect(writeText.mock.calls[0][0]).toContain("Vagrant: not installed, Terraform: not installed");
    expect(writeText.mock.calls[0][0]).toContain("Hypervisors ready: none");
    unmount();
    render(<AboutSection version="1" report={{ ...stopped, docker: { installed: false, version: null } }} agent={null} />);
    await user.click(button(/^\s*copy$/i));
    await flush();
    expect(writeText.mock.calls[1][0]).toContain("Docker: not installed");
  });

  it("stays quiet when the clipboard is unavailable", async () => {
    installTauri();
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn(async () => Promise.reject(new Error("denied"))) }, configurable: true });
    render(<AboutSection version="1" report={null} agent={null} />);
    await user.click(button(/^\s*copy$/i));
    await flush();
    expect(text()).not.toContain("Copied");
  });

  it("replays the onboarding in the main window", async () => {
    installTauri();
    const heard = vi.fn();
    await listen("cyberctf:replay-onboarding", heard);
    localStorage.setItem(ONBOARDED_KEY, "1");
    const user = userEvent.setup();
    render(<AboutSection version="1" report={null} agent={null} />);
    await user.click(button(/replay/i));
    await flush();
    expect(localStorage.getItem(ONBOARDED_KEY)).toBeNull();
    expect(heard).toHaveBeenCalledOnce();
  });
});

/* ------------------------------------------------------------------ hypervisor */

const hv = (over: Partial<ProviderStatus>): ProviderStatus =>
  ({ provider: "virtualbox", remote: false, available: true, hypervisor: true, plugin: null, pluginInstalled: true, reason: null, ...over }) as ProviderStatus;
const withProviders = (vmProviders: ProviderStatus[], over: Partial<SystemReport> = {}): SystemReport => ({ ...readyReport(), vmProviders, ...over });

describe("HypervisorRow", () => {
  const props = { onSaved: vi.fn(), onNavigate: vi.fn(), onRefresh: vi.fn() };
  beforeEach(() => {
    props.onSaved.mockReset();
    props.onNavigate.mockReset();
    props.onRefresh.mockReset();
  });

  it("says it is checking until the report is in", () => {
    installTauri();
    render(<HypervisorRow report={null} {...props} />);
    expect(text()).toContain("Checking hypervisors…");
  });

  it("points to the Machine page when no hypervisor is installed", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<HypervisorRow report={withProviders([hv({ hypervisor: false, available: false }), hv({ provider: "proxmox", remote: true })])} {...props} />);
    expect(text()).toContain("No hypervisor is installed");
    await user.click(button(/set one up/i));
    expect(props.onNavigate).toHaveBeenCalledWith("machine");
  });

  it("shows a single ready hypervisor as selected, without Automatic", () => {
    installTauri();
    render(<HypervisorRow report={withProviders([hv({})])} {...props} />);
    expect(text()).toContain("VM labs and the VM test run on VirtualBox.");
    expect(text()).not.toContain("Automatic");
    expect(text()).toContain("Ready · built into Vagrant");
    expect(screen.getByRole("radio").getAttribute("aria-checked")).toBe("true");
  });

  it("picks a hypervisor among several, and goes back to automatic", async () => {
    installTauri();
    const user = userEvent.setup();
    const report = withProviders([hv({}), hv({ provider: "vmware_desktop", plugin: "vagrant-vmware-desktop" })], { os: "windows" });
    render(<HypervisorRow report={report} {...props} />);
    expect(text()).toContain("Uses VirtualBox, the first ready hypervisor.");
    expect(text()).toContain("Ready · Vagrant add-on vagrant-vmware-desktop");
    const radios = () => screen.getAllByRole("radio");
    expect(radios()[0].getAttribute("aria-checked")).toBe("true");
    await user.click(screen.getByRole("radio", { name: /VMware Workstation/ }));
    expect(getVmProvider()).toBe("vmware_desktop");
    expect(props.onSaved).toHaveBeenCalledOnce();
    expect(text()).toContain("run on VMware Workstation");
    await user.click(screen.getByRole("radio", { name: /automatic/i }));
    expect(getVmProvider()).toBeNull();
    expect(text()).toContain("run on VirtualBox");
  });

  it("falls back to automatic when the saved hypervisor is no longer ready", () => {
    installTauri();
    setVmProvider("parallels");
    render(<HypervisorRow report={withProviders([hv({}), hv({ provider: "qemu" })])} {...props} />);
    expect(screen.getByRole("radio", { name: /automatic/i }).getAttribute("aria-checked")).toBe("true");
  });

  it("installs a missing Vagrant add-on, streaming its log, then re-checks", async () => {
    const calls = installTauri({ install_vagrant_plugin: (a) => (stream(a, "Installing the 'vagrant-vmware-desktop' plugin"), null) });
    const user = userEvent.setup();
    const report = withProviders([hv({ provider: "vmware_desktop", available: false, plugin: "vagrant-vmware-desktop", pluginInstalled: false })]);
    render(<HypervisorRow report={report} {...props} />);
    expect(text()).toContain("None of the installed hypervisors can run VM labs yet.");
    expect(text()).toContain("Installed · needs the Vagrant add-on vagrant-vmware-desktop");
    expect(text()).toContain("VMware Fusion");
    await user.click(button(/install add-on/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_vagrant_plugin")?.args).toMatchObject({ plugin: "vagrant-vmware-desktop" });
    expect(text()).toContain("Installing the 'vagrant-vmware-desktop' plugin");
    expect(text()).toContain("✓ Installed");
    expect(props.onRefresh).toHaveBeenCalledOnce();
  });

  it("shows a failed add-on install in the log", async () => {
    installTauri({
      install_vagrant_plugin: () => {
        throw "gem install failed";
      },
    });
    const user = userEvent.setup();
    render(<HypervisorRow report={withProviders([hv({ provider: "qemu", available: false, plugin: "vagrant-qemu", pluginInstalled: false })])} {...props} />);
    await user.click(button(/install add-on/i));
    await flush();
    expect(text()).toContain("✗ gem install failed");
    expect(props.onRefresh).toHaveBeenCalledOnce();
  });

  it("can't install an add-on before Vagrant, and explains other not-ready hypervisors", () => {
    installTauri();
    const report = withProviders(
      [
        hv({ provider: "qemu", available: false, plugin: "vagrant-qemu", pluginInstalled: false }),
        hv({ provider: "libvirt", available: false, reason: "no /dev/kvm" }),
        hv({ provider: "hyperv", available: false }),
      ],
      { vagrant: { installed: false, version: null } },
    );
    render(<HypervisorRow report={report} {...props} />);
    expect(button(/install add-on/i).hasAttribute("disabled")).toBe(true);
    expect(text()).toContain("no /dev/kvm");
    expect(text()).toContain("Installed · not ready for VM labs");
  });
});

/* ------------------------------------------------------------------ attack box */

describe("AttackBoxRows", () => {
  it("picks a preset image and saves it", async () => {
    installTauri({ image_download_size: () => 2_500_000_000 });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AttackBoxRows onSaved={onSaved} />);
    await flush();
    expect(text()).toContain("2.5 GB");
    await user.click(radioWith("kalilinux/kali-rolling"));
    expect(getAttackImage()).toBe("kalilinux/kali-rolling");
    expect(onSaved).toHaveBeenCalled();
  });

  it("shows the pinned default by its repository, not its digest", async () => {
    installTauri();
    render(<AttackBoxRows onSaved={vi.fn()} />);
    await flush();
    expect(text()).toContain("cyberctf/attack-box (pinned)");
    expect(text()).not.toContain("sha256:");
    expect(screen.getByTitle(/^cyberctf\/attack-box@sha256:/)).toBeTruthy();
  });

  it("saves a custom image only when it changed", async () => {
    installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AttackBoxRows onSaved={onSaved} />);
    await flush();
    await user.click(screen.getByRole("radio", { name: /custom image/i }));
    const save = () => screen.getAllByRole("button", { name: /^save$/i })[0];
    expect(save().hasAttribute("disabled")).toBe(true);
    await user.type(screen.getByPlaceholderText("registry/image:tag"), "  ghcr.io/me/box:1  ");
    await user.click(save());
    expect(getAttackImage()).toBe("ghcr.io/me/box:1");
    expect(onSaved).toHaveBeenCalledOnce();
    expect(save().hasAttribute("disabled")).toBe(true);
  });

  it("opens with the custom field filled when the saved image is not a preset", () => {
    installTauri();
    setAttackImage("ghcr.io/me/box:1");
    setAttackBox("me/box");
    render(<AttackBoxRows onSaved={vi.fn()} />);
    expect(screen.getByDisplayValue("ghcr.io/me/box:1")).toBeTruthy();
    expect(screen.getByDisplayValue("me/box")).toBeTruthy();
  });

  it("picks an attack VM box, or a custom one", async () => {
    installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AttackBoxRows onSaved={onSaved} />);
    await flush();
    await user.click(radioWith("generic/debian12"));
    expect(getAttackBox()).toBe("generic/debian12");
    await user.click(screen.getByRole("radio", { name: /custom box/i }));
    await user.type(screen.getByPlaceholderText("owner/name"), "me/kali{Enter}");
    expect(getAttackBox()).toBe("me/kali");
    expect(onSaved).toHaveBeenCalledTimes(2);
  });

  it("turns the auto-start off and on", async () => {
    installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AttackBoxRows onSaved={onSaved} />);
    await flush();
    const toggle = screen.getByRole("switch", { name: /start the attack box/i });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    await user.click(toggle);
    expect(getAutoAttackBox()).toBe(false);
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await user.click(toggle);
    expect(getAutoAttackBox()).toBe(true);
    expect(onSaved).toHaveBeenCalledTimes(2);
  });
});

/* ------------------------------------------------------------------ shared folder */

describe("SharedFolderControl", () => {
  it("renders nothing until the setting is read", () => {
    installTauri({ shared_folder_get: () => new Promise(() => {}) });
    const { container } = render(<SharedFolderControl />);
    expect(container.textContent).toBe("");
  });

  it("turns sharing on, showing the folder and its mount point", async () => {
    const calls = installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<SharedFolderControl onSaved={onSaved} />);
    await flush();
    expect(text()).toContain("/shared");
    expect(text()).not.toContain("/Users/player/CyberCTF");
    await user.click(screen.getByRole("switch"));
    await flush();
    expect(calls.find((c) => c.cmd === "shared_folder_set")?.args).toEqual({ enabled: true, path: "/Users/player/CyberCTF" });
    expect(text()).toContain("/Users/player/CyberCTF");
    expect(onSaved).toHaveBeenCalledOnce();
  });

  it("changes the folder from the picker, and keeps it when the picker is cancelled", async () => {
    let picked: string | null = "/Users/player/loot";
    const calls = installTauri({
      shared_folder_get: () => ({ enabled: true, path: "/Users/player/CyberCTF", mountPoint: "/shared" }),
      "plugin:dialog|open": () => picked,
    });
    const user = userEvent.setup();
    render(<SharedFolderControl framed />);
    await flush();
    await user.click(button(/change/i));
    await flush();
    expect(calls.find((c) => c.cmd === "plugin:dialog|open")?.args).toMatchObject({ options: { directory: true, defaultPath: "/Users/player/CyberCTF" } });
    expect(calls.find((c) => c.cmd === "shared_folder_set")?.args).toEqual({ enabled: true, path: "/Users/player/loot" });
    expect(text()).toContain("/Users/player/loot");
    picked = null;
    await user.click(button(/change/i));
    await flush();
    expect(calls.filter((c) => c.cmd === "shared_folder_set")).toHaveLength(1);
  });

  it("opens the folder, and shows why when it can't", async () => {
    let fail = false;
    const calls = installTauri({
      shared_folder_get: () => ({ enabled: true, path: "/p", mountPoint: "/shared" }),
      shared_folder_open: () => {
        if (fail) throw "folder is gone";
        return null;
      },
    });
    const user = userEvent.setup();
    render(<SharedFolderControl />);
    await flush();
    await user.click(screen.getByTitle(/open the folder/i));
    await flush();
    expect(calls.map((c) => c.cmd)).toContain("shared_folder_open");
    expect(text()).not.toContain("folder is gone");
    fail = true;
    await user.click(screen.getByTitle(/open the folder/i));
    await flush();
    expect(text()).toContain("folder is gone");
  });

  it("shows a failed save and keeps the old state", async () => {
    installTauri({
      shared_folder_set: () => {
        throw "permission denied";
      },
    });
    const user = userEvent.setup();
    render(<SharedFolderControl />);
    await flush();
    await user.click(screen.getByRole("switch"));
    await flush();
    expect(text()).toContain("permission denied");
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("false");
  });
});

/* ------------------------------------------------------------------ installed tools */

describe("InstalledToolsPanel", () => {
  afterEach(() => vi.restoreAllMocks());

  it("is hidden when Cyber CTF installed nothing", async () => {
    installTauri({ installed_tools: () => [] });
    const { container } = render(<InstalledToolsPanel />);
    await flush();
    expect(container.textContent).toBe("");
  });

  it("is hidden when the list can't be read", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    installTauri({
      installed_tools: () => {
        throw "no state file";
      },
    });
    const { container } = render(<InstalledToolsPanel />);
    await flush();
    expect(container.textContent).toBe("");
  });

  it("removes a tool after confirming, then re-reads the list", async () => {
    let tools = [
      { dependency: "vagrant", at: Math.floor(Date.now() / 1000) - 3600 },
      { dependency: "docker", at: Math.floor(Date.now() / 1000) - 60 },
    ];
    const calls = installTauri({
      installed_tools: () => tools,
      uninstall_dependency: (a) => {
        stream(a, "Removed Vagrant");
        tools = tools.filter((t) => t.dependency !== a?.dependency);
        return null;
      },
    });
    const user = userEvent.setup();
    render(<InstalledToolsPanel />);
    await flush();
    expect(text()).toContain("2 installed");
    expect(text()).toContain("Docker Desktop");
    expect(text()).toContain("Vagrant");
    await user.click(screen.getAllByRole("button", { name: /remove/i })[0]);
    expect(screen.getByRole("alertdialog").textContent).toContain("Remove Vagrant?");
    // Cancel first: nothing is removed.
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(calls.map((c) => c.cmd)).not.toContain("uninstall_dependency");
    await user.click(screen.getAllByRole("button", { name: /remove/i })[0]);
    await user.click(screen.getAllByRole("button", { name: /^remove$/i }).at(-1)!);
    await flush();
    expect(calls.find((c) => c.cmd === "uninstall_dependency")?.args).toMatchObject({ dependency: "vagrant" });
    expect(text()).toContain("Removing Vagrant…");
    expect(text()).toContain("Removed Vagrant");
    expect(text()).toContain("1 installed");
    expect(calls.filter((c) => c.cmd === "installed_tools")).toHaveLength(2);
  });

  it("names Docker Engine on Linux and shows a failed removal", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64)");
    installTauri({
      installed_tools: () => [{ dependency: "docker", at: Math.floor(Date.now() / 1000) }],
      uninstall_dependency: () => {
        throw "apt is locked";
      },
    });
    const user = userEvent.setup();
    render(<InstalledToolsPanel />);
    await flush();
    expect(text()).toContain("Docker Engine");
    await user.click(screen.getByRole("button", { name: /remove/i }));
    await user.click(screen.getAllByRole("button", { name: /^remove$/i }).at(-1)!);
    await flush();
    expect(text()).toContain("✗ apt is locked");
    expect(text()).toContain("Docker Engine");
  });
});

/* ------------------------------------------------------------------ account */

describe("AccountSection", () => {
  it("says it is checking while auth is unknown", () => {
    installTauri();
    render(<AccountSection auth={null} agent={null} onAuthChange={vi.fn()} />);
    expect(text()).toContain("Checking…");
  });

  it("shows the signed-in player and this machine's agent", () => {
    installTauri();
    render(
      <AccountSection
        auth={{ loggedIn: true, name: "Ada Lovelace", email: "ada@example.com" }}
        agent={{ installId: "x", name: "ada-pc", arch: "x86_64", capabilities: ["docker", "vagrant"] }}
        onAuthChange={vi.fn()}
      />,
    );
    expect(text()).toContain("Ada Lovelace");
    expect(text()).toContain("ada@example.com");
    expect(text()).toContain("AL");
    expect(text()).toContain("ada-pc");
    expect(text()).toContain("runs docker, vagrant");
  });

  it("says the machine is registering, and names a player without a name", () => {
    installTauri();
    render(
      <AccountSection
        auth={{ loggedIn: true, name: null, email: null }}
        agent={{ installId: "x", name: "pc", arch: "arm64", capabilities: [] }}
        onAuthChange={vi.fn()}
      />,
    );
    expect(text()).toContain("Signed in");
    expect(text()).not.toContain("runs");
    render(<AccountSection auth={{ loggedIn: true, name: "A", email: null }} agent={null} onAuthChange={vi.fn()} />);
    expect(text()).toContain("Registering…");
  });

  it("signs out", async () => {
    const calls = installTauri();
    const onAuthChange = vi.fn();
    const user = userEvent.setup();
    render(<AccountSection auth={{ loggedIn: true, name: "A", email: null }} agent={null} onAuthChange={onAuthChange} />);
    await user.click(button(/sign out/i));
    await flush();
    expect(calls.map((c) => c.cmd)).toContain("auth_logout");
    expect(onAuthChange).toHaveBeenCalledWith(expect.objectContaining({ loggedIn: false }));
  });

  it("shows a keychain error when signed out", () => {
    installTauri();
    render(<AccountSection auth={{ loggedIn: false, name: null, email: null, keychainError: "Keychain locked" }} agent={null} onAuthChange={vi.fn()} />);
    expect(text()).toContain("Keychain locked");
    expect(text()).not.toContain("Sign in with your Cyber account");
  });

  it("signs in, and shows why a sign-in failed", async () => {
    let fail = true;
    installTauri({
      auth_login: () => {
        if (fail) throw "browser closed";
        return { loggedIn: true, name: "Ada", email: null };
      },
    });
    const onAuthChange = vi.fn();
    const user = userEvent.setup();
    render(<AccountSection auth={{ loggedIn: false, name: null, email: null }} agent={null} onAuthChange={onAuthChange} />);
    expect(text()).toContain("Sign in with your Cyber account");
    await user.click(button(/^sign in$/i));
    await flush();
    expect(text()).toContain("browser closed");
    fail = false;
    await user.click(button(/^sign in$/i));
    await flush();
    expect(onAuthChange).toHaveBeenCalledWith(expect.objectContaining({ loggedIn: true, name: "Ada" }));
  });
});

/* ------------------------------------------------------------------ appearance and ports */

describe("AppearanceSection", () => {
  afterEach(() => {
    act(() => setLocalePreference("system"));
  });

  it("switches the theme and says it saved", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<AppearanceSection />);
    const light = screen.getByRole("radio", { name: "Light" });
    await user.click(light);
    expect(light.getAttribute("aria-checked")).toBe("true");
    expect(text()).toContain("Light uses warm paper tones");
    // Picking the current theme again changes nothing.
    await user.click(light);
    expect(light.getAttribute("aria-checked")).toBe("true");
  });

  it("switches the language, translating the section at once", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<AppearanceSection />);
    expect(getLocalePreference()).toBe("system");
    const select = screen.getByRole("combobox");
    expect(select.querySelectorAll("option")).toHaveLength(7);
    await user.selectOptions(select, "fr");
    expect(getLocalePreference()).toBe("fr");
    expect(text()).not.toContain("Theme");
    await user.selectOptions(screen.getByRole("combobox"), "system");
    expect(getLocalePreference()).toBe("system");
    expect(text()).toContain("Theme");
  });
});

describe("LabPortsRow", () => {
  it("describes and saves each port mode", async () => {
    installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<LabPortsRow onSaved={onSaved} />);
    expect(text()).toContain("Asks before each container lab starts");
    await user.click(screen.getByRole("radio", { name: "Random" }));
    expect(getPortMode()).toBe("random");
    expect(text()).toContain("free random port");
    await user.click(screen.getByRole("radio", { name: "Default" }));
    expect(getPortMode()).toBe("default");
    expect(text()).toContain("like CTFd on 8000");
    await user.click(screen.getByRole("radio", { name: "Ask" }));
    expect(getPortMode()).toBeNull();
    expect(onSaved).toHaveBeenCalledTimes(3);
  });
});
