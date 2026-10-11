// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { EMPTY_CLOUD, HostSetupPage } from "@/features/servers/host-setup";
import type { ServerHostInput, SystemReport } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { installTauri, stream, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);
const heading = () => screen.getByRole("heading", { level: 2 }).textContent;
const title = () => screen.getByRole("heading", { level: 1 }).textContent;
const selects = () => screen.queryAllByRole("combobox") as HTMLSelectElement[];

/** An account being edited (the provider and tools steps are skipped). */
const editing = (patch: Partial<ServerHostInput>): ServerHostInput => ({ ...EMPTY_CLOUD, id: "acct", name: "Lab account", password: null, ...patch });

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

describe("AWS account setup", () => {
  it("edits an account with access keys: region, budget, save and test", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const h = await renderSetup(editing({ useCliCreds: true, host: "mars-1" }));
    expect(title()).toBe("Edit Lab account");
    expect(heading()).toBe("How to connect");
    // First step: Cancel, not Back.
    await go(user, /^cancel$/i);
    expect(h.onDone).toHaveBeenCalled();
    await user.click(screen.getByText("Access keys"));
    calls.length = 0;
    await go(user, /^continue$/i);
    expect(heading()).toBe("Access keys");
    // A saved region the list doesn't know stays selectable.
    expect(selects()[0].value).toBe("mars-1");
    // Editing keeps the stored secret: the key id is enough to go on.
    await user.type(document.querySelector<HTMLInputElement>("input[placeholder='AKIA…']")!, "AKIA1");
    expect(document.querySelector<HTMLInputElement>("input[type=password]")!.placeholder).toBe("Leave empty to keep");
    await user.selectOptions(selects()[0], "eu-west-1");
    await go(user, /^continue$/i);
    expect(heading()).toBe("Lab settings");
    // Access keys: the CLI's identity and spend belong to another account, so neither is read.
    expect(argsOf(calls, "aws_cli_identity")).toEqual([]);
    expect(document.body.textContent).not.toContain("Spent this month");
    await user.type(document.querySelector<HTMLInputElement>("input[type=number]")!, "30");
    await user.selectOptions(selects()[0], "0");
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({
      input: { id: "acct", provider: "aws", host: "eu-west-1", username: "AKIA1", useCliCreds: false, monthlyLimit: 30, autoStopHours: 0 },
    });
    expect(h.onSaved).toHaveBeenCalled();
    expect(heading()).toBe("Connection test");
    expect(screen.getByText("Connected")).toBeTruthy();
    expect(title()).toBe("Lab account connected");
    await go(user, /^\s*done$/i);
    expect(h.onDone).toHaveBeenCalledTimes(2);
  });

  it("reads the CLI identity and spend per profile, and warns at the budget", async () => {
    const calls = installTauri({ aws_profiles: () => ["default", "lab"], aws_month_to_date_cost: () => 12 });
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true, awsProfile: null, monthlyLimit: 10 }));
    await go(user, /^continue$/i);
    expect(heading()).toBe("AWS CLI");
    expect(document.body.textContent).toContain("Signed in as");
    expect(document.body.textContent).toContain("arn:aws:iam::482100000193:user/florian");
    await user.selectOptions(selects()[0], "lab");
    await flush();
    expect(argsOf(calls, "aws_cli_identity").at(-1)).toEqual({ profile: "lab" });
    expect(argsOf(calls, "aws_month_to_date_cost").at(-1)).toEqual({ profile: "lab" });
    await go(user, /^continue$/i);
    const spent = screen.getByText(/Spent this month/);
    expect(spent.textContent).toBe("Spent this month: $12.00 of $10.00.");
    expect(spent.className).toContain("text-destructive");
  });

  it("shows the spend without a budget", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    expect(screen.getByText("Spent this month: $6.40.")).toBeTruthy();
  });

  it("keeps the identity of the profile picked last when reads answer out of order", async () => {
    const pending: Record<string, (id: string) => void> = {};
    installTauri({
      aws_profiles: () => ["default", "lab"],
      aws_cli_identity: (a) => new Promise<string>((r) => (pending[String(a?.profile ?? "default")] = r)),
    });
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true }));
    await go(user, /^continue$/i);
    expect(screen.getByText("Checking…")).toBeTruthy();
    await user.selectOptions(selects()[0], "lab");
    await flush();
    // The newer read answers first, the stale one after it.
    await act(async () => pending.lab("arn:aws:iam::2:user/lab"));
    await act(async () => pending.default("arn:aws:iam::1:user/default"));
    await flush();
    expect(document.body.textContent).toContain("arn:aws:iam::2:user/lab");
    expect(document.body.textContent).not.toContain("arn:aws:iam::1:user/default");
  });

  it("signs in with the browser and logs who it signed in as, or that it didn't", async () => {
    let identity: string | null = null;
    const calls = installTauri({
      aws_cli_identity: () => identity,
      aws_login: (a) => {
        stream(a, "Opening browser");
        return null;
      },
    });
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true, awsProfile: "lab" }));
    await go(user, /^continue$/i);
    expect(screen.getByText("Not signed in on this profile.")).toBeTruthy();
    await go(user, /sign in \(browser\)/i);
    expect(argsOf(calls, "aws_login")[0]).toMatchObject({ profile: "lab" });
    expect(document.body.textContent).toContain("✗ Not signed in");
    identity = "arn:aws:iam::3:user/me";
    await go(user, /sign in \(browser\)/i);
    expect(document.body.textContent).toContain("Signed in asarn:aws:iam::3:user/me");
  });

  it("logs a failed browser sign-in", async () => {
    installTauri({
      aws_cli_identity: () => null,
      aws_login: () => {
        throw "aws: command not found";
      },
    });
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true }));
    await go(user, /^continue$/i);
    await go(user, /sign in \(browser\)/i);
    expect(document.body.textContent).toContain("✗ aws: command not found");
    expect(button(/sign in \(browser\)/i).hasAttribute("disabled")).toBe(false);
  });

  it("shows why the save failed and stays on the step", async () => {
    installTauri({
      server_save: () => {
        throw "keychain locked";
      },
    });
    const user = userEvent.setup();
    const h = await renderSetup(editing({ useCliCreds: true }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(screen.getByText("keychain locked")).toBeTruthy();
    expect(heading()).toBe("Lab settings");
    expect(h.onSaved).not.toHaveBeenCalled();
    await go(user, /^\s*back$/i);
    expect(heading()).toBe("AWS CLI");
  });

  it("reports a saved account whose test failed, and tests it again", async () => {
    let ok = false;
    const calls = installTauri({
      server_test: () => ({ ok, reachable: true, authenticated: false, latencyMs: null, message: ok ? "Connected" : "ExpiredToken", checks: [] }),
    });
    const user = userEvent.setup();
    await renderSetup(editing({ useCliCreds: true }));
    await go(user, /^continue$/i);
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(screen.getByText("ExpiredToken")).toBeTruthy();
    expect(title()).toBe("Lab account saved, not connected yet");
    ok = true;
    await go(user, /test again/i);
    expect(argsOf(calls, "server_test")).toHaveLength(2);
    expect(title()).toBe("Lab account connected");
  });
});

