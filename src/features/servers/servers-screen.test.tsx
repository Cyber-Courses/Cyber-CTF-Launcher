// @vitest-environment jsdom
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ServerScreen } from "@/features/servers/servers-screen";
import { HostRow, type HostView } from "@/features/servers/host-row";
import { ServerSelfTest } from "@/features/servers/server-self-test";
import { loadVmTests } from "@/features/servers/vm-tests";
import { answer } from "@/lib/dev-mock";
import type { ServerHost, ServerList, ServerTest } from "@/lib/tauri";
import { freshReport } from "@/test/fixtures";
import { commands, installTauri, rejects, stream, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const sample = () => structuredClone(answer("server_list", undefined) as ServerList);
const [pve, esxi] = sample().hosts;
const ok: ServerTest = { ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] };
const fail = (message: string): ServerTest => ({ ok: false, reachable: false, authenticated: null, latencyMs: null, message, checks: [] });
const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);
/** A command that fails with `message`, as the Rust core's `other` error. */
const failsWith = (message: string) => rejects({ code: "other", message });

async function renderScreen(onNavigate = vi.fn()) {
  render(<ServerScreen onNavigate={onNavigate} />);
  await flush(50);
  return onNavigate;
}

/** Opens a host row's overflow menu and presses `item`. */
async function menu(user: ReturnType<typeof userEvent.setup>, rowName: string, item: RegExp) {
  const row = screen.getByText(rowName).closest(".border-t") as HTMLElement;
  await user.click(within(row).getByRole("button", { name: /more actions/i }));
  await user.click(button(item));
  await flush();
}

