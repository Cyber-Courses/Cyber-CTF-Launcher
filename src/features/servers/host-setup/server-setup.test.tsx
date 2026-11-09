// @vitest-environment jsdom
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EMPTY_HOST } from "@/features/servers/host-setup";
import { argsOf, failsWith, field, go, heading, input, renderSetup, selects } from "@/features/servers/host-setup/setup-test-kit";
import type { ServerHostInput } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { installTauri, stream } from "@/test/tauri";
import { button, flush } from "@/test/ui";

/** From a new Proxmox setup's first step (its address filled) to its sign-in step. */
async function toSignIn(user: ReturnType<typeof userEvent.setup>) {
  for (let n = 0; n < 3; n++) await go(user, /^continue$/i);
  expect(heading()).toBe("Connect to Proxmox");
}

/** The sign-in method choice named `name`. */
const radio = (name: string) => screen.getByRole("radio", { name });

/** An existing host being edited: server, connection, placement, test. */
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
    fireEvent.change(input("input[type=password]"), { target: { value: "hunter2" } });
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
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { port: null } });
  });

  it("shows the launcher's SSH key for a Proxmox API token, and copies it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    expect(radio("Password").getAttribute("aria-checked")).toBe("true");
    expect(argsOf(calls, "server_public_key")).toEqual([]);
    await user.click(radio("API token"));
    await flush();
    expect(screen.getByText("Token id")).toBeTruthy();
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
        writeText: () => Promise.reject(new Error("denied")),
      },
      configurable: true,
    });
    installTauri();
    await renderSetup(editing({ username: "root@pam!ci" }));
    fireEvent.click(button(/^continue$/i));
    await flush(30);
    fireEvent.click(button(/^\s*copy$/i));
    await flush();
    expect(button(/^\s*copy$/i)).toBeTruthy();
  });

  it("waits for the key, with Copy disabled", async () => {
    installTauri({ server_public_key: () => new Promise(() => {}) });
    await renderSetup(editing({ username: "root@pam!ci" }));
    fireEvent.click(button(/^continue$/i));
    await flush(30);
    expect(screen.getByText("Loading the launcher's public key…")).toBeTruthy();
    expect(button(/^\s*copy$/i).hasAttribute("disabled")).toBe(true);
  });

  it("turns off the self-signed certificate on Proxmox and shows a failed save", async () => {
    const calls = installTauri({
      server_save: failsWith("duplicate name"),
    });
    const user = userEvent.setup();
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(screen.getByText("Bridge")).toBeTruthy();
    await user.click(screen.getByRole("switch"));
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { insecureTls: false } });
    expect(screen.getByText("duplicate name")).toBeTruthy();
    await go(user, /^\s*back$/i);
    expect(heading()).toBe("Connect to Proxmox");
  });
});