describe("Azure account setup", () => {
  it("picks the default subscription and a location", async () => {
    const calls = installTauri({
      azure_subscriptions: () => [
        { name: "Dev", id: "sub-dev", isDefault: false },
        { name: "Prod", id: "sub-prod", isDefault: true },
      ],
    });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "azure", host: "swedencentral", username: "" }));
    expect(heading()).toBe("Azure subscription");
    expect(screen.getByText("Signed in to the Azure CLI.")).toBeTruthy();
    const [subscription, location] = selects();
    expect(subscription.value).toBe("sub-prod");
    expect(subscription.textContent).toContain("Prod (default)");
    expect(screen.getByText("Location")).toBeTruthy();
    await user.selectOptions(subscription, "sub-dev");
    await user.selectOptions(location, location.options[1].value);
    await go(user, /switch account/i);
    expect(argsOf(calls, "cloud_login")[0]).toMatchObject({ provider: "azure" });
    await go(user, /^continue$/i);
    expect(screen.getByPlaceholderText("My Azure")).toBeTruthy();
    // Budgets are AWS only.
    expect(screen.queryByText("Monthly budget (USD)")).toBeNull();
    expect(document.body.textContent).toContain("the launcher tears the lab down");
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { provider: "azure", username: "sub-dev" } });
  });

  it("offers the CLI sign-in when no subscription is visible, then lists them", async () => {
    let subs: { name: string; id: string; isDefault: boolean }[] = [];
    const calls = installTauri({ azure_subscriptions: () => subs });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "azure", host: "swedencentral", username: "" }));
    expect(screen.getByText(/Sign in with the Azure CLI to list/)).toBeTruthy();
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    subs = [{ name: "Only", id: "sub-1", isDefault: false }];
    await go(user, /sign in \(az login\)/i);
    await flush(30);
    expect(argsOf(calls, "cloud_login")[0]).toMatchObject({ provider: "azure" });
    expect(screen.getByText("Signed in to the Azure CLI.")).toBeTruthy();
    expect(selects()[0].value).toBe("sub-1");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
  });

  it("checks the CLI first, and takes a pasted id when it can't list", async () => {
    let fail: (e: string) => void = () => {};
    installTauri({ azure_subscriptions: () => new Promise((_, rej) => (fail = rej)) });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "azure", host: "swedencentral", username: "" }));
    expect(screen.getByText("Checking the Azure CLI…")).toBeTruthy();
    await act(async () => fail("az not found"));
    await flush();
    await user.type(screen.getByPlaceholderText("00000000-0000-0000-0000-000000000000"), "pasted-sub");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
  });
});

