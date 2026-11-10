// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CloudScreen } from "@/features/cloud/cloud-screen";
import { AccountRow } from "@/features/cloud/account-row";
import { AccountsPanel, BudgetPanel, ConnectProviders, RunningCloudLabs } from "@/features/cloud/cloud-panels";
import { unconnectedProviders } from "@/features/cloud/cloud-model";
import type { Lab } from "@/features/labs/use-labs";
import { answer } from "@/lib/dev-mock";
import { invoiceStatus } from "@/lib/dev-mock/labs";
import type { LabStatus, ServerHost, ServerList, ServerTest } from "@/lib/tauri";
import { answersDialog, commands, dialogsAsked, installTauri, rejects, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const sample = () => structuredClone(answer("server_list", undefined) as ServerList);
/** The dev mock's AWS account. */
function sampleAws(): ServerHost {
  const found = sample().hosts.find((h) => h.provider === "aws");
  if (!found) throw new Error("the dev mock lists an AWS account");
  return found;
}
const aws = sampleAws();
/** A command that fails with `message`, as the Rust core's `other` error. */
const failsWith = (message: string) => rejects({ code: "other", message });
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

/** Presses Remove on the one account listed (the system dialog answers as the test set it). */
async function removeAccount(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "Remove" }));
  await flush(50);
}

