// @vitest-environment jsdom
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Toaster } from "@/components/ui/toaster";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { MachineScreen } from "@/features/machine/machine-screen";
import { MachineSetup } from "@/features/machine/machine-setup";
import { SelfTest } from "@/features/machine/self-test";
import { MachineStepBody, useMachineSetup, type MachineStep } from "@/features/machine/setup-steps";
import { setLocalePreference } from "@/lib/i18n";
import { getAttackImage } from "@/lib/settings";
import type { SelfTestEvent, SystemReport } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { commands, installTauri, stream } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const text = () => document.body.textContent ?? "";
const disabled = (el: HTMLElement) => el.hasAttribute("disabled");
const count = (calls: { cmd: string }[], cmd: string) => commands(calls as never).filter((c) => c === cmd).length;
const radioWith = (s: string) => screen.getAllByRole("radio").find((r) => r.textContent?.includes(s))!;
const ev = (step: string, state: SelfTestEvent["state"], detail?: string): SelfTestEvent =>
  ({ step, label: step, state, detail: detail ?? null }) as SelfTestEvent;

/** One step body with a live setup state, the way both setup flows mount it. */
function Step({ step, report, onRefresh = () => {} }: { step: MachineStep; report: SystemReport; onRefresh?: () => void }) {
  const setup = useMachineSetup(report, onRefresh);
  return <MachineStepBody step={step} report={report} setup={setup} />;
}

/** A Mac whose Docker CLI is installed but no engine runs. */
const stoppedMac = (): SystemReport => ({ ...readyReport(), os: "macos", dockerRunning: false, dockerEngine: null, dockerEnginesRunning: [] });

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  setLocalePreference("system");
});

describe("engine step: switching and starting", () => {
  it("says a failed engine switch in a toast and re-checks the machine", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    installTauri({
      docker_use_engine: () => {
        throw "context orbstack not found";
      },
    });
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    const report = {
      ...readyReport(),
      os: "macos",
      dockerEngine: "docker-desktop" as const,
      dockerEnginesRunning: ["docker-desktop", "orbstack"] as SystemReport["dockerEnginesRunning"],
    };
    render(
      <>
        <Step step="docker" report={report} onRefresh={onRefresh} />
        <Toaster />
      </>,
    );
    await user.click(radioWith("OrbStack"));
    await user.click(button(/use orbstack/i));
    await flush();
    expect(text()).toContain("Couldn't switch the Docker engine");
    expect(text()).toContain("context orbstack not found");
    expect(onRefresh).toHaveBeenCalledOnce();
    // The button is back for another try.
    expect(disabled(button(/use orbstack/i))).toBe(false);
  });

  it("starts the engine once however often Start is pressed while it starts", async () => {
    const calls = installTauri({ docker_start_engine: () => new Promise(() => {}) });
    const user = userEvent.setup();
    render(<Step step="docker" report={stoppedMac()} />);
    const start = button(/start docker desktop/i);
    await user.dblClick(start);
    await flush();
    expect(count(calls, "docker_start_engine")).toBe(1);
    expect(disabled(button(/starting…/i))).toBe(true);
    expect(text()).toContain("the first start can take a minute");
  });

  it("starts the engine the player picked, not the recommended one", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    render(<Step step="docker" report={stoppedMac()} />);
    await user.click(radioWith("Colima"));
    await user.click(button(/start colima/i));
    await flush();
    expect(calls.find((c) => c.cmd === "docker_start_engine")?.args).toMatchObject({ engine: "colima" });
  });

  // Bug: the start error is one state for the whole step. After "Start Docker Desktop" fails,
  // picking Colima still shows Docker Desktop's error, with a "Get Colima" button, as if Colima
  // had failed to start. Suggested fix (engine-step.tsx): keep the error per engine
  // (`useState<{ engine: DockerEngine; message: string } | null>` and show it only when
  // `startError.engine === choice.id`), or clear it in the Choice's onSelect.
  it("forgets one engine's start error when the player picks another engine", async () => {
    installTauri({
      docker_start_engine: () => {
        throw "Docker Desktop.app is damaged";
      },
    });
    const user = userEvent.setup();
    render(<Step step="docker" report={stoppedMac()} />);
    await user.click(button(/start docker desktop/i));
    await flush();
    expect(text()).toContain("Docker Desktop.app is damaged");
    await user.click(radioWith("Colima"));
    expect(text()).not.toContain("Docker Desktop.app is damaged");
    expect(text()).toContain("Colima is installed but not running.");
  });

  // Bug: the engine cards' badges ("in use", "running", "recommended") are typed English literals
  // passed straight to the Choice card (choice-card.tsx renders `{badge}`), so they stay English
  // in every language. Suggested fix: keep the union as the prop's value but render
  // `t(\`ui.choice.badge.${badge}\`)` (new keys in each ui.json), as the rest of the step does.
  it("translates the engine badges with the rest of the step", () => {
    installTauri();
    setLocalePreference("fr");
    render(<Step step="docker" report={{ ...readyReport(), os: "macos", dockerEnginesRunning: ["docker-desktop", "orbstack"] }} />);
    expect(text()).not.toMatch(/in use|running|recommended/);
  });
});