describe("Google Cloud account setup", () => {
  it("defaults to the first open billing account and sets the organization", async () => {
    const calls = installTauri({
      gcp_billing_accounts: () => [
        { id: "closed-1", name: "Old", open: false },
        { id: "open-1", name: "", open: true },
      ],
      gcp_organizations: () => [{ id: "org-1", name: "Acme" }],
    });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "gcp", host: "europe-west1", username: "" }));
    expect(heading()).toBe("Google Cloud project");
    expect(document.body.textContent).toContain("player@example.com");
    const [billing, org] = selects();
    expect(billing.value).toBe("open-1");
    expect(billing.textContent).toContain("Old (closed-1) (closed)");
    expect((billing.options[0] as HTMLOptionElement).disabled).toBe(true);
    await user.selectOptions(org, "org-1");
    await user.selectOptions(org, "");
    await user.selectOptions(org, "org-1");
    await go(user, /^continue$/i);
    expect(screen.getByPlaceholderText("My Google Cloud")).toBeTruthy();
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { provider: "gcp", username: "open-1", node: "org-1" } });
  });

  it("takes a typed billing id when the account lists none", async () => {
    installTauri({ gcp_billing_accounts: () => [] });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "gcp", host: "europe-west1", username: "" }));
    expect(screen.getByText("none listed for this account; type the id")).toBeTruthy();
    await user.type(screen.getByPlaceholderText("0X0X0X-0X0X0X-0X0X0X"), "AAAAAA-BBBBBB-CCCCCC");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
  });

  it("offers the gcloud sign-in when the CLI isn't signed in, then reads the account", async () => {
    let email: string | null = null;
    const calls = installTauri({ gcp_account: () => email });
    const user = userEvent.setup();
    await renderSetup(editing({ provider: "gcp", host: "europe-west1", username: "" }));
    expect(screen.getByText("Sign in with the gcloud CLI to list your billing accounts.")).toBeTruthy();
    email = "new@example.com";
    await go(user, /sign in \(gcloud\)/i);
    await flush(30);
    expect(argsOf(calls, "cloud_login")[0]).toMatchObject({ provider: "gcp" });
    expect(document.body.textContent).toContain("new@example.com");
  });

  it("falls back to the sign-in when listing fails", async () => {
    installTauri({
      gcp_billing_accounts: () => {
        throw "PERMISSION_DENIED";
      },
    });
    await renderSetup(editing({ provider: "gcp", host: "europe-west1", username: "" }));
    expect(screen.getByText("Sign in with the gcloud CLI to list your billing accounts.")).toBeTruthy();
  });
});

