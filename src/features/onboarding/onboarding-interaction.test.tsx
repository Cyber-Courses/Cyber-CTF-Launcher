// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Onboarding } from "@/features/onboarding/onboarding";
import { setLocalePreference } from "@/lib/i18n";
import type { SystemReport } from "@/lib/tauri";
import { freshReport, readyReport } from "@/test/fixtures";
import { commands, installTauri, rejects, rejectWith, stream, type Handler } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const text = () => document.body.textContent;
const signedOut = () => ({ loggedIn: false, name: null, email: null });
/** An install that fails as brew does, with the Rust core's `other` error. */
const brewFails = rejects({ code: "other", message: "brew exited with status 1" });
/** The mono "3 / 10 · Package manager" line under the mark. */
const progress = () => screen.getByText((_, el) => el?.tagName === "P" && /^\d+ \/ \d+ · /.test(el.textContent)).textContent;

type User = ReturnType<typeof userEvent.setup>;

/** The onboarding with `handlers` answering its commands, once its first reads have settled. */
async function renderOnboarding(handlers: Record<string, Handler> = {}, user: User = userEvent.setup()) {
  const calls = installTauri(handlers);
  const onComplete = vi.fn();
  render(<Onboarding onComplete={onComplete} />);
  await flush();
  return { calls, user, onComplete };
}

/** Renders the onboarding on `report` and moves past the welcome and the sign-in. */
async function atFirstMachineStep(report: SystemReport | (() => SystemReport), extra: Record<string, Handler> = {}) {
  const opened = await renderOnboarding({ auth_status: signedOut, system_check: typeof report === "function" ? report : () => report, ...extra });
  await opened.user.click(button(/get started/i));
  // Signed out: the sign-in step is skipped for now.
  await opened.user.click(button(/^skip for now$/i));
  await flush();
  return opened;
}

/** Presses the way forward (a button matching `forward`) until the summary, or none is left. */
async function forwardToSummary(user: User, forward: RegExp) {
  for (let n = 0; n < 12 && !/Done$/.test(progress()); n++) {
    const next = screen.queryAllByRole("button").find((b) => forward.test(b.textContent.trim()) && !b.hasAttribute("disabled"));
    if (!next) return;
    await user.click(next);
    await flush();
  }
}

describe("Onboarding: the step list per OS", () => {
  it.each([
    ["macos", 10],
    ["linux", 10],
    // Windows gets the WSL step on top.
    ["windows", 11],
  ])("counts every step of a fresh %s machine in the progress line", async (os, total) => {
    await atFirstMachineStep(freshReport(os));
    expect(progress()).toBe(`3 / ${String(total)} · Package manager`);
  });

  it("puts the WSL step right after the package manager on Windows only", async () => {
    const report = { ...freshReport("windows"), pkgManager: { name: "winget", installed: true, version: "1.9" } };
    const { user } = await atFirstMachineStep(report);
    await user.click(button(/^continue$/i));
    await flush();
    expect(progress()).toBe("4 / 11 · Virtualization");
    expect(text()).toContain("Turn on WSL 2");
  });

  it("holds a fresh machine on the package manager step until one is installed, on every OS", async () => {
    for (const os of ["macos", "windows", "linux"]) {
      await atFirstMachineStep(freshReport(os));
      // Continue waits: nothing past this step works without a package manager.
      expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
      cleanup();
    }
  });

  it("gives each OS its own way to get a package manager", async () => {
    await atFirstMachineStep(freshReport("macos"));
    expect(text()).toContain("Homebrew/install");
    cleanup();
    await atFirstMachineStep(freshReport("windows"));
    expect(text()).toContain("App Installer");
    cleanup();
    await atFirstMachineStep(freshReport("linux"));
    expect(text()).toContain("apt");
  });
});

