// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Labs } from "@/features/labs/labs-screen";
import { answer } from "@/lib/dev-mock";
import { PortModePrompt } from "@/features/labs/port-mode-prompt";
import { setPortMode } from "@/lib/settings";
import type { LabMachine, LabStatus } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { commands, installTauri, stream, type Handler } from "@/test/tauri";
import { flush } from "@/test/ui";

// The Labs screen as a player drives it: the real useLabs / useLabActions / deploy store behind
// the list and the lab page, with only the Rust commands mocked. The component tests next to it
// drive LabDetail with spied handlers; these check what the commands' answers do on screen.

const machine = (name: string, state = "running", p: Partial<LabMachine> = {}): LabMachine => ({
  name,
  state,
  image: "node:20",
  ip: "10.42.0.3",
  ports: [],
  interfaces: [{ network: "default", ip: "10.42.0.3" }],
  services: [],
  infra: false,
  ...p,
});
const status = (p: Partial<LabStatus> = {}): LabStatus => ({
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
  ...p,
});

// Nothing deploying (the dev mock reports GOAD mid-deploy), and no server: Start on a container
// lab with no hosted run then starts here at once, without the "Where should it run?" dialog.
const settled: Record<string, Handler> = {
  // The dev mock opens a browser tab for it; jsdom has none.
  server_open_setup: () => null,
  deploying_labs: () => [],
  active_operations: () => [],
  server_list: () => ({ default: null, hosts: [] }),
};

function renderLabs(slug: string | null, overrides: Record<string, Handler> = {}) {
  const calls = installTauri({ ...settled, ...overrides });
  const onDetailChange = vi.fn();
  const view = render(
    <>
      <Labs
        loggedIn
        authReady
        onLogin={vi.fn(async () => {})}
        hostArch="x86_64"
        report={{ ...readyReport(), arch: "x86_64" }}
        openLab={{ slug, tick: 0 }}
        onDetailChange={onDetailChange}
      />
      {/* Mounted once in the app shell: the ports question before a container lab starts here. */}
      <PortModePrompt />
    </>,
  );
  return { calls, onDetailChange, ...view };
}

const titles = () =>
  screen
    .queryAllByRole("button")
    .map((b) => b.getAttribute("aria-label") ?? "")
    .filter((l) => l.startsWith("Open "))
    .map((l) => l.slice(5));

afterEach(() => setPortMode(null));

// The deploy store adds streamed log lines in batches (every FLUSH_MS, 100 ms): wait past one.
const LOG_BATCH = 150;

/** The lab page's Deployment panel (its steps, state pill and log). */
const deployment = () => screen.getByRole("heading", { name: /^Deployment/ }).closest<HTMLElement>(".surface-panel")!;

