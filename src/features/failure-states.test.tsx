// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HomeScreen } from "@/features/home/home-screen";
import { LabDetail } from "@/features/labs/lab-detail";
import { MachineSetup } from "@/features/machine/machine-setup";
import { Labs } from "@/features/labs/labs-screen";
import type { Lab } from "@/features/labs/use-labs";
import { useLabActions } from "@/features/labs/use-lab-actions";
import { must } from "@/components/test-support";
import { Toaster } from "@/components/ui/toaster";
import { getDeploySnapshot, runFailure, startFailed } from "@/features/labs/runs/deploy-runs";
import { noteOperations, operationSeenAt } from "@/features/labs/runs/operations";
import type { AuthStatus, LabStatus } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { commands, installTauri, rejectWith } from "@/test/tauri";
import { answer } from "@/lib/dev-mock";
import { button, flush } from "@/test/ui";

// The failure states of the UX audit: each one says what happened and offers the way out,
// instead of a state that never resolves or reads as nothing happened.

const signedIn: AuthStatus = { loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" };
const API_DOWN = "error sending request for url (https://api.cyberctf.org/graphql): connection refused";
const PORT_CLASH = "Host port 8080 is already in use on this machine (another lab or app holds it). Stop whatever uses it, or start the lab with random ports.";

const blind: Lab = {
  id: "lab-sqli",
  slug: "sqli-blind-orders",
  title: "Blind orders",
  description: "Pull a customer's email through blind SQL injection.",
  question: null,
  difficulty: 2,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: ["arm64", "x86_64"], providers: [] },
  skills: [],
};
const goad: Lab = {
  ...blind,
  id: "lab-goad",
  slug: "goad-light",
  title: "GOAD Light",
  runtime: { runtime: "VM", architectures: ["x86_64"], providers: ["proxmox"] },
};

const stopped: LabStatus = {
  running: false,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  attacker: null,
  outputs: [],
  message: null,
  reapError: null,
};
const up: LabStatus = {
  ...stopped,
  running: true,
  place: "container",
  provider: "docker",
  machines: [
    {
      name: "api",
      state: "running",
      image: "node:20",
      ip: "10.42.0.3",
      ports: [],
      interfaces: [{ network: "default", ip: "10.42.0.3" }],
      services: [],
      infra: false,
    },
  ],
  networks: [{ name: "default", subnet: "10.42.0.0/24", internal: false }],
};

const idle = { deploying_labs: () => [], active_operations: () => [] };

function renderDetail(props: Partial<Parameters<typeof LabDetail>[0]> = {}) {
  const handlers = { onBack: vi.fn(), onStart: vi.fn(), onStop: vi.fn(), onPark: vi.fn(), onResume: vi.fn(), onProvision: vi.fn() };
  render(<LabDetail lab={blind} busy={false} logs={[]} times={[]} loggedIn hostArch="x86_64" readyVms={[]} dockerRunning {...handlers} {...props} />);
  return handlers;
}

afterEach(() => vi.restoreAllMocks());

describe("the catalogue can't be reached while labs run", () => {
  it("keeps the running lab on the Overview by its id, and offers Retry instead of loading forever", async () => {
    let down = true;
    const calls = installTauri({
      ...idle,
      api_query: (a) => {
        if (down) throw new Error(API_DOWN);
        return answer("api_query", a);
      },
    });
    const user = userEvent.setup();
    render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={vi.fn()} />);
    // The scan (running_labs) sees lab-invoice; no catalogue says what it is.
    await flush(80);
    expect(document.body.textContent).toContain("1 lab running on this machine.");
    expect(screen.getByText("Invoice")).toBeTruthy();
    expect(document.body.textContent).toContain("no details while the catalogue can’t be reached");
    expect(document.body.textContent).not.toContain("Nothing running");
    expect(document.body.textContent).not.toContain("Loading labs…");
    expect(document.body.textContent).toContain("Can’t reach the lab catalogue");

    // Retry reads the catalogue again, and its labs show.
    down = false;
    await user.click(button(/^\s*Retry$/));
    await flush(80);
    expect(document.body.textContent).not.toContain("Can’t reach the lab catalogue");
    expect(document.body.textContent).toContain("Invoice portal API");
    expect(commands(calls).filter((c) => c === "api_query").length).toBeGreaterThan(2);
  });

  it("lists the running lab above the Labs screen's error too", async () => {
    installTauri({
      ...idle,
      api_query: () => {
        throw new Error(API_DOWN);
      },
    });
    render(<Labs loggedIn hostArch="x86_64" report={readyReport()} openLab={{ slug: null, tick: 0 }} />);
    await flush(80);
    expect(document.body.textContent).toContain("Can’t reach the lab catalogue");
    expect(document.body.textContent).toContain("Running now");
    expect(document.body.textContent).toContain("lab-invoice");
  });
});

