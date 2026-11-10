// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Toaster } from "@/components/ui/toaster";
import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { MachineScreen } from "@/features/machine/machine-screen";
import { MachineSetup } from "@/features/machine/machine-setup";
import { SelfTest } from "@/features/machine/self-test";
import { MachineStepBody, useMachineSetup, type MachineStep } from "@/features/machine/setup-steps";
import { setLocalePreference } from "@/lib/i18n";
import { getAttackImage, getLastTest } from "@/lib/settings";
import type { SelfTestEvent, SystemReport } from "@/lib/tauri";
import type { SelfTestResult } from "@/features/machine/use-self-test";
import { freshReport, readyReport, withSetup } from "@/test/fixtures";
import { answersDialog, commands, dialogsAsked, installTauri, rejectWith, stream } from "@/test/tauri";
import { button, flush, settle } from "@/test/ui";
import { pressFirstTest, radioWith, renderMachineScreen } from "@/features/machine/machine-test-kit";

const text = () => document.body.textContent;
const disabled = (el: HTMLElement) => el.hasAttribute("disabled");
const count = (calls: { cmd: string }[], cmd: string) => commands(calls as never).filter((c) => c === cmd).length;
const ev = (step: string, state: SelfTestEvent["state"], detail?: string): SelfTestEvent => ({ step, label: step, state, detail: detail ?? null });

/** One step body with a live setup state, the way both setup flows mount it, on `machine` with
 *  the setup's options the core works out for it. */
function Step({ step, report: machine, onRefresh = () => {} }: { step: MachineStep; report: SystemReport; onRefresh?: () => void }) {
  const report = withSetup(machine);
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
        rejectWith("context orbstack not found");
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

  // Regression: one start error served the whole step: after Docker Desktop failed to start, picking
  // Colima still showed Docker Desktop's error, as if Colima had failed.
  it("forgets one engine's start error when the player picks another engine", async () => {
    installTauri({
      docker_start_engine: () => {
        rejectWith("Docker Desktop.app is damaged");
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

  // Regression: the engine cards' badges ("in use", "running", "recommended") stayed English in
  // every language.
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
        return new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        );
      },
    });
    const user = userEvent.setup();
    render(<SelfTest kind="docker" title="Container lab test" description="d" />);
    await user.click(button(/run test/i));
    await flush();
    expect(screen.queryByRole("button")).toBeNull();
    await settle(() => {
      finish();
    });
    await flush();
    expect(count(calls, "machine_selftest")).toBe(1);
  });

  it("clears the last failure when it is run again, and passes", async () => {
    let fail = true;
    const calls = installTauri({
      machine_selftest: (a) => {
        if (fail) {
          stream(a, ev("engine", "ok"), ev("pull", "fail", "toomanyrequests: rate limit"));
          rejectWith("pull failed");
        }
        stream(a, ...["engine", "pull", "start", "network", "port", "cleanup"].map((s) => ev(s, "ok")));
        return null;
      },
    });
    const onResult = vi.fn<(r: SelfTestResult) => void>();
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

  it("logs a run that rejects before any step failed, and records its error as the why", async () => {
    // Regression: `catch {}` kept only a failed step's detail, so a run that rejected first (its
    // work folder, an earlier test VM that can't be removed) failed with no why and no log line.
    installTauri({ machine_selftest: () => rejectWith({ code: "other", message: "the work folder can't be made: denied" }) });
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    const onResult = vi.fn<(r: SelfTestResult) => void>();
    render(<SelfTest kind="docker" title="Container lab test" description="d" auto onResult={onResult} />);
    await flush();
    expect(onResult.mock.calls.map((c) => c[0])).toEqual(["running", "fail"]);
    expect(getLastTest("docker")?.why).toBe("the work folder can't be made: denied");
    expect(warned.mock.calls.some(([what]) => what === "self-test:")).toBe(true);
    warned.mockRestore();
  });

  it("lists every planned step as pending as soon as it starts", async () => {
    installTauri({ machine_selftest: () => new Promise(() => {}) });
    render(<SelfTest kind="vm" title="VM lab test" description="d" auto />);
    await flush();
    // The VM test's eight steps show up front, so the list doesn't jump as it runs.
    expect(screen.getAllByRole("listitem")).toHaveLength(8);
    expect(text()).toContain("Boot the test VM");
  });

  // Regression: "Hide test" while the self-test ran, then "Test", started a second self-test beside
  // the first: two test labs on the same engine.
  it("does not start a second self-test when the test is hidden and shown again mid-run", async () => {
    const calls = installTauri({ machine_selftest: () => new Promise(() => {}) });
    const user = userEvent.setup();
    await renderMachineScreen();
    await pressFirstTest(user);
    await flush();
    await user.click(button(/hide test/i));
    await pressFirstTest(user);
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
        rejectWith("window label already exists");
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
    await user.click(button(/add a provider/i));
    await flush();
    expect(text()).toContain("Couldn't open machine setup");
    expect(text()).toContain("window label already exists");
  });
});