describe("Labs catalogue, from the keyboard", () => {
  it("picks a filter with Tab and the arrow keys, and the list follows", async () => {
    renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    const runtime = screen.getByRole("radiogroup", { name: "Runtime" });
    const any = within(runtime).getByRole("radio", { name: "Any runtime" });
    const vm = within(runtime).getByRole("radio", { name: "VM" });
    // A filter is one Tab stop, its checked option; the arrows move the choice inside it.
    screen.getByRole("textbox", { name: "Search labs and skills" }).focus();
    for (let n = 0; n < 12 && document.activeElement !== any; n++) await user.tab();
    expect(document.activeElement).toBe(any);
    await user.keyboard("{ArrowRight}{ArrowRight}");
    expect(document.activeElement).toBe(vm);
    expect(vm.getAttribute("aria-checked")).toBe("true");
    expect(titles()).toEqual(["GOAD Light"]);
    await user.keyboard("{ArrowLeft}");
    expect(within(runtime).getByRole("radio", { name: "Container" }).getAttribute("aria-checked")).toBe("true");
    expect(titles()).not.toContain("GOAD Light");
  });

  it("searches as the player types, ignoring case and accents, and clears back to everything", async () => {
    renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    const search = screen.getByRole("textbox", { name: "Search labs and skills" });
    await user.type(search, "GOAD");
    expect(titles()).toEqual(["GOAD Light"]);
    await user.clear(search);
    expect(titles()).toHaveLength(6);
  });

  it("opens a focused row with Enter, and Escape on the lab page comes back to the list with the filters kept", async () => {
    const { onDetailChange } = renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    await user.click(within(screen.getByRole("radiogroup", { name: "Level" })).getByRole("radio", { name: "Hard" }));
    screen.getByRole("button", { name: "Open GOAD Light" }).focus();
    await user.keyboard("{Enter}");
    await flush();
    expect(screen.getByRole("heading", { name: "GOAD Light" })).toBeTruthy();
    expect(onDetailChange).toHaveBeenLastCalledWith("GOAD Light");
    (document.activeElement as HTMLElement | null)?.blur();
    await user.keyboard("{Escape}");
    expect(onDetailChange).toHaveBeenLastCalledWith(null);
    // Back on the list, still filtered to Hard.
    expect(
      within(screen.getByRole("radiogroup", { name: "Level" }))
        .getByRole("radio", { name: "Hard" })
        .getAttribute("aria-checked"),
    ).toBe("true");
    expect(titles()).toEqual(["GOAD Light"]);
  });

  it("keeps Escape typed in the search box from doing anything on the list", async () => {
    const { onDetailChange } = renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    await user.type(screen.getByRole("textbox", { name: "Search labs and skills" }), "goad{Escape}");
    expect(titles()).toEqual(["GOAD Light"]);
    expect(onDetailChange).not.toHaveBeenCalledWith("GOAD Light");
  });

  it("arrow keys don't move between a filter's options", async () => {
    // The filters are WAI-ARIA radiogroups (role=radiogroup / role=radio), and a keyboard or
    // screen-reader user expects the arrow keys to move the choice inside one, with the group a
    // single Tab stop. Segmented (src/components/ui/segmented.tsx:24) renders plain buttons:
    // every option is its own Tab stop and ArrowRight does nothing.
    // Suggested fix: roving tabindex in Segmented (tabIndex 0 on the checked option, -1 on the
    // others) and an onKeyDown on the group that moves and selects with ArrowLeft/Right/Up/Down,
    // Home and End.
    renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    const level = screen.getByRole("radiogroup", { name: "Level" });
    within(level).getByRole("radio", { name: "Any level" }).focus();
    await user.keyboard("{ArrowRight}");
    expect(within(level).getByRole("radio", { name: "Easy" }).getAttribute("aria-checked")).toBe("true");
  });

  it("a lab row is a role=button with its action buttons inside it", async () => {
    // ARIA gives role=button presentational children: a screen reader reads the row as one
    // button ("Open Invoice portal API") and its Open / Stop / Resume / Set up / Connect buttons
    // inside are not exposed as controls (axe: nested-interactive). LabRow
    // (src/features/labs/lab-row.tsx:118) puts them inside the role=button div.
    // Suggested fix: make the row a plain container whose title is the button (or a
    // stretched-link button covering the row), with the row actions as its siblings.
    renderLabs(null);
    await flush(80);
    const nested = screen.queryAllByRole("button").filter((b) => b.getAttribute("role") === "button" && b.querySelector("button"));
    expect(nested.map((b) => b.getAttribute("aria-label"))).toEqual([]);
  });

  it("asks before stopping a running lab from its row, Escape keeps it running", async () => {
    const { calls } = renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("alertdialog");
    // The dialog has the focus (jsdom has no layout, so the trap can't pick its first button).
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(dialog.isConnected).toBe(false);
    expect(commands(calls)).not.toContain("lab_stop");
    // The focus is back on the row's Stop button, not lost to the body.
    expect(document.activeElement?.textContent).toBe("Stop");
  });

  it("offers to connect a cloud account for a cloud lab when none is saved", async () => {
    const { calls } = renderLabs(null);
    const user = userEvent.setup();
    await flush(80);
    // The row's actions sit beside its open button (one row, two controls).
    const row = screen.getByRole("button", { name: "Open EC2 SSRF" }).closest<HTMLElement>(".group")!;
    await user.click(within(row).getByRole("button", { name: /Connect/ }));
    expect(calls.find((c) => c.cmd === "server_open_setup")?.args).toMatchObject({ kind: "cloud" });
    // The click was the row button's: the lab page didn't open behind it.
    expect(screen.queryByRole("heading", { name: "EC2 SSRF" })).toBeNull();
  });
});

