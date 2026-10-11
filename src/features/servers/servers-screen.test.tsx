// @vitest-environment jsdom
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ServerScreen } from "@/features/servers/servers-screen";
import { HostRow } from "@/features/servers/host-row";
import { ServerSelfTest } from "@/features/servers/server-self-test";
import { answer } from "@/lib/dev-mock";
import type { ServerList, ServerTest } from "@/lib/tauri";
import { freshReport } from "@/test/fixtures";
import { commands, installTauri, stream, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const sample = () => structuredClone(answer("server_list", undefined) as ServerList);
const [pve, esxi] = sample().hosts;
const ok: ServerTest = { ok: true, reachable: true, authenticated: true, latencyMs: 12, message: "Connected", checks: [] };
const fail = (message: string): ServerTest => ({ ok: false, reachable: false, authenticated: null, latencyMs: null, message, checks: [] });
const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);

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
    await act(async () => resolve(ok));
    await flush();
    expect(document.body.textContent).toContain("1 of 2 online");
    expect(screen.getByText("1 lab running")).toBeTruthy();
  });

  it("asks for a password instead of calling a host without one down", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    installTauri({ server_test: () => fail("No password stored for this host") });
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
    expect(argsOf(calls, "plugin:opener|open_url")[0]).toMatchObject({ url: expect.stringContaining("Privacy_LocalNetwork") });
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
      server_set_default: () => {
        throw "servers.json is read-only";
      },
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
      server_remove: () => {
        throw "keychain locked";
      },
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
      server_open_setup: () => {
        if (failOpen) throw "window already open";
        return null;
      },
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
    fireEvent.click(document.querySelector("button[aria-hidden]")!);
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
    expect(JSON.parse(localStorage.getItem("cyberctf.server.vmtest")!).pve.result).toBe("ok");
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
      server_list: () => {
        throw "servers.json is corrupt";
      },
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
      server_capacity: () => {
        throw "pveproxy down";
      },
      server_running_labs: () => {
        throw "no state dir";
      },
    });
    await renderScreen();
    expect(screen.getByText("homelab-pve")).toBeTruthy();
    expect(screen.queryByText(/labs? running/)).toBeNull();
  });
});

describe("HostRow", () => {
  const noop = () => {};
  const props = {
    host: pve,
    isDefault: false,
    running: 0,
    capacity: undefined,
    test: undefined,
    lastVm: undefined,
    now: Date.now(),
    vmTesting: false,
    onTest: noop,
    onVmTest: noop,
    onVmTestDone: noop,
    onEdit: noop,
    onRemove: noop,
    onDefault: noop,
  };

  it("reads Not tested before the first test, with the host's facts", () => {
    render(<HostRow {...props} host={{ ...esxi, node: null }} capacity={{ cores: 8, memTotal: 16 * 2 ** 30, memFree: 4 * 2 ** 30 }} />);
    expect(screen.getByText("Not tested")).toBeTruthy();
    const facts = document.body.textContent ?? "";
    expect(facts).toContain("VMware ESXi");
    expect(facts).toContain("root@10.0.4.12:443");
    expect(facts).toContain("8 vCPU");
    expect(facts).not.toContain("node");
  });

  it("disables Test while a test runs", () => {
    render(<HostRow {...props} test="testing" />);
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
      server_selftest: () => {
        throw "terraform not found";
      },
    });
    const onDone = vi.fn();
    render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush(30);
    expect(screen.getByText("terraform not found")).toBeTruthy();
    expect(screen.getByText("VM test failed")).toBeTruthy();
    expect(onDone).toHaveBeenCalledWith("fail");
  });

  it("is running until the host answers, and doesn't report after unmounting", async () => {
    let finish: () => void = () => {};
    installTauri({ server_selftest: () => new Promise<null>((r) => (finish = () => r(null))) });
    const onDone = vi.fn();
    const { unmount } = render(<ServerSelfTest id="pve" provider="proxmox" onDone={onDone} />);
    await flush();
    expect(screen.getByText("Running a real VM on the host…")).toBeTruthy();
    unmount();
    await act(async () => finish());
    await flush();
    expect(onDone).not.toHaveBeenCalled();
  });
});