describe("Proxmox server and sign-in steps", () => {
  it("asks for the server first, then signs in with the first usual SSH key, makes the token, and places labs on the server's own choices", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_HOST });
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(heading()).toBe("Your Proxmox server");
    expect(screen.getByText("API port")).toBeTruthy();
    expect(screen.queryByRole("radio")).toBeNull();
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    fireEvent.change(field("192.168.1.20 or pve.lan"), { target: { value: "10.0.0.50" } });
    await go(user, /^continue$/i);
    expect(heading()).toBe("Connect to Proxmox");
    expect(radio("SSH key").getAttribute("aria-checked")).toBe("true");
    expect(field("~/.ssh/id_ed25519").value).toBe("/Users/me/.ssh/id_ed25519");
    expect(field("root").value).toBe("root");
    expect(field("22").value).toBe("22");
    expect(screen.queryByText("Token secret")).toBeNull();
    expect(screen.getByText(/pveum user token add/)).toBeTruthy();
    // Leaving the step makes the token on the node, so Placement can read the server with it.
    await go(user, /^continue$/i);
    expect(argsOf(calls, "server_save_ssh_key")[0]).toMatchObject({
      input: { provider: "proxmox", host: "10.0.0.50", username: "", password: null, sshKey: { path: "/Users/me/.ssh/id_ed25519", user: "root", port: 22 } },
    });
    expect(heading()).toBe("Placement");
    expect(argsOf(calls, "server_proxmox_placement")[0]).toMatchObject({ input: { id: "new-host", username: "root@pam!cyberctf-0a1b2c3d" } });
    // Each choice is the server's own, preselected: the only node, local-lvm, local, and the
    // bridge on the server's own network.
    expect(selects().map((s) => s.value)).toEqual(["pve", "vmbr1", "local-lvm", "local", "local"]);
    expect(screen.getByRole("option", { name: "vmbr1 · 10.0.0.50/24" })).toBeTruthy();
    fireEvent.change(selects()[2], { target: { value: "local" } });
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save_ssh_key")).toHaveLength(1);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({
      input: {
        id: "new-host",
        username: "root@pam!cyberctf-0a1b2c3d",
        password: null,
        sshKey: { path: "/Users/me/.ssh/id_ed25519" },
        node: "pve",
        network: "vmbr1",
        datastore: "local",
        imageStorage: "local",
        snippetStorage: "local",
      },
    });
    expect(argsOf(calls, "server_test")[0]).toEqual({ id: "new-host" });
  });

  it("says why the key sign-in failed and stays on the step", async () => {
    installTauri({ server_save_ssh_key: failsWith("Couldn't sign in to the node over SSH with your key: Permission denied") });
    const user = userEvent.setup();
    await renderSetup(editing({ id: null }));
    await toSignIn(user);
    await go(user, /^continue$/i);
    expect(heading()).toBe("Connect to Proxmox");
    expect(screen.getByText(/Permission denied/)).toBeTruthy();
  });

  it("falls back to typing the placement when the server can't be read", async () => {
    const calls = installTauri({ server_proxmox_placement: failsWith("401 authentication failure") });
    const user = userEvent.setup();
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(screen.getByText(/Couldn't read the server's options \(401 authentication failure\)/)).toBeTruthy();
    expect(selects()).toEqual([]);
    fireEvent.change(field("vmbr0"), { target: { value: "vmbr7" } });
    fireEvent.change(field("pve"), { target: { value: "node2" } });
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { network: "vmbr7", node: "node2" } });
  });

  it("switches node: its own storages and bridge are picked", async () => {
    const node = (name: string, bridge: string) => ({
      name,
      diskStorages: [`${name}-ssd`],
      imageStorages: ["local"],
      snippetStorages: ["local"],
      bridges: [{ name: bridge, cidr: null }],
      defaultDisk: `${name}-ssd`,
      defaultImage: "local",
      defaultSnippet: "local",
      defaultBridge: bridge,
    });
    installTauri({ server_proxmox_placement: () => ({ nodes: [node("pve1", "vmbr0"), node("pve2", "vmbr5")] }) });
    const user = userEvent.setup();
    await renderSetup(editing({ node: "pve2" }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(selects().map((s) => s.value)).toEqual(["pve2", "vmbr5", "pve2-ssd", "local", "local"]);
    expect(screen.getByRole("option", { name: "vmbr5 · no address" })).toBeTruthy();
    fireEvent.change(selects()[0], { target: { value: "pve1" } });
    await flush();
    expect(selects().map((s) => s.value)).toEqual(["pve1", "vmbr0", "pve1-ssd", "local", "local"]);
  });

  it("needs a key, a user and a port, and picks the key with the file dialog", async () => {
    const calls = installTauri({ server_default_ssh_key: () => null, "plugin:dialog|open": () => "/Users/me/keys/pve" });
    const user = userEvent.setup();
    await renderSetup(editing({ id: null }));
    await toSignIn(user);
    expect(field("~/.ssh/id_ed25519").value).toBe("");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    await go(user, /choose/i);
    expect(field("~/.ssh/id_ed25519").value).toBe("/Users/me/keys/pve");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
    fireEvent.change(field("root"), { target: { value: " " } });
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    fireEvent.change(field("root"), { target: { value: "admin" } });
    fireEvent.change(field("22"), { target: { value: "" } });
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    fireEvent.change(field("22"), { target: { value: "2222" } });
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save_ssh_key")[0]).toMatchObject({ input: { sshKey: { path: "/Users/me/keys/pve", user: "admin", port: 2222 } } });
  });

  it("shows each method's own fields, and a password or token save sends no key", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup(editing({ id: null, username: "" }));
    await toSignIn(user);
    await user.click(radio("Password"));
    expect(screen.getByText("Username")).toBeTruthy();
    expect(input("input[type=password]")).toBeTruthy();
    expect(screen.queryByText("Private key")).toBeNull();
    expect(screen.queryByText("Loading the launcher's public key…")).toBeNull();
    await user.click(radio("API token"));
    expect(screen.getByText("Token id")).toBeTruthy();
    fireEvent.change(field("root!cyberctf"), { target: { value: "root@pam!ci" } });
    fireEvent.change(input("input[type=password]"), { target: { value: "s3cret" } });
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save_ssh_key")).toEqual([]);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { username: "root@pam!ci", password: "s3cret", sshKey: null } });
  });

  it("reopens a setup made with an SSH key on that key, and lists each checked part", async () => {
    const parts = [
      { name: "SSH with your key", state: "ok", detail: "Signed in to root@10.0.0.50:22." },
      { name: "API token", state: "ok", detail: "Made the API token root@pam!cyberctf-0a1b2c3d." },
      { name: "Launcher key", state: "ok", detail: "The launcher's key is in root's authorized_keys." },
      { name: "Proxmox API", state: "fail", detail: "Bridge vmbr9 doesn't exist." },
    ];
    installTauri({
      server_test: () => ({ ok: false, reachable: true, authenticated: true, latencyMs: 3, message: "Bridge vmbr9 doesn't exist.", checks: parts }),
    });
    const user = userEvent.setup();
    await renderSetup(editing({ username: "root@pam!cyberctf-0a1b2c3d", sshKey: { path: "/Users/me/keys/pve", user: "root", port: 22 } }));
    await go(user, /^continue$/i);
    expect(radio("SSH key").getAttribute("aria-checked")).toBe("true");
    expect(field("~/.ssh/id_ed25519").value).toBe("/Users/me/keys/pve");
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    const list = screen.getByRole("list", { name: "What was checked" });
    expect(list.textContent).toContain("SSH with your key");
    expect(list.textContent).toContain("Launcher key");
    expect(list.querySelectorAll("li")).toHaveLength(4);
  });
});

