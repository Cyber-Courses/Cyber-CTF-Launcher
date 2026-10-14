// @vitest-environment jsdom
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CloudScreen } from "@/features/cloud/cloud-screen";
import { EMPTY_CLOUD, EMPTY_HOST, HostSetupPage } from "@/features/servers/host-setup";
import { ServerScreen } from "@/features/servers/servers-screen";
import { answer } from "@/lib/dev-mock";
import type { ServerHostInput, ServerList, SystemReport } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { installTauri, stream, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// The server / cloud setup wizard driven the way a player does: typing in the fields, pressing
// Continue, Back and Save. Complements server-setup.test.tsx and cloud-setup.test.tsx, which
// cover each step's content; these tests look at what moves between steps and what is saved.

type User = ReturnType<typeof userEvent.setup>;
const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);
const heading = () => screen.getByRole("heading", { level: 2 }).textContent;
const field = (placeholder: string) => screen.getByPlaceholderText(placeholder) as HTMLInputElement;
const password = () => document.querySelector<HTMLInputElement>("input[type=password]")!;
const disabled = (name: RegExp) => button(name).hasAttribute("disabled");

async function renderSetup(initial: ServerHostInput, report: SystemReport | null = readyReport()) {
  const handlers = { onRefresh: vi.fn(), onSaved: vi.fn(), onDone: vi.fn() };
  render(<HostSetupPage initial={initial} report={report} {...handlers} />);
  await flush(30);
  return handlers;
}
const go = async (user: User, name: RegExp) => {
  await user.click(button(name));
  await flush(30);
};

/** A saved Proxmox host opened for editing: connection, placement, test. */
const editingServer = (patch: Partial<ServerHostInput> = {}): ServerHostInput => ({
  ...EMPTY_HOST,
  id: "pve",
  name: "homelab",
  host: "192.168.1.20",
  username: "root@pam",
  ...patch,
});

/** A new Proxmox host, past the hypervisor and tools steps (Terraform is installed). */
async function newProxmoxAtConnection(user: User) {
  const report = readyReport();
  report.terraform = { installed: true, version: "1.9.0" };
  const handlers = await renderSetup({ ...EMPTY_HOST }, report);
  await go(user, /^continue$/i);
  await go(user, /^continue$/i);
  expect(heading()).toBe("Connect to Proxmox VE");
  return handlers;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("server setup: validation and saving", () => {
  it("keeps Continue off until the address, user and password are all filled, and a blank address does not count", async () => {
    installTauri();
    const user = userEvent.setup();
    await newProxmoxAtConnection(user);
    expect(disabled(/^continue$/i)).toBe(true);
    // Spaces only: the core would get an empty host.
    fireEvent.change(field("192.168.1.20 or pve.lan"), { target: { value: "   " } });
    fireEvent.change(field("root@pam"), { target: { value: "root@pam" } });
    fireEvent.change(password(), { target: { value: "s3cret" } });
    expect(disabled(/^continue$/i)).toBe(true);
    fireEvent.change(field("192.168.1.20 or pve.lan"), { target: { value: "pve.lan" } });
    expect(disabled(/^continue$/i)).toBe(false);
    // Clearing the password again turns Continue back off.
    fireEvent.change(password(), { target: { value: "" } });
    expect(disabled(/^continue$/i)).toBe(true);
  });

  it("goes back from the connection step to the tools and keeps what was typed", async () => {
    installTauri();
    const user = userEvent.setup();
    await newProxmoxAtConnection(user);
    fireEvent.change(field("192.168.1.20 or pve.lan"), { target: { value: "pve.lan" } });
    await go(user, /back/i);
    expect(heading()).not.toBe("Connect to Proxmox VE");
    await go(user, /^continue$/i);
    expect(field("192.168.1.20 or pve.lan").value).toBe("pve.lan");
  });

  it("saves an edited server without a password, so the core keeps the one in the keychain", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup(editingServer());
    expect(password().placeholder).toBe("Leave empty to keep");
    expect(disabled(/^continue$/i)).toBe(false);
    // Typed then erased: still "keep", never an empty string that would overwrite the secret.
    await user.type(password(), "x");
    await user.clear(password());
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { id: "pve", password: null } });
  });

  it("names a server after its address when the name is left blank", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup(editingServer({ name: "" }));
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { name: "192.168.1.20" } });
  });

  it("saves once when Save and test is pressed twice while the save is in flight", async () => {
    let finish: (v: unknown) => void = () => {};
    const calls = installTauri({ server_save: () => new Promise((r) => (finish = r)) });
    const user = userEvent.setup();
    await renderSetup(editingServer());
    await go(user, /^continue$/i);
    await user.click(button(/save and test/i));
    await user.click(button(/save and test/i));
    expect(argsOf(calls, "server_save")).toHaveLength(1);
    finish({ ...editingServer(), id: "pve" });
    await flush(30);
    expect(heading()).toBe("Connection test");
  });

  it("shows why a save failed, stays on the step, and clears the message when a retry works", async () => {
    let fail = true;
    installTauri({
      server_save: (a) => {
        if (fail) throw "enter the host's password";
        return { ...(a?.input as object), id: "pve" };
      },
    });
    const user = userEvent.setup();
    await renderSetup(editingServer());
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(screen.getByText("enter the host's password")).toBeTruthy();
    expect(heading()).not.toBe("Connection test");
    fail = false;
    await go(user, /save and test/i);
    expect(screen.queryByText("enter the host's password")).toBeNull();
    expect(heading()).toBe("Connection test");
  });

  it("tests the saved host again from the test step, with Test again off while it runs", async () => {
    let answer: (v: unknown) => void = () => {};
    let n = 0;
    const calls = installTauri({
      server_test: () => {
        n++;
        if (n === 1) return { ok: false, reachable: false, authenticated: null, latencyMs: null, message: "no route to host", checks: [] };
        return new Promise((r) => (answer = r));
      },
    });
    const user = userEvent.setup();
    await renderSetup(editingServer());
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(screen.getByText("no route to host")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toMatch(/not connected/i);
    await user.click(button(/test again/i));
    expect(disabled(/test again/i)).toBe(true);
    answer({ ok: true, reachable: true, authenticated: true, latencyMs: 9, message: "Connected", checks: [] });
    await flush(30);
    expect(argsOf(calls, "server_test")).toEqual([{ id: "pve" }, { id: "pve" }]);
    expect(screen.getByText("Connected")).toBeTruthy();
  });
});