describe("a start that failed", () => {
  it("reads Failed to start, offers Retry and, for a port clash, Retry on random ports with a hint", async () => {
    installTauri({ ...idle, lab_status: () => stopped });
    const user = userEvent.setup();
    const h = renderDetail({ logs: [`✗ ${PORT_CLASH}`], times: [2000], runOp: "launch", runStartedAt: 500 });
    await flush();
    expect(document.body.textContent).toContain("Failed to start");
    expect(document.body.textContent).not.toContain("Not started");
    expect(screen.getByRole("button", { name: /^\s*Retry$/ })).toBeTruthy();
    // Under the header: what failed, the error and what to do about it.
    const notice = must(
      screen.getAllByRole("alert").find((a) => a.textContent.includes("Couldn’t start Blind orders")),
      "Couldn’t start Blind orders",
    );
    expect(notice.textContent).toContain(PORT_CLASH);
    expect(notice.textContent).toContain("Start it on random ports instead.");
    // Timed from when it began, not from its one line: 1.5s, not 0.0s.
    expect(document.body.textContent).toContain("1.5s");
    expect(document.body.textContent).not.toContain("0.0s");

    await user.click(screen.getByRole("button", { name: /Retry with random ports/ }));
    expect(h.onStart).toHaveBeenCalledWith({ kind: "local", ports: "random" });

    // Dismissed, the notice goes; the header still says so.
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryAllByRole("alert").some((a) => a.textContent.includes("Couldn’t start"))).toBe(false);
    expect(document.body.textContent).toContain("Failed to start");
  });

  it("offers no port retry for another cause, and a failed stop is not a failed start", async () => {
    installTauri({ ...idle, lab_status: () => stopped });
    renderDetail({ logs: ["✗ Docker isn't running. Start your container engine."], times: [10], runOp: "launch" });
    await flush();
    expect(document.body.textContent).toContain("Failed to start");
    expect(document.body.textContent).toContain("Start your container engine, then retry.");
    expect(screen.queryByRole("button", { name: /random ports/ })).toBeNull();
  });

  it("marks the row, from the run in the store", async () => {
    installTauri({ ...idle, lab_launch: () => rejectWith(PORT_CLASH) });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    let actions: ReturnType<typeof useLabActions> | null = null;
    function Probe() {
      actions = useLabActions(() => {});
      return null;
    }
    render(
      <>
        <Probe />
        <Toaster />
      </>,
    );
    await act(async () => {
      await must(actions, "the lab actions").launch(blind, null, undefined, null, "default");
    });
    const run = getDeploySnapshot().runs[blind.id];
    expect(startFailed(run)).toBe(true);
    expect(runFailure(run)).toBe(PORT_CLASH);
    // Said wherever the player is: a toast.
    expect(document.body.textContent).toContain("Couldn’t start Blind orders");
  });
});

describe("a Stop & remove that failed", () => {
  it("says so under the header and keeps the lab's panels through the stop", async () => {
    installTauri({ ...idle, lab_status: () => up });
    const { rerender } = render(
      <LabDetail
        lab={blind}
        status={up}
        busy
        logs={[]}
        times={[]}
        runOp="stop"
        loggedIn
        hostArch="x86_64"
        readyVms={[]}
        dockerRunning
        onBack={vi.fn()}
        onStart={vi.fn()}
        onStop={vi.fn()}
      />,
    );
    await flush();
    // While it stops: the header says so, the panels stay (dimmed, out of reach).
    expect(document.body.textContent).toContain("Stopping");
    const inert = document.querySelectorAll("[inert]");
    expect(inert.length).toBeGreaterThan(0);
    expect(document.body.textContent).toContain("Attack box");

    rerender(
      <LabDetail
        lab={blind}
        status={up}
        busy={false}
        logs={["✗ Couldn't remove network lab-invoice_default: has active endpoints"]}
        times={[Date.now()]}
        runOp="stop"
        runStartedAt={Date.now() - 2000}
        loggedIn
        hostArch="x86_64"
        readyVms={[]}
        dockerRunning
        onBack={vi.fn()}
        onStart={vi.fn()}
        onStop={vi.fn()}
      />,
    );
    await flush();
    const notice = must(
      screen.getAllByRole("alert").find((a) => a.textContent.includes("Couldn’t stop and remove Blind orders")),
      "Couldn’t stop and remove Blind orders",
    );
    expect(notice.textContent).toContain("has active endpoints");
    expect(notice.textContent).toContain("Part of the lab is still up.");
    // Back to running, not dimmed; still not a failed start.
    expect(document.querySelectorAll("[inert]").length).toBe(0);
    expect(document.body.textContent).not.toContain("Failed to start");
  });
});