describe("CloudScreen", () => {
  it("lists only the cloud accounts, and offers the providers not connected apart from them", async () => {
    const calls = installTauri();
    await renderScreen();
    expect(screen.getByText("AWS sandbox")).toBeTruthy();
    expect(screen.queryByText("homelab-pve")).toBeNull();
    expect(screen.queryByRole("button", { name: "Connect AWS" })).toBeNull();
    const connect = screen.getByRole("region", { name: "Connect a cloud provider" });
    expect(
      within(connect)
        .getAllByRole("button")
        .map((b) => b.getAttribute("aria-label")),
    ).toEqual(["Connect Azure", "Connect Google Cloud"]);
    expect(document.body.textContent).toContain("eu-west-3 · CLI · default · auto-stop 4h");
    // No budget set: Cost Explorer (billed per call) isn't asked.
    expect(commands(calls)).not.toContain("server_month_to_date_cost");
    expect(screen.getByText("Not tested")).toBeTruthy();
  });

  it("shows one account row for the one connected account, and the empty state with the connect actions once it is removed", async () => {
    // Regression: the providers not connected were rows of the accounts list ("Azure, not
    // connected"), so removing the one real account looked as if every account went with it.
    let list: ServerList = { default: null, hosts: [aws], budgets: {} };
    const calls = installTauri({
      server_list: () => list,
      server_remove: (a) => {
        list = { ...list, hosts: list.hosts.filter((h) => h.id !== a?.id) };
        return null;
      },
    });
    const user = userEvent.setup();
    await renderScreen();
    expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: /^Connect / })).toHaveLength(2);
    await removeAccount(user);
    expect(argsOf(calls, "server_remove")).toEqual([{ id: aws.id }]);
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
    expect(screen.getByText("Run labs in your own cloud account")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /^Connect / }).map((b) => b.getAttribute("aria-label"))).toEqual([
      "Connect AWS",
      "Connect Azure",
      "Connect Google Cloud",
    ]);
  });

  it("reads this month's spend for an account with a budget and warns when it is over", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 5, awsProfile: "lab" });
    list.budgets = { [list.hosts[2].id]: 5 };
    const calls = installTauri({ server_list: () => list, server_month_to_date_cost: () => ({ spent: 6.4, over: true }) });
    await renderScreen();
    // The account's own spend (its credentials), never the CLI's default profile's.
    expect(argsOf(calls, "server_month_to_date_cost")).toEqual([{ id: list.hosts[2].id }]);
    expect(commands(calls)).not.toContain("aws_month_to_date_cost");
    expect(screen.getByText(/AWS sandbox is over its monthly budget/)).toBeTruthy();
    expect(screen.getByText("Budget guard · AWS sandbox")).toBeTruthy();
    expect(screen.getAllByText("Over budget").length).toBe(2);
    expect(document.body.textContent).toContain("budget $5/mo, $6.40 this month");
  });

  it("counts the accounts over budget", async () => {
    const list = sample();
    list.hosts = [account({ id: "a", name: "A", monthlyLimit: 1 }), account({ id: "b", name: "B", monthlyLimit: 2 })];
    list.budgets = { a: 1, b: 2 };
    installTauri({ server_list: () => list, server_month_to_date_cost: () => ({ spent: 10, over: true }) });
    await renderScreen();
    expect(screen.getByText(/2 accounts are over their monthly budget/)).toBeTruthy();
  });

  it("takes which accounts have a budget, and whether one is over it, from the backend", async () => {
    // A limit of 0 is no budget: the backend lists none for it, so no spend is read.
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 0 });
    list.budgets = {};
    const calls = installTauri({ server_list: () => list });
    await renderScreen();
    expect(commands(calls)).not.toContain("server_month_to_date_cost");
    expect(screen.queryByText(/Budget guard/)).toBeNull();
    expect(screen.queryByText("Over budget")).toBeNull();
    expect(document.body.textContent).not.toContain("budget $0");
  });

  it("shows the backend's verdict on the spend, not one of its own", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 50 });
    list.budgets = { [list.hosts[2].id]: 50 };
    installTauri({ server_list: () => list, server_month_to_date_cost: () => ({ spent: 12, over: true }) });
    await renderScreen();
    expect(screen.getAllByText("Over budget").length).toBe(2);
  });

  it("keeps the page up when the spend can't be read", async () => {
    const list = sample();
    list.hosts[2] = account({ monthlyLimit: 50 });
    list.budgets = { [list.hosts[2].id]: 50 };
    installTauri({
      server_list: () => list,
      server_month_to_date_cost: failsWith("ce:GetCostAndUsage denied"),
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

  it("removes an account only once the system dialog confirms it, and shows why it failed", async () => {
    let yes = false;
    let fails = false;
    const calls = installTauri({
      "plugin:dialog|message": (a) => answersDialog(yes)(a),
      server_remove: (a) => (fails ? failsWith("keychain locked")(a) : null),
    });
    const user = userEvent.setup();
    await renderScreen();
    // Cancelled: asked in a dialog of its own (nothing inline), and nothing removed.
    await removeAccount(user);
    expect(dialogsAsked(calls)).toEqual([
      {
        title: "Remove AWS sandbox?",
        message:
          "Cyber CTF forgets this account and deletes any keys it saved for it. Nothing changes in the cloud account itself, and you can connect it again later.",
      },
    ]);
    expect(argsOf(calls, "plugin:dialog|message")[0]).toMatchObject({ kind: "warning", buttons: { OkCancelCustom: ["Remove account", "Cancel"] } });
    expect(commands(calls)).not.toContain("server_remove");
    expect(screen.queryByText(/Remove\?/)).toBeNull();
    // Confirmed: that account, and only it, after the answer.
    yes = true;
    await removeAccount(user);
    expect(commands(calls).slice(-3, -1)).toEqual(["plugin:dialog|message", "server_remove"]);
    expect(argsOf(calls, "server_remove")).toEqual([{ id: "aws1" }]);
    fails = true;
    await removeAccount(user);
    expect(screen.getByText("keychain locked")).toBeTruthy();
  });

  it("opens the cloud setup to edit an account, connect another on its own provider, or set one up", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getByRole("button", { name: "Edit" }));
    // Regression: Connect on the Azure and Google Cloud rows opened the setup on AWS.
    await user.click(screen.getByRole("button", { name: "Connect Azure" }));
    await user.click(screen.getByRole("button", { name: "Connect Google Cloud" }));
    await user.click(button(/set up cloud provider/i));
    await flush();
    expect(argsOf(calls, "server_open_setup")).toEqual([
      { id: "aws1", kind: "cloud" },
      { id: null, kind: "cloud", provider: "azure" },
      { id: null, kind: "cloud", provider: "gcp" },
      { id: null, kind: "cloud" },
    ]);
  });

  it("shows why the setup window didn't open", async () => {
    installTauri({
      server_open_setup: failsWith("no window"),
    });
    const user = userEvent.setup();
    await renderScreen();
    await user.click(screen.getByRole("button", { name: "Edit" }));
    await flush();
    expect(screen.getByText("no window")).toBeTruthy();
  });

  it("shows the first run without accounts, connecting a main provider or any other", async () => {
    const calls = installTauri({ server_list: () => ({ default: null, hosts: [sample().hosts[0]], budgets: {} }) });
    const user = userEvent.setup();
    await renderScreen();
    expect(screen.getByText("Run labs in your own cloud account")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
    // One "set up" button, in the first run (none in the header).
    const setUp = screen.getAllByRole("button", { name: /set up cloud provider/i });
    expect(setUp).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Connect AWS" }));
    await user.click(setUp[0]);
    await flush();
    expect(argsOf(calls, "server_open_setup")).toEqual([
      { id: null, kind: "cloud", provider: "aws" },
      { id: null, kind: "cloud" },
    ]);
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
  const row = (host: ServerHost, test?: ServerTest | "testing", limit?: number) =>
    render(<AccountRow host={host} test={test} limit={limit} onTest={noop} onEdit={noop} onRemove={noop} />);

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
    row(account({ monthlyLimit: 25 }), undefined, 25);
    expect(document.body.textContent).toContain("budget $25/mo");
    expect(document.body.textContent).not.toContain("this month");
  });
});

describe("CloudScreen's Stop", () => {
  it("says why a Stop was refused, though the refusal writes no line to the lab's log", async () => {
    const user = userEvent.setup();
    const why = "Another operation on this lab is already in progress; wait for it to finish.";
    installTauri({
      lab_status: (a) => (a?.id === "lab-invoice" ? { ...invoiceStatus, host: aws.name, hostId: aws.id } : answer("lab_status", a)),
      lab_stop: rejects({ code: "alreadyInProgress", message: why }),
    });
    await renderScreen();
    expect(document.body.textContent).not.toContain(why);
    await user.click(screen.getByRole("button", { name: /stop/i }));
    await flush(50);
    expect(document.body.textContent).toContain(why);
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

  it("lists the accounts and nothing else", () => {
    const accounts = [account({}), account({ id: "z", name: "Azure lab", provider: "azure" })];
    render(<AccountsPanel accounts={accounts} tests={{}} budgets={{}} spend={{}} onTest={vi.fn()} onEdit={vi.fn()} onRemove={vi.fn()} />);
    expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(2);
    expect(screen.queryByText("Google Cloud")).toBeNull();
  });

  it("offers each provider not connected, and nothing once all are", async () => {
    const user = userEvent.setup();
    const onConnect = vi.fn();
    const { unmount } = render(<ConnectProviders providers={unconnectedProviders([account({ provider: "azure" })])} onConnect={onConnect} />);
    await user.click(screen.getByRole("button", { name: "Connect Google Cloud" }));
    expect(onConnect).toHaveBeenCalledWith("gcp");
    await user.click(screen.getByRole("button", { name: "Connect AWS" }));
    expect(onConnect).toHaveBeenLastCalledWith("aws");
    unmount();
    const all = [account({}), account({ provider: "azure" }), account({ provider: "gcp" })];
    const { container } = render(<ConnectProviders providers={unconnectedProviders(all)} onConnect={onConnect} />);
    expect(container.textContent).toBe("");
  });

  it("shows a budget under its limit, with no auto-stop", () => {
    const { container } = render(<BudgetPanel host={account({ monthlyLimit: 100, autoStopHours: 0 })} limit={100} spend={{ spent: 25, over: false }} />);
    expect(container.textContent).toContain("$25.00");
    expect(container.textContent).toContain("of $100.00");
    expect(container.textContent).toContain("no auto-stop");
    expect(within(container).queryByText("Over budget")).toBeNull();
  });
});
