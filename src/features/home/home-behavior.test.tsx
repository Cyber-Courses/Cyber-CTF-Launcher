// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { assessRam } from "@/features/home/capacity";
import { DeployPanel } from "@/features/home/deploy-panel";
import { HomeScreen } from "@/features/home/home-screen";
import { JumpBackPanel } from "@/features/home/jump-back-panel";
import { MachineSummaryPanel } from "@/features/home/machine-summary-panel";
import type { Lab } from "@/features/labs/use-labs";
import { must } from "@/components/test-support";
import { Toaster } from "@/components/ui/toaster";
import type { AuthStatus } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

const GB = 1e9;
const signedIn: AuthStatus = { loggedIn: true, name: "Florian Amette", email: "florian@cyberctf.org" };
const signedOut: AuthStatus = { loggedIn: false, name: null, email: null };
const lab = (id: string, slug: string, title: string, over: Partial<Lab> = {}) =>
  ({
    id,
    slug,
    title,
    description: "",
    question: null,
    difficulty: 1,
    category: "Web",
    runtime: { runtime: "DOCKER", architectures: [], providers: ["docker"], hosted: false },
    skills: [],
    ...over,
  }) as unknown as Lab;

afterEach(() => vi.restoreAllMocks());

/** The Overview, signed in on a ready machine, once its first reads answered. */
async function renderHome() {
  const onNavigate = vi.fn();
  const user = userEvent.setup();
  render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={onNavigate} />);
  await flush(80);
  return { onNavigate, user };
}

describe("HomeScreen", () => {
  it("greets the player by first name and lists what runs", async () => {
    installTauri({ active_operations: () => [] });
    render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={vi.fn()} />);
    await flush(50);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toContain("Florian");
    expect(screen.getByRole("heading", { level: 1 }).textContent).not.toContain("Amette");
    expect(document.body.textContent).toMatch(/1 lab running/);
    // Its 7 containers (the dev mock's wide lab), from the lab's own status.
    expect(document.body.textContent).toContain("7 containers");
    // Nothing deploying: the machine summary instead.
    expect(document.body.textContent).toMatch(/Docker engine/i);
    expect(document.body.textContent).not.toMatch(/sign in/i);
  });

  it("greets a signed-in player without a name plainly", async () => {
    installTauri();
    render(<HomeScreen report={readyReport()} auth={{ ...signedIn, name: null }} onNavigate={vi.fn()} />);
    await flush(50);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toMatch(/^Good (morning|afternoon|evening)/);
  });

  it("offers to set up Docker when it isn't ready, and says when that fails", async () => {
    const calls = installTauri({
      running_labs: () => [],
      machine_open_setup: () => {
        throw new Error("setup window busy");
      },
    });
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const user = userEvent.setup();
    render(
      <>
        <HomeScreen report={freshReport("windows")} auth={signedOut} onNavigate={vi.fn()} />
        <Toaster />
      </>,
    );
    await flush(50);
    expect(document.body.textContent).toMatch(/Set up Docker/);
    await user.click(screen.getByRole("button", { name: /set up this machine/i }));
    await flush();
    expect(commands(calls)).toContain("machine_open_setup");
    expect(document.body.textContent).toContain("setup window busy");
  });

  it("browses the labs once Docker is ready", async () => {
    installTauri();
    const onNavigate = vi.fn();
    const user = userEvent.setup();
    render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={onNavigate} />);
    await flush(50);
    await user.click(screen.getByRole("button", { name: /browse/i }));
    expect(onNavigate).toHaveBeenCalledWith("labs");
  });

  it("says the check is still running before the report", async () => {
    installTauri();
    render(<HomeScreen report={null} auth={null} onNavigate={vi.fn()} />);
    await flush(50);
    expect(document.body.textContent).toMatch(/checking this machine/i);
  });

  it("warns a machine short on memory, pointing at a server or the cloud", async () => {
    installTauri({
      machine_metrics: () => ({ cpu: 10, memUsed: 3 * GB, memTotal: 6 * GB, diskUsed: 0, diskTotal: 0, uptimeSecs: 1, cores: 2, containers: 0 }),
    });
    const { onNavigate, user } = await renderHome();
    expect(document.body.textContent).toContain("6.0");
    await user.click(screen.getByRole("button", { name: /cloud/i }));
    await user.click(screen.getByRole("button", { name: /server/i }));
    expect(onNavigate.mock.calls).toEqual([["cloud"], ["server"]]);
  });

  it("says when there are no labs, or nothing running", async () => {
    installTauri({
      api_query: () => ({ labs: [] }),
      running_labs: () => [],
      lab_status: () => ({ running: false, parked: null, url: null, host: null, expiresAt: null, place: null, provider: null, networks: [], machines: [] }),
    });
    render(<HomeScreen report={readyReport()} auth={signedIn} onNavigate={vi.fn()} />);
    await flush(80);
    expect(document.body.textContent).toMatch(/no labs|nothing/i);
  });

  it("resumes a parked lab from Jump back in", async () => {
    localStorage.setItem("cyberctf.lastrun.lab-kube", String(Date.now() - 3_600_000));
    const parked = {
      running: false,
      parked: "pause",
      url: null,
      host: null,
      expiresAt: null,
      place: "container",
      provider: "docker",
      networks: [],
      machines: [],
      attacker: null,
      outputs: [],
      message: null,
    };
    const calls = installTauri({
      active_operations: () => [],
      lab_status: (a) => (a?.id === "lab-kube" ? parked : { ...parked, parked: null, place: null }),
      running_labs: () => [],
    });
    const { onNavigate, user } = await renderHome();
    const panel = must(must(screen.getByText(/jump back in/i).closest("section, div"), "the Jump back in panel").parentElement, "its parent");
    await user.click(within(panel).getAllByText("Exposed dashboard")[0]);
    expect(onNavigate).toHaveBeenCalledWith("labs", "kube-dashboard-exposed");
    await user.click(within(panel).getByRole("button", { name: /resume/i }));
    await flush(50);
    expect(commands(calls)).toContain("lab_resume");
    expect(commands(calls)).not.toContain("lab_launch");
  });
});

