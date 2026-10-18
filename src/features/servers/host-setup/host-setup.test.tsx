// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EMPTY_CLOUD, EMPTY_HOST, HostSetupPage, SetupTrademarks } from "@/features/servers/host-setup";
import { answer } from "@/lib/dev-mock";
import type { ServerHost, ServerHostInput } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { commands, installTauri } from "@/test/tauri";
import { clickEverything, fillEmptyFields, flush } from "@/test/ui";

const hosts = (answer("server_list", undefined) as { hosts: ServerHost[] }).hosts;
const enabled = () => screen.queryAllByRole("button").filter((b) => !b.hasAttribute("disabled"));
const FORWARD = /^(continue|next|save and test|save|test connection|connect)/i;

/** At each step: fill the empty fields, press the step's own controls, then go forward. */
async function walk(user: ReturnType<typeof userEvent.setup>, { explore = true, max = 8 } = {}) {
  const titles: string[] = [];
  for (let n = 0; n < max; n++) {
    titles.push(screen.getByRole("heading", { level: 1 }).textContent);
    await fillEmptyFields(user);
    if (explore) await clickEverything(user, { skip: /^(back|continue|next|save.*|cancel|done|close|finish)$/i, max: 12 });
    await fillEmptyFields(user);
    const forward = enabled().find((b) => FORWARD.test(b.textContent.trim()));
    if (!forward) break;
    await user.click(forward);
    await flush(30);
  }
  return titles;
}

function renderSetup(initial: ServerHostInput, report = readyReport()) {
  const handlers = { onRefresh: vi.fn(), onSaved: vi.fn(), onDone: vi.fn() };
  render(<HostSetupPage initial={initial} report={report} {...handlers} />);
  return handlers;
}

describe("HostSetupPage", () => {
  it("adds a Proxmox server step by step and tests it", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const h = renderSetup({ ...EMPTY_HOST });
    const titles = await walk(user, { explore: false });
    expect(titles.length).toBeGreaterThan(2);
    expect(commands(calls)).toContain("server_save");
    expect(h.onSaved).toHaveBeenCalled();
  });

  it("explores every server step on a fresh machine", async () => {
    installTauri();
    const user = userEvent.setup();
    renderSetup({ ...EMPTY_HOST }, freshReport("linux"));
    await walk(user);
  });

  it("explores the ESXi variant", async () => {
    installTauri();
    const user = userEvent.setup();
    renderSetup({ ...EMPTY_HOST, provider: "vmware_esxi" });
    await walk(user);
  });

  it("edits an existing server", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    renderSetup({ ...hosts[0], password: null });
    await walk(user);
    expect(commands(calls)).toContain("server_save");
  });

  it("reports a failed connection test", async () => {
    installTauri({
      server_test: () => ({ ok: false, reachable: true, authenticated: false, latencyMs: 30, message: "401 authentication failure", checks: [] }),
    });
    const user = userEvent.setup();
    renderSetup({ ...hosts[1], password: null });
    await walk(user, { explore: false });
    expect(screen.getAllByText(/authentication failure/).length).toBeGreaterThan(0);
  });

  it("adds an AWS account", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    renderSetup({ ...EMPTY_CLOUD });
    await walk(user);
    expect(commands(calls).length).toBeGreaterThan(0);
  });

  for (const provider of ["azure", "gcp", "digitalocean", "linode", "oci"] as const) {
    it(`explores the ${provider} account steps`, async () => {
      installTauri();
      const user = userEvent.setup();
      renderSetup({ ...EMPTY_CLOUD, provider, host: "" }, freshReport());
      await walk(user, { max: 6 });
    });
  }

  it("shows the trademark notices", () => {
    render(
      <>
        <SetupTrademarks cloud />
        <SetupTrademarks cloud={false} />
      </>,
    );
    expect(document.body.textContent).toMatch(/trademark/i);
  });
});
