// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CloudScreen } from "@/features/cloud/cloud-screen";
import { HomeScreen } from "@/features/home/home-screen";
import { Labs } from "@/features/labs/labs-screen";
import { MachineScreen } from "@/features/machine/machine-screen";
import { ServerScreen } from "@/features/servers/servers-screen";
import { freshReport, readyReport } from "@/test/fixtures";
import { commands, installTauri } from "@/test/tauri";
import { clickEverything, flush } from "@/test/ui";
import { must } from "@/components/test-support";

const signedIn = { loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" };
// Controls that leave the screen or open another window: pressed in their own tests.
const LEAVE = /^(home|labs|overview)$/i;

describe("MachineScreen", () => {
  it("shows a ready machine and presses its controls", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={onNavigate} />);
    await flush(50);
    expect(screen.getAllByText(/Docker Desktop/).length).toBeGreaterThan(0);
    await clickEverything(user, { skip: LEAVE, max: 30 });
    expect(commands(calls)).toContain("machine_metrics");
  });

  it("lists what a fresh machine is missing", async () => {
    const calls = installTauri({ machine_workloads: () => [], machine_storage: () => ({ images: [], boxes: [], plugins: [] }) });
    const user = userEvent.setup();
    render(<MachineScreen report={freshReport("windows")} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    await clickEverything(user, { skip: LEAVE, max: 20 });
    expect(commands(calls)).toContain("machine_open_setup");
  });
});

describe("ServerScreen", () => {
  it("lists the servers and presses their controls", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<ServerScreen onNavigate={vi.fn()} />);
    await flush(50);
    expect(screen.getAllByText("homelab-pve").length).toBeGreaterThan(0);
    await clickEverything(user, { skip: LEAVE, max: 30 });
    expect(commands(calls)).toContain("server_list");
  });

  it("offers to add the first server", async () => {
    const calls = installTauri({ server_list: () => ({ default: null, hosts: [] }) });
    const user = userEvent.setup();
    render(<ServerScreen onNavigate={vi.fn()} />);
    await flush(50);
    await clickEverything(user, { max: 5 });
    expect(commands(calls)).toContain("server_open_setup");
  });
});

describe("CloudScreen", () => {
  it("lists the cloud accounts with their spend", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<CloudScreen />);
    await flush(50);
    expect(screen.getAllByText("AWS sandbox").length).toBeGreaterThan(0);
    await clickEverything(user, { max: 20 });
    expect(commands(calls)).toContain("server_list");
  });

  it("shows the first-run setup without accounts", async () => {
    const calls = installTauri({ server_list: () => ({ default: null, hosts: [] }) });
    const user = userEvent.setup();
    render(<CloudScreen />);
    await flush(50);
    await clickEverything(user, { max: 5 });
    expect(commands(calls)).toContain("server_open_setup");
  });
});

describe("HomeScreen", () => {
  it("shows the dashboard for a signed-in player", async () => {
    installTauri();
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={onNavigate} />);
    await flush(50);
    expect(screen.getAllByText(/Invoice portal API/).length).toBeGreaterThan(0);
    await clickEverything(user, { max: 20 });
    expect(onNavigate).toHaveBeenCalled();
  });

  it("shows a signed-out player on a fresh machine", async () => {
    installTauri({ running_labs: () => [], deploying_labs: () => [], active_operations: () => [] });
    render(<HomeScreen report={freshReport()} auth={{ loggedIn: false, name: null, email: null }} onNavigate={vi.fn()} />);
    await flush(50);
    expect(document.body.textContent).toMatch(/sign in/i);
  });
});

describe("Labs", () => {
  const props = {
    loggedIn: true,
    authReady: true,
    onLogin: vi.fn(async () => {}),
    hostArch: "x86_64",
    report: readyReport(),
    openLab: { slug: null, tick: 0 },
    onDetailChange: vi.fn(),
  };

  it("lists, searches and filters the labs", async () => {
    installTauri();
    const user = userEvent.setup();
    render(<Labs {...props} />);
    await flush(50);
    expect(screen.getAllByText("Blind orders").length).toBeGreaterThan(0);
    const search = must(document.querySelector<HTMLInputElement>("[data-lab-search]"), "the lab search");
    await user.type(search, "GOAD");
    await flush();
    expect(screen.queryAllByText("Blind orders").length).toBe(0);
    await user.clear(search);
    await clickEverything(user, { skip: /invoice|goad|blind|exposed|public backup/i, max: 25 });
  });

  it("opens a lab's page from a deep link and goes back", async () => {
    installTauri();
    const user = userEvent.setup();
    const onDetailChange = vi.fn();
    render(<Labs {...props} openLab={{ slug: "invoice-portal-api", tick: 1 }} onDetailChange={onDetailChange} />);
    await flush(80);
    expect(onDetailChange).toHaveBeenCalledWith("Invoice portal API");
    await user.click(screen.getByRole("button", { name: /all labs/i }));
    await flush();
    expect(onDetailChange).toHaveBeenLastCalledWith(null);
  });

  it("starts and stops a lab from its page", async () => {
    const calls = installTauri({
      lab_status: () => ({
        running: false,
        parked: null,
        url: null,
        host: null,
        expiresAt: null,
        place: null,
        provider: null,
        networks: [],
        machines: [],
        attacker: null,
        outputs: [],
        message: null,
      }),
    });
    const user = userEvent.setup();
    render(<Labs {...props} openLab={{ slug: "sqli-blind-orders", tick: 1 }} />);
    await flush(80);
    await user.click(screen.getByRole("button", { name: /start lab/i }));
    await flush();
    const dialog = screen.queryByRole("dialog");
    if (dialog) {
      const go = Array.from(must(dialog.parentElement, "the dialog's backdrop").querySelectorAll("button")).filter((b) => /start/i.test(b.textContent));
      await user.click(go[go.length - 1]);
    }
    await flush(80);
    expect(commands(calls)).toContain("lab_launch");
  });

  it("asks a signed-out player to sign in", async () => {
    installTauri({ auth_status: () => ({ loggedIn: false, name: null, email: null }) });
    const user = userEvent.setup();
    const onLogin = vi.fn(async () => {});
    render(<Labs {...props} loggedIn={false} onLogin={onLogin} openLab={{ slug: "s3-public-backup", tick: 1 }} />);
    await flush(50);
    const signIn = screen.getAllByRole("button").find((b) => /sign in/i.test(b.textContent));
    await user.click(must(signIn, "a Sign in button"));
    expect(onLogin).toHaveBeenCalled();
  });
});