describe("DeployPanel", () => {
  const labs = [lab("lab-goad", "goad-light", "GOAD Light"), lab("lab-sqli", "blind", "Blind orders")];

  it("names the one lab deploying, its step and machine, with a link to it", async () => {
    const onOpen = vi.fn();
    const user = userEvent.setup();
    render(<DeployPanel ops={[{ labId: "lab-goad", op: "provision", machine: "isoloom-SRV02", step: "TASK [users]" }]} labs={labs} onOpen={onOpen} />);
    // Its title, not its slug or id.
    expect(screen.getByText("GOAD Light")).toBeTruthy();
    expect(screen.queryByText("goad-light")).toBeNull();
    expect(screen.getByText("provisioning srv02")).toBeTruthy();
    expect(screen.getByText("TASK [users]")).toBeTruthy();
    expect(screen.getByText("SRV02")).toBeTruthy();
    const views = screen.getAllByRole("button", { name: /view/i });
    expect(views).toHaveLength(1);
    await user.click(views[0]);
    expect(onOpen).toHaveBeenCalledWith("goad-light");
  });

  it("lists several deploys, each with its own link; an unknown lab by its id made readable", async () => {
    const onOpen = vi.fn();
    const user = userEvent.setup();
    render(
      <DeployPanel
        ops={[
          { labId: "lab-goad", op: "launch", machine: null, step: null },
          { labId: "lab-sqli", op: "stop", machine: null, step: "Removing containers" },
          { labId: "lab-gone", op: "pause", machine: null, step: null },
        ]}
        labs={labs}
        onOpen={onOpen}
      />,
    );
    expect(screen.getByText("3 labs")).toBeTruthy();
    expect(screen.getByText("GOAD Light")).toBeTruthy();
    expect(screen.getByText("Gone")).toBeTruthy();
    expect(screen.getByText("Removing containers")).toBeTruthy();
    const views = screen.getAllByRole("button", { name: /view/i });
    expect(views).toHaveLength(2);
    await user.click(views[1]);
    expect(onOpen).toHaveBeenCalledWith("blind");
  });

  it("names a lone deploy of an unknown lab by its id made readable, without a link", () => {
    render(<DeployPanel ops={[{ labId: "lab-goad-light", op: "launch", machine: null, step: null }]} labs={labs} onOpen={vi.fn()} />);
    expect(screen.getByText("Goad Light")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /view/i })).toBeNull();
  });
});

describe("JumpBackPanel", () => {
  it("shows how long ago, and disables Resume while busy or not allowed", async () => {
    const onStart = vi.fn();
    const a = lab("a", "alpha", "Alpha");
    const b = lab("b", "beta", "Beta");
    const user = userEvent.setup();
    const now = 10_000_000;
    render(
      <JumpBackPanel
        recent={[
          { lab: a, ts: now - 2 * 3_600_000 },
          { lab: b, ts: now - 60_000 },
        ]}
        now={now}
        canStart={(l) => l.id === "a"}
        isBusy={(l) => l.id === "b"}
        onOpen={vi.fn()}
        onStart={onStart}
      />,
    );
    expect(document.body.textContent).toMatch(/alpha.*2 h/);
    const [resumeA, busyB] = screen.getAllByRole("button").filter((x) => !/Alpha|Beta/.test(x.textContent));
    expect(busyB.hasAttribute("disabled")).toBe(true);
    await user.click(resumeA);
    expect(onStart).toHaveBeenCalledWith(a);
  });
});

describe("MachineSummaryPanel", () => {
  it("shows the engine state, containers and cores, or placeholders", async () => {
    const onOpen = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<MachineSummaryPanel report={null} metrics={null} dockerReady={null} onOpen={onOpen} />);
    expect(document.body.textContent).toMatch(/none/i);
    rerender(<MachineSummaryPanel report={freshReport()} metrics={null} dockerReady={true} onOpen={onOpen} />);
    expect(document.body.textContent).toMatch(/stopped/i);
    expect(document.body.textContent).toContain("0");
    await user.click(screen.getByRole("button"));
    expect(onOpen).toHaveBeenCalled();
  });
});

describe("assessRam", () => {
  it.each([
    [32, "ok"],
    [16, "ok"],
    [12, "tight"],
    [4, "low"],
  ])("%d GB is %s", (gb, level) => {
    const c = assessRam(gb * GB);
    expect(c.level).toBe(level);
    expect(c.totalGB).toBe(gb);
    expect(c.title).not.toContain("home.capacity");
    expect(c.detail).not.toContain("home.capacity");
  });
});