/** The open menu's backdrop: hidden from assistive tech, a click on it closes the menu. */
function menuBackdrop(): Element {
  const el = document.querySelector("button[aria-hidden]");
  if (!el) throw new Error("the menu is closed");
  return el;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("ServerScreen", () => {
  it("lists only the hypervisors, tests and probes each once, and summarises them", async () => {
    const list = sample();
    // Token and key clouds are cloud accounts too: they belong on the Cloud page.
    list.hosts.push(
      { ...esxi, id: "do", name: "DO droplets", provider: "digitalocean", host: "fra1" },
      { ...esxi, id: "oci", name: "Oracle tenancy", provider: "oci", host: "eu-frankfurt-1" },
    );
    const calls = installTauri({ server_list: () => list, server_running_labs: () => ({ pve: 2 }) });
    await renderScreen();
    expect(screen.getByText("homelab-pve")).toBeTruthy();
    expect(screen.getByText("office-esxi")).toBeTruthy();
    for (const cloud of ["AWS sandbox", "DO droplets", "Oracle tenancy"]) expect(screen.queryByText(cloud)).toBeNull();
    expect(argsOf(calls, "server_test").map((a) => a?.id)).toEqual(["pve", "esxi"]);
    expect(argsOf(calls, "server_capacity").map((a) => a?.id)).toEqual(["pve", "esxi"]);
    expect(document.body.textContent).toContain("2 of 2 online · 2 hosts");
    expect(screen.getAllByText("Online")).toHaveLength(2);
    expect(screen.getByText("2 labs running")).toBeTruthy();
    expect(screen.getByText("Capacity on homelab-pve")).toBeTruthy();
    expect(document.body.textContent).toContain("12 ms");
    expect(document.body.textContent).toContain("node pve");
  });

  it("tests every host again with Test all, and one with its own Test", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderScreen();
    calls.length = 0;
    await user.click(button(/^\s*test all$/i));
    await flush();
    expect(argsOf(calls, "server_test").map((a) => a?.id)).toEqual(["pve", "esxi"]);
    calls.length = 0;
    const row = screen.getByText("office-esxi").closest(".border-t") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: /^\s*test$/i }));
    await flush();
    expect(argsOf(calls, "server_test")).toEqual([{ id: "esxi" }]);
  });

  it("shows a host that is testing, then its failure, without a phantom running count", async () => {
    let resolve: (t: ServerTest) => void = () => {};
    installTauri({
      server_test: (a) => (a?.id === "pve" ? new Promise<ServerTest>((r) => (resolve = r)) : fail("Connection refused (os error 61)")),
      server_running_labs: () => ({ pve: 1, esxi: 3 }),
    });
    await renderScreen();
    expect(screen.getByText("Testing…")).toBeTruthy();
    expect(document.body.textContent).toContain("Checking… · 2 hosts");
    expect(screen.getByText("Unreachable")).toBeTruthy();
    expect(screen.getByText("Connection refused (os error 61)")).toBeTruthy();
    // The unreachable ESXi host doesn't claim its 3 stale running labs.
    expect(screen.queryByText("3 labs running")).toBeNull();
    resolve(ok);
    await flush();
    expect(document.body.textContent).toContain("1 of 2 online");
    expect(screen.getByText("1 lab running")).toBeTruthy();
  });

  it("asks for a password instead of calling a host without one down", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    installTauri({ server_test: rejects({ code: "noPasswordStored", message: "no password stored for this host, edit it and enter one" }) });
    await renderScreen();
    expect(screen.getAllByText("Needs password")).toHaveLength(2);
    // A missing password isn't a network problem: no Local Network hint.
    expect(screen.queryByText(/Open Privacy & Security/)).toBeNull();
    expect(document.body.textContent).toContain("0 of 2 online");
  });

  it("points macOS players at the Local Network permission for a LAN host that fails", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    const calls = installTauri({ server_test: () => fail("timed out") });
    const user = userEvent.setup();
    await renderScreen();
    const links = screen.getAllByRole("button", { name: /Open Privacy & Security/ });
    expect(links).toHaveLength(2);
    await user.click(links[0]);
    await flush();
    expect(argsOf(calls, "plugin:opener|open_url")[0]?.url).toMatch(/Privacy_LocalNetwork/);
  });

  it("does not show the Local Network hint off macOS or for a public host", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    installTauri({ server_test: () => fail("timed out") });
    await renderScreen();
    expect(screen.queryByText(/Open Privacy & Security/)).toBeNull();
  });

  it("toggles the default host: unsets the current default, sets another", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderScreen();
    const defaults = screen.getAllByRole("button", { name: /default/ });
    expect(defaults[0].getAttribute("title")).toMatch(/Click to unset/);
    expect(defaults[1].getAttribute("title")).toMatch(/Make this the default/);
    // The default host's chip is pressed and says Default; the other offers to become it.
    expect(defaults.map((d) => [d.textContent.trim(), d.getAttribute("aria-pressed")])).toEqual([
      ["default", "true"],
      ["Set as default", "false"],
    ]);
    const lists = commands(calls).filter((c) => c === "server_list").length;
    await user.click(defaults[0]);
    await flush();
    await user.click(defaults[1]);
    await flush();
    expect(argsOf(calls, "server_set_default")).toEqual([{ id: null }, { id: "esxi" }]);
    expect(commands(calls).filter((c) => c === "server_list").length).toBe(lists + 2);
  });

  it("shows why setting the default failed", async () => {
    installTauri({
      server_set_default: failsWith("servers.json is read-only"),
    });
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getAllByRole("button", { name: /default/ })[1]);
    await flush();
    expect(screen.getByText("servers.json is read-only")).toBeTruthy();
  });

  it("removes a host only once the player confirms", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderScreen();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    await menu(user, "office-esxi", /remove/i);
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("Remove office-esxi?"));
    expect(commands(calls)).not.toContain("server_remove");
    confirm.mockReturnValue(true);
    await menu(user, "office-esxi", /remove/i);
    expect(argsOf(calls, "server_remove")).toEqual([{ id: "esxi" }]);
  });

  it("shows why a removal failed", async () => {
    installTauri({
      server_remove: failsWith("keychain locked"),
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const user = userEvent.setup();
    await renderScreen();
    await menu(user, "homelab-pve", /remove/i);
    expect(screen.getByText("keychain locked")).toBeTruthy();
  });

  it("opens the setup window to edit a host or add one, and shows when it can't", async () => {
    let failOpen = false;
    const calls = installTauri({
      server_open_setup: (a) => (failOpen ? failsWith("window already open")(a) : null),
    });
    const user = userEvent.setup();
    await renderScreen();
    await menu(user, "homelab-pve", /edit/i);
    await user.click(button(/add server/i));
    await flush();
    expect(argsOf(calls, "server_open_setup")).toEqual([
      { id: "pve", kind: "server" },
      { id: null, kind: "server" },
    ]);
    failOpen = true;
    await user.click(button(/add server/i));
    await flush();
    expect(screen.getByText("window already open")).toBeTruthy();
  });

  it("closes the overflow menu on a second click, the backdrop, or a scroll", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderScreen();
    const row = screen.getByText("homelab-pve").closest(".border-t") as HTMLElement;
    const more = within(row).getByRole("button", { name: /more actions/i });
    await user.click(more);
    expect(screen.getByText("VM test")).toBeTruthy();
    await user.click(more);
    expect(screen.queryByText("VM test")).toBeNull();
    await user.click(more);
    fireEvent.scroll(window);
    expect(screen.queryByText("VM test")).toBeNull();
    await user.click(more);
    fireEvent.click(menuBackdrop());
    expect(screen.queryByText("VM test")).toBeNull();
  });

  it("runs a VM test from the menu, remembers its result and tests the host again", async () => {
    const calls = installTauri({
      server_selftest: (a) => {
        stream(a, { step: "connect", state: "ok" }, { step: "apply", state: "ok", detail: "vm 9001" }, { step: "cleanup", state: "ok" });
        return null;
      },
    });
    const user = userEvent.setup();
    await renderScreen();
    calls.length = 0;
    await menu(user, "homelab-pve", /^\s*vm test$/i);
    await flush(30);
    expect(argsOf(calls, "server_selftest")[0]).toMatchObject({ id: "pve" });
    expect(screen.getAllByText("VM test passed").length).toBeGreaterThan(0);
    expect(screen.getByText("vm 9001")).toBeTruthy();
    expect(loadVmTests().pve?.result).toBe("ok");
    expect(argsOf(calls, "server_test")).toEqual([{ id: "pve" }]);
    expect(commands(calls)).toContain("server_running_labs");
    // The menu now offers to hide it.
    await menu(user, "homelab-pve", /hide vm test/i);
    expect(screen.queryByText("Destroy the test VM")).toBeNull();
  });

  it("shows the remembered result of an earlier VM test", async () => {
    localStorage.setItem("cyberctf.server.vmtest", JSON.stringify({ esxi: { result: "fail", at: Date.now() - 120_000 } }));
    installTauri();
    await renderScreen();
    expect(screen.getByText("VM test failed")).toBeTruthy();
  });

  it("reloads and re-tests every host when the setup window saves one", async () => {
    const calls = installTauri();
    await renderScreen();
    calls.length = 0;
    await act(async () => {
      await emit("server:changed");
    });
    await flush(30);
    expect(commands(calls)).toContain("server_list");
    expect(commands(calls)).toContain("server_running_labs");
    expect(argsOf(calls, "server_test").map((a) => a?.id)).toEqual(["pve", "esxi"]);
  });

  it("shows the list error", async () => {
    installTauri({
      server_list: failsWith("servers.json is corrupt"),
    });
    await renderScreen();
    expect(screen.getByText("servers.json is corrupt")).toBeTruthy();
  });

  it("offers the first server, and a local hypervisor when this machine can't run VMs", async () => {
    const calls = installTauri({ server_list: () => ({ default: null, hosts: [] }), system_check: () => freshReport() });
    const user = userEvent.setup();
    const onNavigate = await renderScreen();
    expect(document.body.textContent).not.toContain("Test all");
    expect(screen.queryByText("Hosts")).toBeNull();
    await user.click(button(/set up this machine/i));
    expect(onNavigate).toHaveBeenCalledWith("setup");
    expect(commands(calls)).not.toContain("server_test");
  });

  it("hides the capacity panel when no host reports its capacity", async () => {
    installTauri({ server_capacity: () => null });
    await renderScreen();
    expect(screen.queryByText(/Capacity on/)).toBeNull();
    expect(document.body.textContent).not.toContain("vCPU");
  });

  it("keeps the page up when the capacity read or running count fails", async () => {
    installTauri({
      server_capacity: failsWith("pveproxy down"),
      server_running_labs: failsWith("no state dir"),
    });
    await renderScreen();
    expect(screen.getByText("homelab-pve")).toBeTruthy();
    expect(screen.queryByText(/labs? running/)).toBeNull();
  });
});

