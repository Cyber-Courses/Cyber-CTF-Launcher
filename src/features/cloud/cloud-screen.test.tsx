// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CloudScreen } from "@/features/cloud/cloud-screen";
import { AccountRow } from "@/features/cloud/account-row";
import { AccountsPanel, BudgetPanel, RunningCloudLabs } from "@/features/cloud/cloud-panels";
import type { Lab } from "@/features/labs/use-labs";
import { answer } from "@/lib/dev-mock";
import type { LabStatus, ServerHost, ServerList, ServerTest } from "@/lib/tauri";
import { commands, installTauri, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const sample = () => structuredClone(answer("server_list", undefined) as ServerList);
const aws = sample().hosts.find((h) => h.provider === "aws")!;
const account = (patch: Partial<ServerHost>): ServerHost => ({ ...aws, ...patch });
const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);
const result = (patch: Partial<ServerTest>): ServerTest => ({
  ok: true,
  reachable: true,
  authenticated: true,
  latencyMs: 40,
  message: "Connected",
  checks: [],
  ...patch,
});

async function renderScreen() {
  render(<CloudScreen />);
  await flush(50);
}

describe("CloudScreen", () => {
  it("lists only the cloud accounts, with a connect row for each main provider not connected", async () => {
    const calls = installTauri();
    await renderScreen();
    expect(screen.getByText("AWS sandbox")).toBeTruthy();
    expect(screen.queryByText("homelab-pve")).toBeNull();
    expect(screen.queryByText("Amazon Web Services")).toBeNull();
    expect(screen.getByText("Microsoft Azure")).toBeTruthy();
    expect(screen.getByText("Google Cloud")).toBeTruthy();
    expect(document.body.textContent).toContain("eu-west-3 · CLI · default · auto-stop 4h");
    // No budget set: Cost Explorer (billed per call) isn't asked.
    expect(commands(calls)).not.toContain("aws_month_to_date_cost");
    expect(screen.getByText("Not tested")).toBeTruthy();
  });

  it("reads this month's spend for an account with a budget and warns when it is over", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 5, awsProfile: "lab" });
    const calls = installTauri({ server_list: () => list, aws_month_to_date_cost: () => 6.4 });
    await renderScreen();
    expect(argsOf(calls, "aws_month_to_date_cost")).toEqual([{ profile: "lab" }]);
    expect(screen.getByText(/AWS sandbox is over its monthly budget/)).toBeTruthy();
    expect(screen.getByText("Budget guard · AWS sandbox")).toBeTruthy();
    expect(screen.getAllByText("Over budget").length).toBe(2);
    expect(document.body.textContent).toContain("budget $5/mo, $6.40 this month");
  });

  it("counts the accounts over budget", async () => {
    const list = sample();
    list.hosts = [account({ id: "a", name: "A", monthlyLimit: 1 }), account({ id: "b", name: "B", monthlyLimit: 2 })];
    installTauri({ server_list: () => list, aws_month_to_date_cost: () => 10 });
    await renderScreen();
    expect(screen.getByText(/2 accounts are over their monthly budget/)).toBeTruthy();
  });

  it("treats a budget of 0 as no limit", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 0 });
    const calls = installTauri({ server_list: () => list });
    await renderScreen();
    expect(commands(calls)).not.toContain("aws_month_to_date_cost");
    expect(screen.queryByText(/Budget guard/)).toBeNull();
    expect(screen.queryByText("Over budget")).toBeNull();
    expect(document.body.textContent).not.toContain("budget $0");
  });

  it("keeps the page up when the spend can't be read", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 50 });
    installTauri({
      server_list: () => list,
      aws_month_to_date_cost: () => {
        throw "ce:GetCostAndUsage denied";
      },
    });
    await renderScreen();
    expect(document.body.textContent).toContain("$…");
    expect(screen.queryByText(/over its monthly budget/)).toBeNull();
  });

  it("tests an account and shows each check", async () => {
    const calls = installTauri({
      server_test: () =>
        result({
          checks: [
            { name: "Credentials", state: "ok", detail: "arn:aws:iam::1:user/me" },
            { name: "Quota", state: "warn", detail: "2 vCPU left" },
          ],
        }),
    });
    const user = userEvent.setup();
    await renderScreen();
    await user.click(button(/^\s*test$/i));
    await flush();
    expect(argsOf(calls, "server_test")).toEqual([{ id: "aws1" }]);
    expect(screen.getByText("Needs attention")).toBeTruthy();
    expect(screen.getByText("2 vCPU left")).toBeTruthy();
    expect(document.body.textContent).toContain("· 40 ms");
  });

  it("removes an account after confirming, and shows why it failed", async () => {
    let fails = false;
    const calls = installTauri({
      server_remove: () => {
        if (fails) throw "keychain locked";
        return null;
      },
    });
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(screen.getByText("Remove?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByText("Remove?")).toBeNull();
    expect(commands(calls)).not.toContain("server_remove");
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await user.click(button(/^remove$/i));
    await flush();
    expect(argsOf(calls, "server_remove")).toEqual([{ id: "aws1" }]);
    fails = true;
    await user.click(button(/^remove$/i));
    await flush();
    expect(screen.getByText("keychain locked")).toBeTruthy();
  });

  it("opens the cloud setup to edit an account, connect another, or set one up", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getByRole("button", { name: "Edit" }));
    await user.click(screen.getAllByRole("button", { name: "Connect" })[0]);
    await user.click(button(/set up cloud provider/i));
    await flush();
    expect(argsOf(calls, "server_open_setup")).toEqual([
      { id: "aws1", kind: "cloud" },
      { id: null, kind: "cloud" },
      { id: null, kind: "cloud" },
    ]);
  });

  it("shows why the setup window didn't open", async () => {
    installTauri({
      server_open_setup: () => {
        throw "no window";
      },
    });
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getByRole("button", { name: "Edit" }));
    await flush();
    expect(screen.getByText("no window")).toBeTruthy();
  });

  it("shows the first run without accounts", async () => {
    const calls = installTauri({ server_list: () => ({ default: null, hosts: [sample().hosts[0]] }) });
    const user = userEvent.setup();
    await renderScreen();
    expect(screen.getByText("Run labs in your own cloud account")).toBeTruthy();
    // One "set up" button, in the first run (none in the header).
    const setUp = screen.getAllByRole("button", { name: /set up cloud provider/i });
    expect(setUp).toHaveLength(1);
    await user.click(setUp[0]);
    await flush();
    expect(argsOf(calls, "server_open_setup")).toEqual([{ id: null, kind: "cloud" }]);
  });

  it("shows loading, then the list error", async () => {
    let fail: (e: string) => void = () => {};
    installTauri({ server_list: () => new Promise((_, rej) => (fail = rej)) });
    render(<CloudScreen />);
    await flush();
    expect(screen.getByText("Loading…")).toBeTruthy();
    fail("servers.json is corrupt");
    await flush();
    expect(screen.getByText("servers.json is corrupt")).toBeTruthy();
  });
});