describe("SelfTest: states and re-runs", () => {
  it("shows no run button while a test runs, so it can't be started twice", async () => {
    let finish: () => void = () => {};
    const calls = installTauri({
      machine_selftest: (a) => {
        stream(a, ev("engine", "running"));
        return new Promise<null>((r) => (finish = () => r(null)));
      },
    });
    const user = userEvent.setup();
    render(<SelfTest kind="docker" title="Container lab test" description="d" />);
    await user.click(button(/run test/i));
    await flush();
    expect(screen.queryByRole("button")).toBeNull();
    await act(async () => finish());
    await flush();
    expect(count(calls, "machine_selftest")).toBe(1);
  });

  it("clears the last failure when it is run again, and passes", async () => {
    let fail = true;
    const calls = installTauri({
      machine_selftest: (a) => {
        if (fail) {
          stream(a, ev("engine", "ok"), ev("pull", "fail", "toomanyrequests: rate limit"));
          throw "pull failed";
        }
        stream(a, ...["engine", "pull", "start", "network", "port", "cleanup"].map((s) => ev(s, "ok")));
        return null;
      },
    });
    const onResult = vi.fn();
    const user = userEvent.setup();
    render(<SelfTest kind="docker" title="Container lab test" description="d" auto onResult={onResult} />);
    await flush();
    expect(text()).toContain("toomanyrequests: rate limit");
    fail = false;
    await user.click(button(/retry/i));
    await flush();
    expect(text()).not.toContain("toomanyrequests");
    expect(text()).toContain("Passed");
    expect(onResult.mock.calls.map((c) => c[0])).toEqual(["running", "fail", "running", "ok"]);
    // The re-run button after a pass is an icon: it must still have a name.
    expect(screen.getByRole("button", { name: "Run again" })).toBeTruthy();
    expect(count(calls, "machine_selftest")).toBe(2);
  });

  it("lists every planned step as pending as soon as it starts", async () => {
    installTauri({ machine_selftest: () => new Promise(() => {}) });
    render(<SelfTest kind="vm" title="VM lab test" description="d" auto />);
    await flush();
    // The VM test's eight steps show up front, so the list doesn't jump as it runs.
    expect(screen.getAllByRole("listitem")).toHaveLength(8);
    expect(text()).toContain("Boot the test VM");
  });

  // Bug: on the Machine page, "Hide test" stays available while the self-test runs. Hiding it
  // unmounts SelfTest (the Rust run goes on), and "Test" mounts a fresh one with `auto`, which
  // starts a second machine_selftest beside the first: two test labs on the same engine.
  // Suggested fix (lab-type-rows.tsx TestButton / machine-parts.tsx LabTypeRow): disable "Hide
  // test" while the row's test runs (LabTypeRow knows it from onResult), or keep the SelfTest
  // mounted and only hide it.
  it("does not start a second self-test when the test is hidden and shown again mid-run", async () => {
    const calls = installTauri({ machine_selftest: () => new Promise(() => {}) });
    const user = userEvent.setup();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    const engines = screen.getByText("Engines").closest("section, div")!.parentElement!;
    await user.click(within(engines).getAllByRole("button", { name: /^test$/i })[0]);
    await flush();
    await user.click(button(/hide test/i));
    await user.click(within(engines).getAllByRole("button", { name: /^test$/i })[0]);
    await flush();
    expect(count(calls, "machine_selftest")).toBe(1);
  });
});

describe("Machine page", () => {
  it("says so when the setup window can't be opened", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    installTauri({
      machine_open_setup: () => {
        throw "window label already exists";
      },
    });
    const user = userEvent.setup();
    render(
      <>
        <MachineScreen report={freshReport("linux")} onRefresh={vi.fn()} onNavigate={vi.fn()} />
        <Toaster />
      </>,
    );
    await flush(50);
    await user.click(button(/set up this machine/i));
    await flush();
    expect(text()).toContain("Couldn't open machine setup");
    expect(text()).toContain("window label already exists");
  });

  it("names the Health and Setup views as one radio group", async () => {
    installTauri();
    render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
    await flush(50);
    const group = screen.getByRole("radiogroup", { name: "Machine view" });
    expect(within(group).getByRole("radio", { name: "Health" }).getAttribute("aria-checked")).toBe("true");
  });
});

