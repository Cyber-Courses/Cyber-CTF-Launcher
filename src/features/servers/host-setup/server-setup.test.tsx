// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EMPTY_HOST, HostSetupPage } from "@/features/servers/host-setup";
import type { ServerHostInput, SystemReport } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { installTauri, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);
const heading = () => screen.getByRole("heading", { level: 2 }).textContent;
const field = (placeholder: string) => screen.getByPlaceholderText(placeholder) as HTMLInputElement;

async function renderSetup(initial: ServerHostInput, report: SystemReport | null = readyReport()) {
  const handlers = { onRefresh: vi.fn(), onSaved: vi.fn(), onDone: vi.fn() };
  render(<HostSetupPage initial={initial} report={report} {...handlers} />);
  await flush(30);
  return handlers;
}
const go = async (user: ReturnType<typeof userEvent.setup>, name: RegExp) => {
  await user.click(button(name));
  await flush(30);
};
/** An existing host being edited: connection, placement, test. */
const editing = (patch: Partial<ServerHostInput>): ServerHostInput => ({
  ...EMPTY_HOST,
  id: "pve",
  name: "homelab",
  host: "192.168.1.20",
  username: "root@pam",
  ...patch,
});

describe("server connection step", () => {
  it("needs a host, a user and a password for a new server, then saves it with its placement", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const report = readyReport();
    report.ovftool = { installed: true, version: "4.6.3" };
    await renderSetup({ ...EMPTY_HOST, provider: "vmware_esxi" }, report);
    await go(user, /^continue$/i);
    expect(document.body.textContent).toContain("4.6.3");
    await go(user, /^continue$/i);
    expect(heading()).toBe("Connect to VMware ESXi");
    expect(screen.getByText("SSH port")).toBeTruthy();
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    fireEvent.change(field("192.168.1.20 or pve.lan"), { target: { value: "10.0.4.12" } });
    fireEvent.change(field("root"), { target: { value: "root" } });
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    fireEvent.change(document.querySelector("input[type=password]")!, { target: { value: "hunter2" } });
    fireEvent.change(field("22"), { target: { value: "2222" } });
    await go(user, /^continue$/i);
    expect(screen.getByText("Port group")).toBeTruthy();
    expect(screen.queryByRole("switch")).toBeNull();
    fireEvent.change(field("datastore1"), { target: { value: "ssd" } });
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({
      input: { provider: "vmware_esxi", name: "10.0.4.12", host: "10.0.4.12", port: 2222, password: "hunter2", datastore: "ssd" },
    });
  });

  it("clears the port back to the default", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup(editing({ port: 8007 }));
    fireEvent.change(field("8006"), { target: { value: "" } });
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { port: null } });
  });

  it("shows the launcher's SSH key for a Proxmox API token, and copies it", async () => {
    const calls = installTauri();
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    await renderSetup(editing({}));
    expect(screen.getByText("Password")).toBeTruthy();
    expect(argsOf(calls, "server_public_key")).toEqual([]);
    fireEvent.change(field("root@pam"), { target: { value: "root@pam!ci" } });
    await flush();
    expect(screen.getByText("Token secret")).toBeTruthy();
    expect(screen.getByText("ssh-ed25519 AAAAC3Nza cyberctf")).toBeTruthy();
    // A plain click: user-event would swap in its own clipboard.
    fireEvent.click(button(/^\s*copy$/i));
    await flush();
    expect(writeText).toHaveBeenCalledWith("ssh-ed25519 AAAAC3Nza cyberctf");
    expect(button(/copied/i)).toBeTruthy();
  });

  it("keeps the key on screen when the clipboard is blocked, and while it loads", async () => {
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: async () => {
          throw new Error("denied");
        },
      },
      configurable: true,
    });
    installTauri();
    await renderSetup(editing({ username: "root@pam!ci" }));
    fireEvent.click(button(/^\s*copy$/i));
    await flush();
    expect(button(/^\s*copy$/i)).toBeTruthy();
  });

  it("waits for the key, with Copy disabled", async () => {
    installTauri({ server_public_key: () => new Promise(() => {}) });
    await renderSetup(editing({ username: "root@pam!ci" }));
    expect(screen.getByText("Loading the launcher's public key…")).toBeTruthy();
    expect(button(/^\s*copy$/i).hasAttribute("disabled")).toBe(true);
  });

  it("turns off the self-signed certificate on Proxmox and shows a failed save", async () => {
    const calls = installTauri({
      server_save: () => {
        throw "duplicate name";
      },
    });
    const user = userEvent.setup();
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    expect(screen.getByText("Bridge")).toBeTruthy();
    await user.click(screen.getByRole("switch"));
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { insecureTls: false } });
    expect(screen.getByText("duplicate name")).toBeTruthy();
    await go(user, /^\s*back$/i);
    expect(heading()).toBe("Connect to Proxmox VE");
  });
});

describe("server tools step", () => {
  it("installs Terraform for Proxmox", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const h = await renderSetup({ ...EMPTY_HOST }, freshReport("linux"));
    await go(user, /^continue$/i);
    expect(heading()).toBe("Tools on this machine");
    expect(screen.getByText(/Drives the Proxmox API/)).toBeTruthy();
    await go(user, /install terraform/i);
    expect(argsOf(calls, "install_dependency")[0]).toMatchObject({ dependency: "terraform" });
    expect(h.onRefresh).toHaveBeenCalled();
  });

  it("needs Vagrant before the ESXi plugin, and links to the OVF Tool", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_HOST, provider: "vmware_esxi" }, freshReport());
    await go(user, /^continue$/i);
    expect(screen.getByText("vagrant-vmware-esxi, once Vagrant is installed.")).toBeTruthy();
    expect(button(/install plugin/i).hasAttribute("disabled")).toBe(true);
    await go(user, /install vagrant/i);
    expect(argsOf(calls, "install_dependency")[0]).toMatchObject({ dependency: "vagrant" });
    await go(user, /^\s*get$/i);
    expect(argsOf(calls, "plugin:opener|open_url")[0]).toMatchObject({ url: expect.stringContaining("broadcom.com") });
  });

  it("installs the ESXi plugin once Vagrant is there", async () => {
    const report = readyReport();
    report.vmProviders = report.vmProviders.map((p) => (p.provider === "vmware_esxi" ? { ...p, pluginInstalled: false } : p));
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_HOST, provider: "vmware_esxi" }, report);
    await go(user, /^continue$/i);
    expect(document.body.textContent).toContain("2.4.3");
    await go(user, /install plugin/i);
    expect(argsOf(calls, "install_vagrant_plugin")[0]).toMatchObject({ plugin: "vagrant-vmware-esxi" });
  });
});