describe("cloud setup: validation per provider", () => {
  it("needs a DigitalOcean token before Continue, and a provider switch drops the token typed for another cloud", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD });
    await user.click(screen.getByRole("radio", { name: /digitalocean/i }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(disabled(/^continue$/i)).toBe(true);
    await user.type(password(), "dop_v1_abc");
    expect(disabled(/^continue$/i)).toBe(false);
    // Back to the provider: Linode must not inherit DigitalOcean's token.
    await go(user, /back/i);
    await go(user, /back/i);
    await user.click(screen.getByRole("radio", { name: /linode/i }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(password().value).toBe("");
    expect(disabled(/^continue$/i)).toBe(true);
    expect(argsOf(calls, "server_save")).toEqual([]);
  });

  it("lets an edited token account keep its token, and saves no new one", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD, id: "do1", name: "DO", provider: "digitalocean", host: "fra1", useCliCreds: false });
    expect(heading()).toMatch(/digitalocean/i);
    expect(password().placeholder).toBe("Leave empty to keep");
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { id: "do1", provider: "digitalocean", password: null } });
  });

  it("selects a cloud from the keyboard in the provider list", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD });
    const linode = screen.getByRole("radio", { name: /linode/i });
    linode.focus();
    await user.keyboard(" ");
    expect(linode.getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("radio", { name: /amazon/i }).getAttribute("aria-checked")).toBe("false");
  });

  // Bug: connectionReady() (setup-model.ts:94) treats any edited account as having a stored
  // secret. An AWS account saved with the CLI never had a secret key, yet after switching it to
  // access keys the secret field says "Leave empty to keep" and Continue is on with no secret;
  // the save then fails in the core with "enter the host's password" on the last step.
  // Suggested fix: only count the secret as kept when the saved account already used the same
  // way in (pass `initial.useCliCreds` to connectionReady, and require a secret when it was
  // true and v.useCliCreds is now false), and show no "keep" placeholder in that case.
  it("asks for a secret key when an account that signed in with the CLI switches to access keys", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD, id: "acct", name: "Lab account", useCliCreds: true });
    await user.click(screen.getByText("Access keys"));
    await go(user, /^continue$/i);
    await user.type(field("AKIA…"), "AKIA1");
    expect(password().placeholder).not.toBe("Leave empty to keep");
    expect(disabled(/^continue$/i)).toBe(true);
  });

  // Bug: options-step.tsx:28 lists 1, 2, 4, 8, 12, 24 hours and Never, but the core accepts any
  // auto-stop (input.rs keeps 72). Editing such an account shows the first option ("1 hour")
  // while the form still holds 72, so the screen says one thing and Save writes another.
  // Suggested fix: as the region select does, add an option for a saved value the list lacks.
  it("shows an edited account's own auto-stop when it is not one of the listed choices", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD, id: "do1", name: "DO", provider: "digitalocean", host: "fra1", autoStopHours: 72 });
    await go(user, /^continue$/i);
    const autoStop = screen.getAllByRole("combobox").at(-1) as HTMLSelectElement;
    expect(autoStop.selectedOptions[0]?.textContent).toMatch(/72/);
  });

  // Bug: use-host-setup.ts:73 derives "an install is running" from the log's last line not
  // starting with ✓ or ✗. An installer that prints its own "✓ …" progress line mid-run makes the
  // step think the install ended: the Install buttons come back on and a second install of the
  // same tool can start beside the first. Suggested fix: keep an explicit `installing` flag set
  // before the task and cleared in a finally, as useMachineSetup does.
  it("keeps the Install buttons off while an install runs, whatever its log lines say", async () => {
    let finish: () => void = () => {};
    const calls = installTauri({
      install_dependency: (a) =>
        new Promise<null>((r) => {
          stream(a, "✓ Downloaded terraform_1.9.0.zip");
          finish = () => r(null);
        }),
    });
    const user = userEvent.setup();
    const report = readyReport();
    report.terraform = { installed: false, version: null };
    await renderSetup({ ...EMPTY_HOST }, report);
    await go(user, /^continue$/i);
    await user.click(button(/install terraform/i));
    await flush();
    expect(disabled(/install terraform/i)).toBe(true);
    await user.click(button(/install terraform/i));
    expect(argsOf(calls, "install_dependency")).toHaveLength(1);
    finish();
  });
});

