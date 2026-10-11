// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SettingsScreen } from "@/features/settings/settings-screen";
import { getPortMode } from "@/lib/settings";
import { readyReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { clickEverything, flush } from "@/test/ui";

const signedIn = { loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" };

async function openTab(name: RegExp) {
  const user = userEvent.setup();
  const nav = screen.getByRole("navigation");
  await user.click(within(nav).getByRole("button", { name }));
  await flush();
  return user;
}

describe("SettingsScreen", () => {
  it("shows the account section first, with the signed-in player", async () => {
    installTauri();
    render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
    await flush();
    expect(screen.getAllByText(/Florian Amette/).length).toBeGreaterThan(0);
  });

  for (const tab of [/account/i, /appearance/i, /labs/i, /maintenance/i, /about/i]) {
    it(`presses every control of the ${tab.source} section`, async () => {
      const calls = installTauri();
      render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
      await flush();
      const user = await openTab(tab);
      expect(within(screen.getByRole("navigation")).getByRole("button", { name: tab }).getAttribute("aria-current")).toBe("page");
      await clickEverything(user, { skip: /^(Account|Appearance|Labs|Maintenance|About)$/ });
      expect(calls.length).toBeGreaterThan(0);
    });
  }

  it("saves the lab port mode from the Labs section", async () => {
    installTauri();
    render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
    await flush();
    const user = await openTab(/labs/i);
    const before = getPortMode();
    const radios = screen.getAllByRole("radio");
    for (const r of radios) if (r.getAttribute("aria-checked") !== "true") await user.click(r);
    await flush();
    expect(getPortMode()).not.toBe(before);
  });

  it("still opens when the version, the machine check and the agent can't be read", async () => {
    installTauri({
      "plugin:app|version": () => {
        throw "no version";
      },
      system_check: () => {
        throw "check failed";
      },
      agent_info: () => {
        throw "not registered";
      },
    });
    render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
    await flush();
    expect(document.body.textContent).toContain("Registering…");
    await openTab(/about/i);
    expect(document.body.textContent).toContain("…");
    expect(document.body.textContent).not.toContain("Install ID");
    await openTab(/labs/i);
    expect(document.body.textContent).toContain("Checking hypervisors…");
  });

  it("re-checks the machine after a hypervisor add-on install", async () => {
    const calls = installTauri({
      system_check: () => ({
        ...readyReport(),
        os: "linux",
        vmProviders: [{ provider: "qemu", remote: false, available: false, hypervisor: true, plugin: "vagrant-qemu", pluginInstalled: false, reason: null }],
      }),
    });
    render(<SettingsScreen auth={signedIn} onAuthChange={vi.fn()} onNavigate={vi.fn()} />);
    await flush();
    const user = await openTab(/labs/i);
    await user.click(screen.getByRole("button", { name: /install add-on/i }));
    await flush();
    expect(calls.filter((c) => c.cmd === "system_check")).toHaveLength(2);
    expect(document.body.textContent).toContain("✓ Installed");
  });

  it("works signed out", async () => {
    installTauri({ auth_status: () => ({ loggedIn: false, name: null, email: null }) });
    const onAuthChange = vi.fn();
    render(<SettingsScreen auth={{ loggedIn: false, name: null, email: null }} onAuthChange={onAuthChange} onNavigate={vi.fn()} />);
    await flush();
    const user = userEvent.setup();
    const signIn = screen.queryAllByRole("button").find((b) => /sign in|log in/i.test(b.textContent ?? ""));
    if (signIn) {
      await user.click(signIn);
      await flush();
      expect(onAuthChange).toHaveBeenCalled();
    }
  });
});