describe("AccountRow", () => {
  const noop = () => {};
  const row = (host: ServerHost, test?: ServerTest | "testing", spent?: number | null) =>
    render(<AccountRow host={host} test={test} spent={spent} onTest={noop} onEdit={noop} onRemove={noop} />);

  it.each([
    [{ provider: "digitalocean" as const }, "API token"],
    [{ provider: "linode" as const }, "API token"],
    [{ provider: "oci" as const }, "API key"],
    [{ provider: "azure" as const }, "CLI sign-in"],
    [{ provider: "gcp" as const }, "CLI sign-in"],
    [{ useCliCreds: true, awsProfile: null }, "CLI credentials"],
    [{ useCliCreds: false }, "access keys"],
  ])("names how %o signs in", (patch, how) => {
    row(account({ ...patch, autoStopHours: null }));
    expect(document.body.textContent).toContain(`${how} · no auto-stop`);
  });

  it("shows a failed test's message, and a passing one without checks", () => {
    row(account({}), result({ ok: false, message: "ExpiredToken", latencyMs: null }));
    expect(screen.getByText("Failed")).toBeTruthy();
    expect(screen.getByText("ExpiredToken")).toBeTruthy();
  });

  it("is Connected when every check passes, and Testing… while one runs", () => {
    const { unmount } = row(account({}), result({ checks: [{ name: "Credentials", state: "ok", detail: "fine" }] }));
    expect(screen.getByText("Connected")).toBeTruthy();
    unmount();
    row(account({}), "testing");
    expect(screen.getByText("Testing…")).toBeTruthy();
    expect(button(/^\s*test$/i).hasAttribute("disabled")).toBe(true);
  });

  it("marks a failed check", () => {
    row(account({}), result({ ok: false, checks: [{ name: "Region", state: "fail", detail: "opt-in required" }] }));
    expect(screen.getByText("opt-in required")).toBeTruthy();
    expect(screen.getByText("Failed")).toBeTruthy();
  });

  it("shows the budget with no spend read yet", () => {
    row(account({ monthlyLimit: 25 }), undefined, null);
    expect(document.body.textContent).toContain("budget $25/mo");
    expect(document.body.textContent).not.toContain("this month");
  });
});

describe("cloud panels", () => {
  it("lists the labs running on an account with their auto-stop, and stops one", async () => {
    const user = userEvent.setup();
    const onStop = vi.fn();
    const lab = (id: string, title: string) => ({ id, title }) as Lab;
    const statuses = {
      a: { running: true, host: "AWS sandbox", expiresAt: 1_900_000_000 } as LabStatus,
      b: { running: true, host: "AWS sandbox", expiresAt: null } as LabStatus,
    };
    render(<RunningCloudLabs labs={[lab("a", "Blind orders"), lab("b", "GOAD")]} statuses={statuses} isBusy={(l) => l.id === "b"} onStop={onStop} />);
    expect(document.body.textContent).toContain("AWS sandbox · auto-stops");
    const rows = screen.getAllByRole("button", { name: /stop/i });
    expect(rows[1].hasAttribute("disabled")).toBe(true);
    await user.click(rows[0]);
    expect(onStop).toHaveBeenCalledWith(expect.objectContaining({ id: "a" }));
  });

  it("hides the connect rows for the providers already connected", () => {
    const accounts = [account({}), account({ id: "z", name: "Azure lab", provider: "azure" }), account({ id: "g", name: "GCP lab", provider: "gcp" })];
    render(<AccountsPanel accounts={accounts} tests={{}} spend={{}} onTest={vi.fn()} onEdit={vi.fn()} onRemove={vi.fn()} onSetup={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "Connect" })).toBeNull();
  });

  it("shows a budget under its limit, with no auto-stop", () => {
    const { container } = render(<BudgetPanel host={account({ monthlyLimit: 100, autoStopHours: 0 })} spent={25} />);
    expect(container.textContent).toContain("$25.00");
    expect(container.textContent).toContain("of $100.00");
    expect(container.textContent).toContain("no auto-stop");
    expect(within(container).queryByText("Over budget")).toBeNull();
  });
});