describe("Downloads panel: Terraform plugins", () => {
  it("counts the provider plugins labs share in what is stored", async () => {
    installTauri({ machine_storage: () => ({ images: [], boxes: [], plugins: [{ name: "Terraform provider plugins", bytes: 700_000_000 }] }) });
    render(<DownloadsPanel />);
    await flush();
    expect(screen.getAllByText(/700(\.0)? MB/).length).toBeGreaterThan(0);
  });
});

describe("Downloads panel: a failed read", () => {
  it("says the read failed and why, never that nothing was downloaded", async () => {
    installTauri({ machine_storage: () => rejectWith({ code: "other", message: "docker images: permission denied" }) });
    render(<DownloadsPanel />);
    await flush();
    expect(text()).toContain("Couldn't read what Cyber CTF downloaded: docker images: permission denied");
    expect(text()).not.toContain("No lab downloads yet");
  });
});

describe("Downloads panel: clean-up", () => {
  const storage = () => ({
    images: [{ name: "cyberctf/attack:latest", bytes: 3_400_000_000 }],
    boxes: [{ name: "generic/alpine318", bytes: 600_000_000 }],
    plugins: [] as { name: string; bytes: number }[],
  });

  it("asks in the system dialog before removing anything, and Cancel keeps everything", async () => {
    const calls = installTauri({ machine_storage: storage, "plugin:dialog|message": answersDialog(false) });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    await user.click(button(/clean up/i));
    await flush();
    expect(dialogsAsked(calls)).toEqual([
      { title: "Remove 4.0 GB of lab downloads?", message: "Labs download what they need again on their next start. Anything in use stays." },
    ]);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(count(calls, "machine_storage_clean")).toBe(0);
    expect(button(/clean up/i)).toBeTruthy();
  });

  it("asks once and removes once however often Clean up is pressed, keeping the attack box image", async () => {
    let finish: (n: number) => void = () => {};
    const calls = installTauri({ machine_storage: storage, machine_storage_clean: () => new Promise((r) => (finish = r)) });
    const user = userEvent.setup();
    render(<DownloadsPanel />);
    await flush();
    await user.dblClick(button(/clean up/i));
    await flush();
    expect(count(calls, "plugin:dialog|message")).toBe(1);
    expect(disabled(button(/removing/i))).toBe(true);
    await settle(() => {
      finish(600_000_000);
    });
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
    // Opened with no step asked for: the choice of path comes first.
    expect(text()).toContain("What do you want to run?");
  });

  it("opens at a step asked for before the machine check answered", async () => {
    installTauri();
    const { rerender } = render(<MachineSetup report={null} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "virtualization", nonce: 1 }} />);
    const report = { ...freshReport("windows"), pkgManager: { name: "winget", installed: true, version: "1.9" } };
    rerender(<MachineSetup report={report} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "virtualization", nonce: 1 }} />);
    await flush();
    // Windows: the container path's steps, the WSL one among them, and the final "ready" page.
    expect(text()).toContain("Step 2 of 6");
    expect(text()).toContain("Turn on WSL 2");
  });

  it("jumps again on a later Fix while the window is open, and only once per request", async () => {
    installTauri();
    const user = userEvent.setup();
    const props = { report: readyReport(), onRefresh: vi.fn(), onClose: vi.fn() };
    const { rerender } = render(<MachineSetup {...props} startAt={{ step: "vagrant", nonce: 1 }} />);
    await flush();
    // The VM path: package manager, hypervisor, Vagrant, the test, the summary.
    expect(text()).toContain("Step 3 of 5");
    // The player moves on; the same request must not pull them back.
    await user.click(button(/^continue$/i));
    await flush();
    rerender(<MachineSetup {...props} startAt={{ step: "vagrant", nonce: 1 }} />);
    expect(text()).toContain("Step 4 of 5");
    // A step of another path switches path.
    rerender(<MachineSetup {...props} startAt={{ step: "docker", nonce: 2 }} />);
    expect(text()).toContain("Step 2 of 5");
    expect(text()).toContain("Container engine");
  });

  it("holds Back and Continue while a self-test runs on its step", async () => {
    let finish: () => void = () => {};
    installTauri({
      machine_selftest: () =>
        new Promise<null>(
          (r) =>
            (finish = () => {
              r(null);
            }),
        ),
    });
    render(<MachineSetup report={readyReport()} onRefresh={vi.fn()} onClose={vi.fn()} startAt={{ step: "docker-test", nonce: 1 }} />);
    await flush();
    expect(disabled(button(/back$/i))).toBe(true);
    expect(disabled(button(/^continue$/i))).toBe(true);
    await settle(() => {
      finish();
    });
    await flush();
    expect(disabled(button(/back$/i))).toBe(false);
    expect(disabled(button(/^continue$/i))).toBe(false);
  });

  // Regression: going Back from a passed test step and Continue again ran the whole test again (for
  // the VM test, another VM boot of several minutes).
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
    expect(text()).toContain("Step 5 of 5");
    await user.click(button(/done$/i));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