describe("Downloads panel: clean-up", () => {
  const storage = () => ({
    images: [{ name: "cyberctf/attack:latest", bytes: 3_400_000_000 }],
    boxes: [{ name: "generic/alpine318", bytes: 600_000_000 }],
  });

  it("asks before removing anything, and Cancel keeps everything", async () => {
    const calls = installTauri({ machine_storage: storage });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    await user.click(button(/clean up/i));
    expect(screen.getByRole("alert").textContent).toContain("Remove 4.0 GB?");
    // While asking, the header's Clean up is gone: one question at a time.
    expect(screen.queryAllByRole("button").some((b) => /clean up/i.test(b.textContent ?? ""))).toBe(false);
    await user.click(button(/^cancel$/i));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(count(calls, "machine_storage_clean")).toBe(0);
    expect(button(/clean up/i)).toBeTruthy();
  });

  it("removes once however often Remove is pressed, keeping the attack box image", async () => {
    let finish: (n: number) => void = () => {};
    const calls = installTauri({ machine_storage: storage, machine_storage_clean: () => new Promise((r) => (finish = r)) });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    await user.click(button(/clean up/i));
    await user.dblClick(button(/^remove$/i));
    expect(disabled(button(/removing/i))).toBe(true);
    expect(disabled(button(/^cancel$/i))).toBe(true);
    await act(async () => finish(600_000_000));
    await flush();
    expect(count(calls, "machine_storage_clean")).toBe(1);
    // The attack box image is named so it is kept: it is the slowest download of all.
    expect(calls.find((c) => c.cmd === "machine_storage_clean")?.args?.extraImages).toContain(getAttackImage());
    expect(text()).toContain("Freed 600 MB.");
    // The list is read again after the clean-up.
    expect(count(calls, "machine_storage")).toBe(2);
  });

  it("opens and closes the list with the keyboard, saying whether it is open", async () => {
    installTauri({ machine_storage: storage });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    const toggle = screen.getByRole("button", { expanded: false });
    toggle.focus();
    await user.keyboard("{Enter}");
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(text()).toContain("generic/alpine318");
    await user.keyboard(" ");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("MachineSetup window", () => {
  it("retries a failed machine check from the window", async () => {
    installTauri();
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<MachineSetup report={null} checkError="vagrant --version timed out" onRefresh={onRefresh} onClose={vi.fn()} />);
    expect(text()).toContain("vagrant --version timed out");
    await user.click(button(/try again/i));
    expect(onRefresh).toHaveBeenCalledOnce();
    rerender(<MachineSetup report={readyReport()} checkError={null} onRefresh={onRefresh} onClose={vi.fn()} />);
    await flush();
    expect(text()).toContain("Step 1 of 8");
  });

  it("opens at a step asked for before the machine check answered", async () => {
    installTauri();
    const { rerender } = render(<MachineSetup report={null} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "virtualization", nonce: 1 }} />);
    const report = { ...freshReport("windows"), pkgManager: { name: "winget", installed: true, version: "1.9" } };
    rerender(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "virtualization", nonce: 1 }} />);
    await flush();
    // Windows: the seven steps, the WSL one and the final "ready" page.
    expect(text()).toContain("Step 2 of 9");
    expect(text()).toContain("Turn on WSL 2");
  });

  it("jumps again on a later Fix while the window is open, and only once per request", async () => {
    installTauri();
    const user = userEvent.setup();
    const props = { report: readyReport(), onRefresh: vi.fn(), onClose: vi.fn() };
    const { rerender } = render(<MachineSetup {...props} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Step 6 of 8");
    // The player moves on; the same request must not pull them back.
    await user.click(button(/^continue$/i));
    await flush();
    rerender(<MachineSetup {...props} startAt={{ step: "vagrant", nonce: 1 }} />);
    expect(text()).toContain("Step 7 of 8");
    rerender(<MachineSetup {...props} startAt={{ step: "docker", nonce: 2 }} />);
    expect(text()).toContain("Step 2 of 8");
  });

  it("holds Back and Continue while a self-test runs on its step", async () => {
    let finish: () => void = () => {};
    installTauri({ machine_selftest: () => new Promise<null>((r) => (finish = () => r(null))) });
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "docker-test", nonce: 1 }} />);
    await flush();
    expect(disabled(button(/back$/i))).toBe(true);
    expect(disabled(button(/^continue$/i))).toBe(true);
    await act(async () => finish());
    await flush();
    expect(disabled(button(/back$/i))).toBe(false);
    expect(disabled(button(/^continue$/i))).toBe(false);
  });

  // Bug: a test step's SelfTest runs on mount (`auto`), and its "already ran" flag lives in the
  // component. Going Back from a passed test step and Continue again mounts a new SelfTest that
  // runs the whole test again; for the VM test that is another VM boot of several minutes,
  // though setup.vmTest already says "ok". Suggested fix (setup-steps/index.tsx): pass
  // `auto={setup.dockerTest === "idle"}` (and the same for vmTest), and seed SelfTest's result
  // from the setup state so the passed checklist shows again.
  it("does not run a passed test again when the player comes back to its step", async () => {
    const calls = installTauri({ machine_selftest: () => null });
    const user = userEvent.setup();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "docker-test", nonce: 1 }} />);
    await flush();
    expect(text()).toContain("Passed");
    await user.click(button(/back$/i));
    await user.click(button(/^continue$/i));
    await flush();
    expect(count(calls, "machine_selftest")).toBe(1);
  });

  it("closes from the last page", async () => {
    installTauri();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={onClose} startAt={{ step: "vm-test", nonce: 1 }} />);
    await flush();
    await user.click(button(/^continue$/i));
    expect(text()).toContain("Step 8 of 8");
    await user.click(button(/done$/i));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