describe("removing a host or account that still runs labs", () => {
  const refused = "1 lab still running on this host. Stop it before removing the host.";

  it("leaves a server with a running lab listed and says why the core refused", async () => {
    installTauri({
      server_running_labs: () => ({ pve: 1 }),
      server_remove: () => {
        throw refused;
      },
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const user = userEvent.setup();
    render(<ServerScreen onNavigate={vi.fn()} />);
    await flush(50);
    const row = screen.getByText("homelab-pve").closest(".border-t") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: /more actions/i }));
    await user.click(button(/remove/i));
    await flush();
    expect(screen.getByText(refused)).toBeTruthy();
    expect(screen.getByText("homelab-pve")).toBeTruthy();
  });

  it("leaves a cloud account with a running lab listed and says why the core refused", async () => {
    installTauri({
      server_remove: () => {
        throw refused;
      },
    });
    const user = userEvent.setup();
    render(<CloudScreen />);
    await flush(50);
    const accountName = (answer("server_list", undefined) as ServerList).hosts.find((h) => h.provider === "aws")!.name;
    expect(screen.getByText(accountName)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(button(/^remove$/i));
    await flush();
    expect(screen.getByText(refused)).toBeTruthy();
    expect(screen.getByText(accountName)).toBeTruthy();
  });
});

describe("server row menu: keyboard", () => {
  async function openMenu(user: User) {
    installTauri();
    render(<ServerScreen onNavigate={vi.fn()} />);
    await flush(50);
    const row = screen.getByText("homelab-pve").closest(".border-t") as HTMLElement;
    const trigger = within(row).getByRole("button", { name: /more actions/i });
    await user.click(trigger);
    return trigger;
  }

  // Bug: the overflow menu (host-row.tsx:166) closes on a click outside, a scroll or a resize,
  // but not on Escape: a keyboard player who opened it has to Tab past its items or click.
  // Suggested fix: while open, listen for Escape on document, close the menu and return focus
  // to the trigger.
  it("closes the overflow menu on Escape", async () => {
    const user = userEvent.setup();
    await openMenu(user);
    expect(screen.getByRole("menuitem", { name: /remove/i })).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem", { name: /remove/i })).toBeNull();
    // The focus goes back to the button that opened it.
    expect(document.activeElement?.getAttribute("aria-label")).toMatch(/more actions/i);
  });

  // Bug: the trigger (host-row.tsx:189) says nothing about the menu it opens: no aria-haspopup,
  // no aria-expanded, so a screen reader can't tell it opened. Suggested fix: add
  // aria-haspopup="menu" and aria-expanded={!!pos}, give the dropdown role="menu" and its items
  // role="menuitem", and move focus to the first item on open.
  it("tells assistive tech that the overflow menu is open", async () => {
    const user = userEvent.setup();
    const trigger = await openMenu(user);
    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("menu")).toBeTruthy();
  });
});
