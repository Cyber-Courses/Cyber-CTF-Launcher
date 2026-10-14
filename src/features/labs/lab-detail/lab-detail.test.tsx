// @vitest-environment jsdom
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabDetail } from "@/features/labs/lab-detail";
import type { Lab } from "@/features/labs/use-labs";
import type { LabMachine, LabStatus } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";

const dockerLab: Lab = {
  id: "lab-invoice",
  slug: "invoice-portal-api",
  title: "Invoice portal API",
  description: "A billing portal exposes a REST API guarded by a static key.",
  question: "What is the total amount of invoice INV-2041?",
  difficulty: 1,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: ["aws"], hosted: true },
  skills: [{ id: "s1", name: "API keys" }],
};
const vmLab: Lab = {
  ...dockerLab,
  id: "lab-goad",
  slug: "goad-light",
  title: "GOAD Light",
  question: null,
  runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["virtualbox", "vmware_esxi", "proxmox"] },
};

const machine = (name: string, state = "running", p: Partial<LabMachine> = {}): LabMachine => ({
  name,
  state,
  image: "node:20",
  ip: "10.42.0.3",
  ports: [{ published: 8080, target: 8080 }],
  interfaces: [{ network: "default", ip: "10.42.0.3" }],
  services: [{ name: "api", kind: "web", ports: [8080] }],
  infra: false,
  ...p,
});
const status = (p: Partial<LabStatus>): LabStatus => ({
  running: true,
  parked: null,
  machines: [machine("api"), machine("db", "running", { ip: "10.42.0.4", services: [{ name: "postgres", kind: "database", ports: [5432] }] })],
  networks: [{ name: "default", subnet: "10.42.0.0/24", internal: false }],
  url: "http://127.0.0.1:8080",
  host: null,
  expiresAt: null,
  place: "container",
  provider: "docker",
  attacker: null,
  outputs: [],
  message: null,
  reapError: null,
  ...p,
});

// The dev mock reports GOAD mid-deploy; these tests want it settled.
const idle = { deploying_labs: () => [], active_operations: () => [] };

const flush = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 30));
  });

function renderDetail(props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const handlers = { onBack: vi.fn(), onStart: vi.fn(), onStop: vi.fn(), onPark: vi.fn(), onResume: vi.fn(), onProvision: vi.fn(), onLogin: vi.fn() };
  render(
    <LabDetail lab={dockerLab} busy={false} logs={[]} times={[]} loggedIn hostArch="x86_64" readyVms={["virtualbox"]} dockerRunning {...handlers} {...props} />,
  );
  return handlers;
}