describe("Onboarding: Back, Continue and Skip", () => {
  it("goes back from the first machine step to the sign-in, then to the welcome", async () => {
    const { user } = await atFirstMachineStep(readyReport());
    await user.click(button(/back$/i));
    expect(progress()).toBe("2 / 10 · Sign in");
    await user.click(button(/back$/i));
    expect(progress()).toBe("1 / 10 · Welcome");
    expect(button(/get started/i).hasAttribute("disabled")).toBe(false);
  });

  it("reaches the summary on a ready machine and hides Skip there", async () => {
    const { user, onComplete } = await atFirstMachineStep(readyReport());
    await forwardToSummary(user, /^(continue|skip)$/i);
    expect(progress()).toBe("10 / 10 · Done");
    // The summary has its own way out; "Skip setup" there would be the same button twice.
    expect(screen.queryByRole("button", { name: /skip setup/i })).toBeNull();
    await user.click(button(/browse labs/i));
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("skips the whole onboarding from any step", async () => {
    const { user, onComplete } = await atFirstMachineStep(freshReport("linux"));
    await user.click(screen.getByRole("button", { name: /skip setup/i }));
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("moves forward with the keyboard alone", async () => {
    const { user } = await renderOnboarding({ auth_status: signedOut });
    button(/get started/i).focus();
    await user.keyboard("{Enter}");
    expect(progress()).toBe("2 / 10 · Sign in");
  });

  it("follows a language change made while it is open", async () => {
    await atFirstMachineStep(readyReport());
    act(() => {
      setLocalePreference("fr");
    });
    expect(text()).not.toContain("Package manager");
    act(() => {
      setLocalePreference("system");
    });
    expect(text()).toContain("Package manager");
  });
});

describe("Onboarding: sign-in", () => {
  afterEach(() => vi.useRealTimers());

  it("says why a sign-in failed, and clears it on the next try", async () => {
    let attempt = 0;
    let finish: () => void = () => {};
    const { user } = await renderOnboarding({
      auth_status: signedOut,
      auth_login: () => {
        attempt += 1;
        if (attempt === 1) throw new Error("no browser could be opened");
        return new Promise(
          (r) =>
            (finish = () => {
              r({ loggedIn: true, name: "Ada", email: "ada@example.com" });
            }),
        );
      },
    });
    await user.click(button(/get started/i));
    await user.click(button(/sign in with your browser/i));
    await flush();
    expect(screen.getByRole("alert").textContent).toContain("no browser could be opened");
    await user.click(button(/sign in with your browser/i));
    // Waiting for the browser again: the old reason is gone and the button is busy.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(button(/waiting for the browser/i).hasAttribute("disabled")).toBe(true);
    finish();
    await flush();
    expect(text()).toContain("Signed in as Ada");
    expect(text()).toContain("ada@example.com");
  });

  it("opens the browser once however often the sign-in button is pressed", async () => {
    const { user, calls } = await renderOnboarding({ auth_status: signedOut, auth_login: () => new Promise(() => {}) });
    await user.click(button(/get started/i));
    const signIn = button(/sign in with your browser/i);
    await user.dblClick(signIn);
    await user.click(signIn);
    await flush();
    expect(commands(calls).filter((c) => c === "auth_login")).toHaveLength(1);
  });

  it("lets a signed-out player skip signing in for now, and says where to do it later", async () => {
    const { user, calls } = await renderOnboarding({ auth_status: signedOut });
    await user.click(button(/get started/i));
    // Optional, and said so on the button: no plain "Continue" past a step left undone.
    expect(text()).toContain("sign in later from Settings");
    expect(screen.queryByRole("button", { name: /^continue$/i })).toBeNull();
    await user.click(button(/^skip for now$/i));
    expect(commands(calls)).not.toContain("auth_login");
    expect(text()).toContain("Package manager");
  });

  it("says Continue on the sign-in step once signed in", async () => {
    const { user } = await renderOnboarding();
    await user.click(button(/get started/i));
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
    expect(screen.queryByRole("button", { name: /skip for now/i })).toBeNull();
  });

  it("treats an unreadable sign-in state as signed out", async () => {
    const { user } = await renderOnboarding({
      auth_status: () => {
        throw new Error("keychain locked");
      },
    });
    await user.click(button(/get started/i));
    expect(button(/sign in with your browser/i).hasAttribute("disabled")).toBe(false);
  });
});

describe("Onboarding: a machine check that fails, then recovers", () => {
  it("says the check failed with a Try again, and shows the step once a retry answers", async () => {
    let fail = true;
    const { user } = await atFirstMachineStep(() => {
      if (fail) throw new Error("docker info hung");
      return readyReport();
    });
    expect(text()).toContain("Couldn’t check this machine");
    expect(text()).toContain("docker info hung");
    fail = false;
    await user.click(button(/try again/i));
    await flush();
    expect(text()).not.toContain("Couldn’t check this machine");
    expect(text()).toContain("installs everything else for you");
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(false);
  });

  it("shows the latest reason when a retry fails again", async () => {
    let n = 0;
    const { user, calls } = await atFirstMachineStep(() => {
      n += 1;
      throw new Error(`attempt ${String(n)} failed`);
    });
    const before = commands(calls).filter((c) => c === "system_check").length;
    await user.click(button(/try again/i));
    await flush();
    expect(commands(calls).filter((c) => c === "system_check").length).toBe(before + 1);
    expect(text()).toContain(`attempt ${String(n)} failed`);
  });

  it("recovers on its own on the next poll, without Try again", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let fail = true;
    await atFirstMachineStep(() => {
      if (fail) throw new Error("hung");
      return readyReport();
    });
    expect(text()).toContain("Try again");
    fail = false;
    await act(() => vi.advanceTimersByTimeAsync(5000));
    await flush();
    expect(text()).not.toContain("Try again");
    vi.useRealTimers();
  });

  // Bug: while the machine check has not answered (or keeps failing), a machine step shows only
  // the loader or the "Try again" panel: no Back button. The player cannot return to the sign-in
  // step to sign in; the only ways out are a check that succeeds or "Skip setup", which ends the
  // whole onboarding. Suggested fix (onboarding.tsx, the `report === null` branch): render
  // <StepActions onBack={back} onNext={next} disabled nextLabel={t("onboarding.continue")} />
  // under <WaitingForCheck>.
  it("keeps a Back button on a machine step while the check is failing", async () => {
    const { user } = await atFirstMachineStep(() => {
      throw new Error("hung");
    });
    await user.click(button(/back$/i));
    expect(progress()).toBe("2 / 10 · Sign in");
  });
});

describe("Onboarding: installs that stream and then fail", () => {
  it("streams the WSL install log on Windows, ends it with ✗ and asks for no restart", async () => {
    const report = { ...freshReport("windows"), pkgManager: { name: "winget", installed: true, version: "1.9" } };
    let fail: (e: Error) => void = () => {};
    const { user } = await atFirstMachineStep(report, {
      install_dependency: (a) => {
        stream(a, "Enabling Virtual Machine Platform", "Downloading WSL kernel");
        return new Promise((_, reject) => (fail = reject));
      },
    });
    await user.click(button(/^continue$/i));
    await flush();
    await user.click(button(/turn on wsl 2/i));
    await flush();
    expect(text()).toContain("Downloading WSL kernel");
    // An install is running: neither Back nor the forward button may leave the step.
    expect(button(/back$/i).hasAttribute("disabled")).toBe(true);
    expect(button(/done this$/i).hasAttribute("disabled")).toBe(true);
    fail(new Error("The user declined the UAC prompt"));
    await flush();
    expect(text()).toContain("✗ The user declined the UAC prompt");
    // A declined prompt is not fixed by a restart: the restart notice stays away.
    expect(screen.queryByRole("status")).toBeNull();
    expect(button(/turn on wsl 2/i).hasAttribute("disabled")).toBe(false);
    expect(button(/back$/i).hasAttribute("disabled")).toBe(false);
  });

  it("streams the Docker Desktop install log and ends it with ✗ when it fails", async () => {
    const report = { ...freshReport("macos"), pkgManager: { name: "brew", installed: true, version: "4.5" } };
    const { user, calls } = await atFirstMachineStep(report, {
      install_dependency: (a) => {
        stream(a, "==> Downloading docker.dmg", "==> Verifying checksum");
        return brewFails(a);
      },
    });
    await user.click(button(/^continue$/i));
    await flush();
    await user.click(button(/install docker desktop/i));
    await flush();
    expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "docker" });
    const log = text();
    expect(log.indexOf("==> Downloading docker.dmg")).toBeLessThan(log.indexOf("==> Verifying checksum"));
    expect(log).toContain("✗ brew exited with status 1");
    // A failed install can be tried again, and the flow still waits for an engine.
    expect(button(/install docker desktop/i).hasAttribute("disabled")).toBe(false);
    expect(button(/^continue$/i).hasAttribute("disabled")).toBe(true);
  });

  it("keeps an install's log on its own step only", async () => {
    const report = { ...freshReport("macos"), pkgManager: { name: "brew", installed: true, version: "4.5" } };
    const { user } = await atFirstMachineStep(report, {
      install_dependency: brewFails,
    });
    await user.click(button(/^continue$/i));
    await flush();
    await user.click(button(/install docker desktop/i));
    await flush();
    expect(text()).toContain("✗ brew exited with status 1");
    await user.click(button(/back$/i));
    expect(text()).not.toContain("✗ brew exited with status 1");
  });
});

describe("Onboarding: the summary and the machine check", () => {
  afterEach(() => vi.useRealTimers());

  it("shows who is signed in, and goes back to the welcome", async () => {
    const { user } = await renderOnboarding({ auth_status: () => ({ loggedIn: true, name: null, email: "p@example.com" }) });
    await user.click(button(/get started/i));
    await flush();
    expect(text()).toContain("Signed in");
    expect(text()).toContain("p@example.com");
    await user.click(button(/back/i));
    expect(text()).toContain("Get started");
  });

  it("waits for the machine check before showing a machine step", async () => {
    const { user } = await renderOnboarding({ system_check: () => new Promise(() => {}) });
    await user.click(button(/get started/i));
    await user.click(button(/^continue$/i));
    await flush();
    expect(text()).toContain("Checking this machine…");
  });

  it("summarises a signed-out player on the last step", async () => {
    const { user, onComplete } = await renderOnboarding({ auth_status: signedOut, system_check: () => ({ ...readyReport(), vmProviders: [] }) });
    await forwardToSummary(user, /^(get started|continue|skip for now)/i);
    expect(text()).toContain("Not signed in");
    expect(screen.queryByRole("button", { name: /skip setup/i })).toBeNull();
    await user.click(button(/browse labs/i));
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("keeps the current step when a machine re-check fails", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let fail = false;
    const { user } = await renderOnboarding(
      {
        system_check: () => {
          if (fail) rejectWith("system check timed out");
          return { ...readyReport(), os: "windows", dockerRunning: false };
        },
      },
      userEvent.setup({ advanceTimers: vi.advanceTimersByTime }),
    );
    await user.click(button(/get started/i));
    await user.click(button(/^continue$/i));
    await flush();
    await user.click(button(/^continue$/i));
    await flush();
    expect(text()).toContain("Turn on WSL 2");
    expect(text()).toContain("4 / 11");
    fail = true;
    await act(() => vi.advanceTimersByTimeAsync(5_100));
    await flush();
    // Still on the WSL step: a failed re-check neither blanks it nor shifts the step list.
    expect(text()).toContain("Turn on WSL 2");
    expect(text()).toContain("4 / 11");
  });
});