describe("Proxmox VM test after the connection test", () => {
  it("runs a real test lab once the connection test passes, step by step, and again with Test again", async () => {
    const calls = installTauri({
      server_selftest: (a) => {
        stream(
          a,
          { step: "connect", label: "Reach the host", state: "ok", detail: "homelab" },
          { step: "image", label: "Download", state: "ok", detail: null },
          {
            step: "uplink",
            label: "Uplink",
            state: "fail",
            detail:
              "The test VM got no address on bridge vmbr1: the bridge has no DHCP server reachable from the VMs (or no internet). Use an uplink bridge with DHCP and internet.",
          },
          { step: "cleanup", label: "Cleanup", state: "ok", detail: "Nothing of the test is left on the server." },
        );
        return null;
      },
    });
    const user = userEvent.setup();
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    await flush(30);
    expect(argsOf(calls, "server_selftest")[0]).toMatchObject({ id: "pve" });
    for (const label of ["Download the Debian cloud image", "Create the test's lab network", "Start the test VMs", "Remove everything the test created"]) {
      expect(screen.getByText(label)).toBeTruthy();
    }
    expect(screen.getByText(/got no address on bridge vmbr1/)).toBeTruthy();
    expect(screen.getByText("VM test failed")).toBeTruthy();
    await go(user, /test again/i);
    await flush(30);
    expect(argsOf(calls, "server_selftest")).toHaveLength(2);
  });

  it("runs no VM test while the connection test fails", async () => {
    const calls = installTauri({ server_test: () => ({ ok: false, reachable: true, authenticated: false, latencyMs: 4, message: "rejected", checks: [] }) });
    const user = userEvent.setup();
    await renderSetup(editing({}));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_selftest")).toEqual([]);
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
    expect(argsOf(calls, "plugin:opener|open_url")[0]?.url).toMatch(/broadcom\.com/);
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