describe("token and key clouds", () => {
  it.each([
    ["digitalocean", "DigitalOcean token", "https://cloud.digitalocean.com/account/api/tokens", "fra1"],
    ["linode", "Linode token", "https://cloud.linode.com/profile/tokens", "eu-central"],
  ] as const)("%s takes an API token", async (provider, name, url, host) => {
    const calls = installTauri();
    const user = userEvent.setup();
    await renderSetup(editing({ provider, host, username: "" }));
    expect(heading()).toBe(name);
    const token = document.querySelector<HTMLInputElement>("input[type=password]")!;
    expect(token.placeholder).toBe("Leave empty to keep");
    await user.type(token, "secret-token");
    await user.click(button(/create one in the/i));
    await flush();
    expect(argsOf(calls, "plugin:opener|open_url")[0]).toMatchObject({ url });
    await user.clear(token);
    await go(user, /^continue$/i);
    await go(user, /save and test/i);
    expect(argsOf(calls, "server_save")[0]).toMatchObject({ input: { provider, password: null } });
  });

  it("prefills Oracle Cloud from ~/.oci/config, keeping an edited account's region", async () => {
    installTauri({ oci_config: () => ({ configured: true, tenancy: "ocid1.tenancy.oc1..root", region: "us-ashburn-1" }) });
    await renderSetup(editing({ provider: "oci", host: "eu-frankfurt-1", username: "" }));
    expect(heading()).toBe("Oracle Cloud");
    expect(screen.getByText("Found ~/.oci/config")).toBeTruthy();
    expect(screen.getByPlaceholderText("ocid1.compartment.oc1..…")).toHaveProperty("value", "ocid1.tenancy.oc1..root");
    expect(selects()[0].value).toBe("eu-frankfurt-1");
  });

  it("says when there is no ~/.oci/config", async () => {
    installTauri({
      oci_config: () => {
        throw "no such file";
      },
    });
    await renderSetup(editing({ provider: "oci", host: "eu-frankfurt-1", username: "" }));
    expect(screen.getByText(/No ~\/.oci\/config found/)).toBeTruthy();
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
  });
});

describe("new cloud account: provider and tools", () => {
  it("picks Oracle Cloud, prefilled with the config's region", async () => {
    installTauri({ oci_config: () => ({ configured: true, tenancy: "ocid1.tenancy.oc1..root", region: "us-ashburn-1" }) });
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD });
    expect(title()).toBe("Set up cloud provider");
    await user.click(screen.getByText("Oracle Cloud"));
    await go(user, /^continue$/i);
    expect(heading()).toBe("Command-line tools");
    expect(document.body.textContent).toContain("No CLI needed");
    expect(screen.queryByText(/ CLI$/)).toBeNull();
    await go(user, /^continue$/i);
    expect(selects()[0].value).toBe("us-ashburn-1");
  });

  it("installs a missing cloud CLI and re-checks the machine", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const h = await renderSetup({ ...EMPTY_CLOUD }, freshReport());
    await user.click(screen.getByText("Microsoft Azure"));
    await go(user, /^continue$/i);
    expect(screen.getByText("Microsoft Azure CLI")).toBeTruthy();
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
    await go(user, /install az/i);
    expect(argsOf(calls, "install_dependency")[0]).toMatchObject({ dependency: "azurecli" });
    expect(document.body.textContent).toContain("✓ Installed");
    expect(h.onRefresh).toHaveBeenCalledTimes(1);
    await go(user, /install terraform/i);
    expect(argsOf(calls, "install_dependency")[1]).toMatchObject({ dependency: "terraform" });
    await go(user, /re-check/i);
    expect(h.onRefresh).toHaveBeenCalledTimes(3);
    await go(user, /^\s*back$/i);
    expect(heading()).toBe("Choose a cloud provider");
  });

  it("logs a failed install and frees the buttons", async () => {
    installTauri({
      install_dependency: () => {
        throw "winget failed";
      },
    });
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD }, freshReport("windows"));
    await go(user, /^continue$/i);
    await go(user, /install aws/i);
    expect(document.body.textContent).toContain("✗ winget failed");
    expect(button(/install aws/i).hasAttribute("disabled")).toBe(false);
  });

  it("waits for the machine check", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD }, null);
    await go(user, /^continue$/i);
    expect(screen.getByText("Checking this machine…")).toBeTruthy();
  });

  it("goes on when the tools are there", async () => {
    installTauri();
    const user = userEvent.setup();
    await renderSetup({ ...EMPTY_CLOUD });
    await go(user, /^continue$/i);
    expect(document.body.textContent).toContain("2.18.0");
    expect(document.body.textContent).toContain("1.9.8");
    await go(user, /^continue$/i);
    expect(heading()).toBe("How to connect");
    // Not the first step any more: Back, not Cancel.
    await go(user, /^\s*back$/i);
    expect(heading()).toBe("Command-line tools");
  });
});