describe("HostRow", () => {
  const noop = () => {};
  const actions = { onTest: noop, onVmTest: noop, onVmTestDone: noop, onEdit: noop, onRemove: noop, onDefault: noop };
  const view: HostView = { isDefault: false, running: 0, capacity: undefined, test: undefined, lastVm: undefined, vmTesting: false };
  const row = (host: ServerHost, patch: Partial<HostView>) => render(<HostRow host={host} view={{ ...view, ...patch }} now={Date.now()} actions={actions} />);

  it("reads Not tested before the first test, with the host's facts", () => {
    row({ ...esxi, node: null }, { capacity: { cores: 8, memTotal: 16 * 2 ** 30, memFree: 4 * 2 ** 30 } });
    expect(screen.getByText("Not tested")).toBeTruthy();
    const facts = document.body.textContent;
    expect(facts).toContain("VMware ESXi");
    expect(facts).toContain("root@10.0.4.12:443");
    expect(facts).toContain("8 vCPU");
    expect(facts).not.toContain("node");
  });

  it("disables Test while a test runs", () => {
    row(pve, { test: "testing" });
    expect(button(/^\s*test$/i).hasAttribute("disabled")).toBe(true);
  });
});

describe("ServerSelfTest", () => {
  it("plans the ESXi steps and reports a failed step", async () => {
    installTauri({
      server_selftest: (a) => {
        stream(
          a,
          { step: "connect", state: "ok" },
          { step: "up", state: "running" },
          { step: "up", state: "fail", detail: "datastore full" },
          { step: "ssh", state: "skip" },
        );
        return null;
      },
    });
    const onDone = vi.fn();
    render(<ServerSelfTest id="esxi" provider="vmware_esxi" onDone={onDone} />);
    await flush(30);
    expect(screen.getByText("Run a command in the VM")).toBeTruthy();
    expect(screen.queryByText("VM reports a network address")).toBeNull();
    expect(screen.getByText("VM test failed")).toBeTruthy();
    expect(screen.getByText("datastore full")).toBeTruthy();
    expect(onDone).toHaveBeenCalledWith("fail");
  });

  it("shows the error when the test can't run", async () => {
    installTauri({
      server_selftest: failsWith("terraform not found"),
    });
    const onDone = vi.fn();
    render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush(30);
    expect(screen.getByText("terraform not found")).toBeTruthy();
    expect(screen.getByText("VM test failed")).toBeTruthy();
    expect(onDone).toHaveBeenCalledWith("fail");
  });

  it("a panel reopened while the test runs attaches to it instead of starting a second VM", async () => {
    let finish: () => void = () => {};
    let args: Record<string, unknown> | undefined;
    const selftest = vi.fn((a: Record<string, unknown> | undefined) => {
      args = a;
      return new Promise<null>(
        (r) =>
          (finish = () => {
            r(null);
          }),
      );
    });
    installTauri({ server_selftest: selftest });
    const onDone = vi.fn();
    const first = render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush();
    act(() => {
      stream(args, { step: "connect", label: "Connect", state: "ok", detail: null });
    });
    first.unmount();
    render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush();
    expect(selftest).toHaveBeenCalledTimes(1);
    // The step that passed before this view opened is shown again.
    expect(screen.getByText("Running a real VM on the host…")).toBeTruthy();
    finish();
    await flush();
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(onDone).toHaveBeenCalledWith("ok");
  });

  it("is running until the host answers, and doesn't report after unmounting", async () => {
    let finish: () => void = () => {};
    installTauri({
      server_selftest: () =>
        new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        ),
    });
    const onDone = vi.fn();
    const { unmount } = render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush();
    expect(screen.getByText("Running a real VM on the host…")).toBeTruthy();
    unmount();
    finish();
    await flush();
    expect(onDone).not.toHaveBeenCalled();
  });
});