describe("a deploy with no log here", () => {
  it("shows the backend's step and counts from when the operation was first seen", async () => {
    const step = "Configure SRV02 (ansible 41/88)";
    installTauri({
      deploying_labs: () => [goad.id],
      active_operations: () => [{ labId: goad.id, op: "launch", machine: "SRV02", step }],
      lab_deploy_log: () => "",
      lab_status: () => stopped,
    });
    // First seen a minute ago (this window saw it then, before the page opened).
    noteOperations([{ labId: goad.id, op: "launch", machine: "SRV02", step }], Date.now() - 61_000);
    render(
      <LabDetail lab={goad} busy={false} logs={[]} times={[]} loggedIn hostArch="x86_64" readyVms={[]} onBack={vi.fn()} onStart={vi.fn()} onStop={vi.fn()} />,
    );
    await flush(2200);
    expect(document.body.textContent).toContain(step);
    expect(document.body.textContent).not.toContain("Preparing…");
    // One word for it: Starting (not Building, not Deploying).
    expect(document.body.textContent).toContain("Starting…");
    expect(document.body.textContent).not.toMatch(/Building|Deploying/);
    expect(document.body.textContent).toMatch(/Starting… 1:0\d/);
  });

  it("keeps when an operation was first seen until it ends", () => {
    const op = { labId: "lab-x", op: "launch" as const, machine: null, step: null };
    noteOperations([op], 1000);
    noteOperations([op], 5000);
    expect(operationSeenAt("lab-x", "launch")).toBe(1000);
    noteOperations([], 6000);
    expect(operationSeenAt("lab-x", "launch")).toBeUndefined();
  });
});

describe("the machine check failed", () => {
  it("says so on the Overview with Try again, not Checking this machine… forever", async () => {
    installTauri({ ...idle, machine_metrics: () => Promise.reject(new Error("metrics unavailable")) });
    const user = userEvent.setup();
    const retry = vi.fn();
    render(<HomeScreen report={null} checkError="failed to run `docker info`: permission denied" onRetryCheck={retry} auth={signedIn} onNavigate={vi.fn()} />);
    await flush(80);
    expect(document.body.textContent).toContain("Couldn’t check this machine.");
    expect(document.body.textContent).toContain("permission denied");
    expect(document.body.textContent).not.toContain("Checking this machine…");
    // The usage it couldn't read: unavailable, not placeholders.
    expect(screen.getAllByText("unavailable").length).toBe(3);
    await user.click(screen.getByRole("button", { name: /Try again/ }));
    await flush();
    expect(retry).toHaveBeenCalled();
  });

  it("says so in the machine-setup window, with Try again", async () => {
    installTauri();
    const user = userEvent.setup();
    let fail = true;
    const retry = vi.fn(() => (fail ? Promise.reject(new Error("still failing")) : Promise.resolve()));
    render(<MachineSetup report={null} checkError="brew: command not found" onRetryCheck={retry} onRefresh={vi.fn()} onClose={vi.fn()} />);
    expect(document.body.textContent).toContain("Couldn’t check this machine");
    expect(document.body.textContent).toContain("brew: command not found");
    expect(document.body.textContent).not.toContain("Checking this machine…");
    await user.click(screen.getByRole("button", { name: /Try again/ }));
    await flush();
    fail = false;
    await user.click(screen.getByRole("button", { name: /Try again/ }));
    await flush();
    expect(retry).toHaveBeenCalledTimes(2);
  });
});