describe("LabDetail", () => {
  it("offers to start a stopped Docker lab and starts it", async () => {
    installTauri({ lab_status: () => status({ running: false, machines: [], url: null }) });
    const user = userEvent.setup();
    const h = renderDetail();
    expect(screen.getByRole("heading", { name: "Invoice portal API" })).toBeTruthy();
    expect(screen.getByText("What is the total amount of invoice INV-2041?")).toBeTruthy();
    await flush();
    // Esc on the page goes back to the list.
    await user.keyboard("{Escape}");
    expect(h.onBack).toHaveBeenCalledTimes(1);
    // With servers and hosted runs to choose from, Start asks where first.
    await user.click(screen.getByRole("button", { name: /start lab/i }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog)
      .getAllByRole("button")
      .filter((b) => /start/i.test(b.textContent ?? ""));
    // Pick the lab's default ports, then the AWS account and back to this machine.
    await user.click(within(dialog).getByText("The lab's default ports"));
    await user.click(within(dialog).getByText("AWS sandbox"));
    await user.click(within(dialog).getAllByText("This machine")[1]);
    await user.click(confirm[confirm.length - 1]);
    await flush();
    expect(h.onStart).toHaveBeenCalledWith(expect.objectContaining({ kind: "local" }));
  });

  it("shows a running Docker lab with its network, details and actions", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = renderDetail({ status: status({}) });
    await flush();
    // The diagram's code loads on first use (network-panel.tsx).
    // Under a full parallel run that import can take more than the default second.
    expect((await screen.findAllByText(/10\.42\.0\.3/, {}, { timeout: 10_000 })).length).toBeGreaterThan(0);
    // Every header action that doesn't leave the page.
    for (const b of screen.getAllByRole("button")) {
      if (/stop|remove|delete/i.test(b.textContent ?? "")) {
        await user.click(b);
        await flush();
        break;
      }
    }
    const dialogButtons = screen.queryAllByRole("button", { name: /cancel/i });
    if (dialogButtons[0]) await user.click(dialogButtons[0]);
    expect(h.onBack).not.toHaveBeenCalled();
  });

  it("draws the diagram across the page on a wide window, in the main column otherwise", async () => {
    // A window that can be resized past the wide breakpoint (and back).
    let wide = false;
    const listeners = new Set<() => void>();
    vi.stubGlobal("matchMedia", (query: string) => ({
      get matches() {
        return query === "(min-width: 96rem)" && wide;
      },
      media: query,
      addEventListener: (_: string, l: () => void) => listeners.add(l),
      removeEventListener: (_: string, l: () => void) => listeners.delete(l),
    }));
    const resize = (to: boolean) =>
      act(() => {
        wide = to;
        listeners.forEach((l) => l());
      });
    try {
      installTauri();
      renderDetail({ status: status({}) });
      await flush();
      const card = () => document.querySelector(".hostcard")!;
      const grid = () => document.querySelector(".grid")!;
      // Narrow: in the main column, beside the rail.
      expect(grid().contains(card())).toBe(true);
      resize(true);
      expect(card()).not.toBeNull();
      expect(grid().contains(card())).toBe(false);
      expect(card().compareDocumentPosition(grid()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      // Only one diagram, whichever the place.
      expect(document.querySelectorAll(".hostcard")).toHaveLength(1);
      resize(false);
      expect(grid().contains(card())).toBe(true);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("reports a container that went down and resets the lab", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = renderDetail({ status: status({ machines: [machine("api"), machine("db", "exited", { ip: "" })] }) });
    await flush();
    expect(screen.getAllByText(/db/).length).toBeGreaterThan(0);
    const reset = screen.queryAllByRole("button").find((b) => /reset|restart/i.test(b.textContent ?? ""));
    if (reset) {
      await user.click(reset);
      await flush();
      expect(h.onStop).toHaveBeenCalled();
    }
  });

  it("follows a deploy in progress with its log", async () => {
    installTauri();
    renderDetail({
      lab: vmLab,
      busy: true,
      status: status({ running: false, machines: [], place: "local_vm", provider: "virtualbox", url: null }),
      logs: ["==> DC01: Importing base box", "==> DC01: Booting VM...", "TASK [Configure SRV02] ***"],
      times: [1, 2, 3],
    });
    await flush();
    expect(screen.getAllByText(/Starting/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/DC01/).length).toBeGreaterThan(0);
  });

  it("offers to run a broken VM setup again", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    const vmMachines = [machine("DC01", "running", { image: "", ip: "192.168.56.10", ports: [], services: [] })];
    const h = renderDetail({
      lab: vmLab,
      status: status({ machines: vmMachines, place: "local_vm", provider: "virtualbox", url: null }),
      logs: ["==> DC01: Booting VM...", "✗ Provisioning failed on DC01"],
      times: [1, 2],
    });
    await flush();
    await user.click(screen.getAllByRole("button", { name: /re-run setup|run setup/i })[0]);
    await flush();
    expect(h.onProvision).toHaveBeenCalled();
  });

  it("shows a running VM lab with re-provisioning and parking", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    const vmMachines = [
      machine("DC01", "running", { image: "", ip: "192.168.56.10", ports: [], services: [] }),
      machine("SRV02", "running", { image: "", ip: "192.168.56.22", ports: [], services: [] }),
    ];
    const h = renderDetail({ lab: vmLab, status: status({ machines: vmMachines, place: "local_vm", provider: "virtualbox", url: null }) });
    await flush();
    expect(screen.getAllByText(/DC01/).length).toBeGreaterThan(0);
    const pause = screen.queryAllByRole("button").find((b) => /pause|suspend/i.test(b.textContent ?? ""));
    if (pause) {
      await user.click(pause);
      await flush();
    }
    expect(h.onBack).not.toHaveBeenCalled();
  });

  it("offers resume for a parked VM lab", async () => {
    installTauri(idle);
    const user = userEvent.setup();
    const h = renderDetail({
      lab: vmLab,
      status: status({ running: false, parked: "pause", machines: [], place: "local_vm", provider: "virtualbox", url: null }),
    });
    await flush();
    await user.click(screen.getAllByRole("button").find((b) => /resume/i.test(b.textContent ?? ""))!);
    await flush();
    expect(h.onResume).toHaveBeenCalled();
  });

  it("asks a signed-out player to sign in", async () => {
    installTauri();
    renderDetail({ loggedIn: false, status: status({ running: false, machines: [], url: null }) });
    await flush();
    expect(screen.getAllByText(/sign in/i).length).toBeGreaterThan(0);
  });

  it("shows a placeholder until the first checks are in", () => {
    installTauri();
    renderDetail({ ready: false });
    expect(screen.queryByRole("heading", { name: "Invoice portal API" })).toBeNull();
  });

  it("shows a lab on a server host", async () => {
    installTauri();
    renderDetail({
      lab: vmLab,
      status: status({ place: "server", provider: "proxmox", host: "homelab-pve", url: null, expiresAt: Math.floor(Date.now() / 1000) + 3600 }),
    });
    await flush();
    expect(screen.getAllByText(/homelab-pve/).length).toBeGreaterThan(0);
  });
});