// "Exposed dashboard": a container lab with no hosted run and no VM option, so with no server
// saved its Start starts here at once.
const KUBE = "kube-dashboard-exposed";
const kubeUp = () => status({ running: true, machines: [machine("dashboard")], url: "http://127.0.0.1:8443", place: "container", provider: "docker" });

/** A lab_launch that streams `lines`, then waits for `release()`; `more()` streams further lines. */
function heldLaunch(...lines: string[]) {
  let args: Record<string, unknown> | undefined;
  let release!: (v?: unknown) => void;
  let reject!: (e: unknown) => void;
  const handler: Handler = (a) => {
    args = a;
    stream(a, ...lines);
    return new Promise((res, rej) => {
      release = res;
      reject = rej;
    });
  };
  return { handler, more: (...l: string[]) => stream(args, ...l), release: () => release(null), reject: (e: unknown) => reject(e) };
}

describe("Lab page, driven through the Labs screen", () => {
  it("follows a container start step by step from the streamed lines, then reads Ready", async () => {
    setPortMode("random");
    const launch = heldLaunch("Downloading CyberCTF/kube-dashboard-exposed@abc123", " Network cyberctf-kube_default Creating");
    let up = false;
    renderLabs(KUBE, { lab_launch: launch.handler, lab_status: (a) => (a?.id === "lab-kube" && up ? kubeUp() : status()) });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    // Past the deploy store's batching of log lines (FLUSH_MS, 100 ms).
    await flush(LOG_BATCH);
    // Started at once (no servers, ports remembered): no dialog, and the header says so.
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { name: /Starting/ }).hasAttribute("disabled")).toBe(true);
    expect(within(deployment()).getByText("Starting")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Download the lab/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Create the network and containers/ })).toBeTruthy();
    launch.more(" Container kube-dashboard-1 Starting", " Container kube-dashboard-1 Started");
    await flush(LOG_BATCH);
    expect(screen.getByRole("button", { name: /Start the containers/ })).toBeTruthy();
    // The running step shows its newest line inline.
    expect(screen.getByText("Container kube-dashboard-1 Started")).toBeTruthy();
    up = true;
    launch.release();
    await flush(80);
    expect(within(deployment()).getAllByText("Ready").length).toBeGreaterThan(0);
    expect(within(deployment()).queryByText("Starting")).toBeNull();
    expect(screen.getByRole("button", { name: /Stop & remove/ })).toBeTruthy();
  });

  it("launches once when Start is double-clicked", async () => {
    setPortMode("random");
    const launch = heldLaunch("Downloading CyberCTF/kube-dashboard-exposed@abc123");
    const { calls } = renderLabs(KUBE, { lab_launch: launch.handler, lab_status: () => status() });
    const user = userEvent.setup();
    await flush(80);
    await user.dblClick(screen.getByRole("button", { name: /Start lab/ }));
    await flush();
    expect(commands(calls).filter((c) => c === "lab_launch")).toHaveLength(1);
    launch.release();
    await flush(80);
  });

  it("asks for the ports first and launches once when Start is double-clicked before answering", async () => {
    // No port mode remembered: the first click asks; the second asks again and cancels the first.
    const launch = heldLaunch("Downloading CyberCTF/kube-dashboard-exposed@abc123");
    const { calls } = renderLabs(KUBE, { lab_launch: launch.handler, lab_status: () => status() });
    const user = userEvent.setup();
    await flush(80);
    await user.dblClick(screen.getByRole("button", { name: /Start lab/ }));
    await flush();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Start lab" }));
    await flush();
    expect(commands(calls).filter((c) => c === "lab_launch")).toHaveLength(1);
    expect(calls.find((c) => c.cmd === "lab_launch")?.args).toMatchObject({ defaultPorts: false });
    launch.release();
    await flush(80);
  });

  it("shows a start that fails with its reason, marks the step failed, and offers Start again", async () => {
    setPortMode("random");
    const launch = heldLaunch(
      "Downloading CyberCTF/kube-dashboard-exposed@abc123",
      " Container kube-dashboard-1 Creating",
      " Container kube-dashboard-1 Starting",
    );
    renderLabs(KUBE, { lab_launch: launch.handler, lab_status: () => status() });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    await flush();
    launch.reject("Bind for 0.0.0.0:8443 failed: port is already allocated");
    await flush(80);
    expect(within(deployment()).getByText("Failed")).toBeTruthy();
    // The reason, in the deployment panel and in the failure notice under the header.
    expect(within(deployment()).getByText("Bind for 0.0.0.0:8443 failed: port is already allocated")).toBeTruthy();
    expect(screen.getByText(/^Couldn.t start Exposed dashboard/)).toBeTruthy();
    expect(screen.getAllByText("Bind for 0.0.0.0:8443 failed: port is already allocated")).toHaveLength(2);
    expect(within(deployment()).getByText("Start the containers").className).toContain("text-destructive");
    // Nothing runs: Start is back as Retry, enabled, with the same start on random ports beside it
    // (the reason is a port taken here).
    expect(screen.getByRole("button", { name: /^\s*Retry$/ }).hasAttribute("disabled")).toBe(false);
    expect(screen.getByRole("button", { name: /Retry with random ports/ }).hasAttribute("disabled")).toBe(false);
  });

  it("shows a stop that fails in the deployment log, and keeps the lab's actions", async () => {
    const { calls } = renderLabs("invoice-portal-api", {
      lab_stop: (a) => {
        stream(a, "==> api: Stopping");
        throw "docker: permission denied while trying to connect to the Docker daemon socket";
      },
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Stop & remove/ }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Remove" }));
    await flush(80);
    expect(calls.find((c) => c.cmd === "lab_stop")?.args).toMatchObject({ id: "lab-invoice", runtime: "DOCKER" });
    expect(within(deployment()).getByText("Failed")).toBeTruthy();
    // The error, in the deployment log and in the failure notice under the header.
    expect(within(deployment()).getByText("docker: permission denied while trying to connect to the Docker daemon socket")).toBeTruthy();
    expect(screen.getAllByText("docker: permission denied while trying to connect to the Docker daemon socket")).toHaveLength(2);
    // Still running per its status: the player can try again.
    expect(screen.getByRole("button", { name: /Stop & remove/ }).hasAttribute("disabled")).toBe(false);
  });

  it("pauses a VM lab running here, saying Pausing… on that button only, then shows it paused with Resume", async () => {
    let parked = false;
    let release!: () => void;
    const vmUp = status({ running: true, machines: [machine("DC01", "running", { image: "" })], place: "local_vm", provider: "virtualbox" });
    const { calls } = renderLabs("goad-light", {
      lab_status: (a) =>
        a?.id === "lab-goad" ? (parked ? status({ parked: "pause", machines: [machine("DC01", "saved", { image: "" })], place: "local_vm" }) : vmUp) : status(),
      lab_park: (a) => {
        stream(a, "==> DC01: Saving state");
        return new Promise<null>((r) => (release = () => r(null)));
      },
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /^\s*Pause$/ }));
    await flush(LOG_BATCH);
    expect(calls.find((c) => c.cmd === "lab_park")?.args).toMatchObject({ id: "lab-goad", runtime: "VM", mode: "pause" });
    expect(screen.getByRole("button", { name: /Pausing…/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Stop & remove/ }).hasAttribute("disabled")).toBe(true);
    expect(within(deployment()).getByText("Pausing")).toBeTruthy();
    expect(within(deployment()).getByText("Save the machines' state")).toBeTruthy();
    parked = true;
    release();
    await flush(80);
    expect(screen.getByRole("button", { name: /Resume/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Pausing…/ })).toBeNull();
  });

  it("resumes a parked VM lab from the header", async () => {
    let parked = true;
    const { calls } = renderLabs("goad-light", {
      lab_status: (a) =>
        a?.id !== "lab-goad"
          ? status()
          : parked
            ? status({ parked: "shutdown", machines: [machine("DC01", "poweroff", { image: "" })], place: "local_vm" })
            : status({ running: true, machines: [machine("DC01", "running", { image: "" })], place: "local_vm", provider: "virtualbox" }),
      lab_resume: (a) => {
        stream(a, "==> DC01: Resuming");
        parked = false;
        return null;
      },
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Resume/ }));
    await flush(80);
    expect(calls.find((c) => c.cmd === "lab_resume")?.args).toMatchObject({ id: "lab-goad", runtime: "VM" });
    expect(screen.getByRole("button", { name: /^\s*Pause$/ })).toBeTruthy();
  });

  it("re-runs a VM lab's setup on the machine picked, streamed into the deployment log", async () => {
    const { calls } = renderLabs("goad-light", {
      lab_status: (a) =>
        a?.id === "lab-goad"
          ? status({
              running: true,
              machines: [machine("DC01", "running", { image: "" }), machine("SRV02", "running", { image: "" })],
              place: "local_vm",
              provider: "virtualbox",
            })
          : status(),
    });
    const user = userEvent.setup();
    await flush(80);
    await user.selectOptions(screen.getByRole("combobox"), "SRV02");
    await user.click(screen.getByRole("button", { name: /Re-run setup/ }));
    await flush(80);
    expect(calls.find((c) => c.cmd === "lab_provision")?.args).toMatchObject({ id: "lab-goad", runtime: "VM", machine: "SRV02" });
    await user.click(screen.getByRole("button", { name: /Show full log/ }));
    expect(document.body.textContent).toContain("TASK [Configure DC01]");
    expect(document.body.textContent).toContain("✓ SRV02 provisioned");
  });

  it("cleans up an interrupted start from the header", async () => {
    let left = true;
    const { calls } = renderLabs(KUBE, {
      lab_status: (a) => (a?.id === "lab-kube" && left ? status({ machines: [machine("dashboard", "exited")] }) : status()),
      lab_stop: (a) => {
        stream(a, " Container kube-dashboard-1 Removed");
        left = false;
        return null;
      },
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Stop & clean up/ }));
    await flush(80);
    expect(calls.find((c) => c.cmd === "lab_stop")?.args).toMatchObject({ id: "lab-kube", runtime: "DOCKER" });
    expect(screen.getByRole("button", { name: /Start lab/ })).toBeTruthy();
  });

  it("cleaning up an interrupted start reads as a stop at once, not Starting…", async () => {
    // "Stop & clean up" runs lab_stop on a lab that isn't running or parked. Before the next
    // active_operations poll (every 2 s), the page must name the stop from its own action (the
    // deploy store's run op), not guess a launch from the status (busy, neither running nor
    // parked), which read "Starting… 0:01" for a clean-up.
    let release!: () => void;
    renderLabs(KUBE, {
      lab_status: (a) => (a?.id === "lab-kube" ? status({ machines: [machine("dashboard", "exited")] }) : status()),
      lab_stop: () => new Promise<null>((r) => (release = () => r(null))),
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Stop & clean up/ }));
    await flush();
    expect(screen.queryByRole("button", { name: /Starting/ })).toBeNull();
    expect(within(deployment()).queryByText("Starting")).toBeNull();
    expect(within(deployment()).getByText("Stopping")).toBeTruthy();
    release();
    await flush(80);
  });

  it("closes the Where-should-it-run dialog with Escape and stays on the lab page", async () => {
    const { calls, onDetailChange } = renderLabs("goad-light", {
      server_list: () => ({ default: null, hosts: [{ id: "esxi", name: "office-esxi", provider: "vmware_esxi", host: "10.0.4.12" }] }),
      lab_status: () => status(),
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
    // One Escape, one action: the dialog closed, the page stayed.
    expect(screen.getByRole("heading", { name: "GOAD Light" })).toBeTruthy();
    expect(onDetailChange).not.toHaveBeenLastCalledWith(null);
    expect(commands(calls)).not.toContain("lab_launch");
  });

  it("cancels Stop & remove with Escape and stays on the lab page", async () => {
    const { calls, onDetailChange } = renderLabs("invoice-portal-api");
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Stop & remove/ }));
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.getByRole("heading", { name: "Invoice portal API" })).toBeTruthy();
    expect(onDetailChange).not.toHaveBeenLastCalledWith(null);
    expect(commands(calls)).not.toContain("lab_stop");
  });

  it("cancels the ports question with Escape: nothing starts and the page stays", async () => {
    const { calls } = renderLabs(KUBE, { lab_status: () => status() });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    await flush();
    expect(screen.getByRole("dialog")).toBeTruthy();
    await user.keyboard("{Escape}");
    await flush();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("heading", { name: "Exposed dashboard" })).toBeTruthy();
    expect(commands(calls)).not.toContain("lab_launch");
    expect(screen.getByRole("button", { name: /Start lab/ }).hasAttribute("disabled")).toBe(false);
  });

  it("starts a VM lab on the server picked in the dialog", async () => {
    const { calls } = renderLabs("goad-light", {
      server_list: () => ({ default: null, hosts: [{ id: "esxi", name: "office-esxi", provider: "vmware_esxi", host: "10.0.4.12" }] }),
      server_test: () => ({ ok: true, reachable: true, authenticated: true, latencyMs: 4, message: null, checks: [] }),
      lab_status: () => status(),
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    const dialog = screen.getByRole("dialog");
    await flush();
    await user.click(within(dialog).getByText("office-esxi"));
    await user.click(within(dialog).getByRole("button", { name: /^Start$/ }));
    await flush(80);
    expect(calls.find((c) => c.cmd === "lab_launch")?.args).toMatchObject({ labId: "lab-goad", host: "esxi" });
  });
});

describe("Lab page, a hosted run", () => {
  it("Start stays enabled while a hosted launch request is on its way, so it can be sent twice", async () => {
    // StartLabButton (lab-detail/start-lab-button.tsx:52) is disabled by `hostedLive`, which is
    // only true once the backend answered with a session. While `requestLabLaunch` is in flight
    // (useHostedRun's `starting`), Start is enabled: a second click opens the dialog again and
    // sends a second launchHosted (or starts the lab here beside the hosted one on its way).
    // Suggested fix: pass `hostedLive={hosted.live || hosted.starting}` from LabDetail (index.tsx)
    // or a separate `hostedStarting` prop that disables Start with a "Starting hosted…" title.
    const { calls } = renderLabs("sqli-blind-orders", {
      lab_status: () => status(),
      api_query: (a) => {
        const op = String(a?.operation);
        if (op.includes("launchHosted")) return new Promise(() => {});
        return answer("api_query", a);
      },
    });
    const user = userEvent.setup();
    await flush(80);
    await user.click(screen.getByRole("button", { name: /Start lab/ }));
    await user.click(within(screen.getByRole("dialog")).getByText("Hosted by Cyber CTF"));
    await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: /^Start$/ }));
    await flush();
    expect(calls.filter((c) => String(c.args?.operation).includes("launchHosted"))).toHaveLength(1);
    expect(screen.getByRole("button", { name: /Start lab/ }).hasAttribute("disabled")).toBe(true);
  });
});
