// @vitest-environment jsdom
import { act, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emit, listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppShell } from "@/components/app-shell";
import { UpdateBanner } from "@/components/update-banner";
import { Screen } from "@/components/shell/screen";
import { SettingsWindowView, isSettingsWindow } from "@/components/shell/settings-window";
import { useSystemCheck } from "@/components/shell/use-system-check";
import { useAuthSession } from "@/components/shell/use-auth-session";
import { Toaster } from "@/components/ui/toaster";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { QuitGuard } from "@/features/app/quit-guard";
import { AUTH_CHANGED_EVENT, NAVIGATE_EVENT, REPLAY_ONBOARDING_EVENT } from "@/lib/app-events";
import { SIGNED_OUT_EVENT } from "@/lib/deploy-store";
import { readyReport } from "@/test/fixtures";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";
import { blockQuit, askLaunch, focusedPalette, openMainWindow } from "@/components/shell-test-kit";
import { emitSettled, must, toastsShown } from "@/components/test-support";
import { resetUpdater, updater } from "@/components/updater-double";

vi.mock("@tauri-apps/plugin-updater", () => import("@/components/updater-double"));

const mockUserAgent = (ua: string) => vi.spyOn(navigator, "userAgent", "get").mockReturnValue(ua);

beforeEach(resetUpdater);
afterEach(() => {
  vi.restoreAllMocks();
});

// The banner with no lab deploying (the dev mock's sample data has one mid-deploy, which holds
// the update back: see "waits for running deploys" below).
const idleTauri = (overrides: Parameters<typeof installTauri>[0] = {}) => installTauri({ deploying_labs: () => [], ...overrides });

describe("UpdateBanner", () => {
  /** The banner, once its check answered, after the player clicked Update. */
  const clickUpdate = async () => {
    const user = userEvent.setup();
    render(<UpdateBanner />);
    await flush();
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    return user;
  };

  it("waits for running deploys: installing would end them", async () => {
    let deploying = ["web-sqli"];
    idleTauri({ deploying_labs: () => deploying });
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    updater.pending = { version: "1.5.0", downloadAndInstall };
    render(<UpdateBanner />);
    await flush();
    expect(screen.getByRole("status").textContent).toContain("Wait until your labs finish deploying");
    const button = screen.getByRole("button", { name: "Update" });
    expect(button.hasAttribute("disabled")).toBe(true);
    // A deploy that starts after the last poll is still caught when Update is clicked.
    deploying = [];
    fireEvent(window, new Event("focus"));
    await flush();
    expect(button.hasAttribute("disabled")).toBe(false);
    deploying = ["web-sqli"];
    fireEvent.click(button);
    await flush();
    expect(downloadAndInstall).not.toHaveBeenCalled();
  });

  it("stays hidden when up to date, and when the check fails", async () => {
    idleTauri();
    const { unmount } = render(<UpdateBanner />);
    await flush();
    expect(screen.queryByRole("status")).toBeNull();
    unmount();
    updater.fail = true;
    render(<UpdateBanner />);
    await flush();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("offers the new version, installs it and relaunches", async () => {
    const calls = idleTauri({ restart_app: () => null });
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    updater.pending = { version: "1.5.0", downloadAndInstall };
    const user = userEvent.setup();
    render(<UpdateBanner />);
    await flush();
    expect(screen.getByRole("status").textContent).toContain("Version 1.5.0 is available.");
    // Ghost glass, not the jewel: it shows on every screen, beside each screen's own primary.
    expect(screen.getByRole("button", { name: "Update" }).className).toContain("btn-glass");
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toContain("Update installed");
    expect(commands(calls)).toContain("restart_app");
  });

  it("disables the button while downloading", async () => {
    idleTauri();
    let finish: () => void = () => {};
    updater.pending = { version: "1.5.0", downloadAndInstall: () => new Promise<void>((r) => (finish = r)) };
    const user = userEvent.setup();
    render(<UpdateBanner />);
    await flush();
    await user.click(screen.getByRole("button", { name: "Update" }));
    const busy = screen.getByRole("button", { name: "Updating…" });
    expect(busy.hasAttribute("disabled")).toBe(true);
    finish();
    await flush();
  });

  it("says when the update failed", async () => {
    idleTauri();
    updater.pending = {
      version: "1.5.0",
      downloadAndInstall: () => Promise.reject(new Error("signature mismatch")),
    };
    await clickUpdate();
    expect(screen.getByRole("status").textContent).toContain("Update failed");
    // Retry stays possible.
    expect(screen.getByRole("button", { name: "Update" })).toBeTruthy();
  });

  it("keeps Restart now when the relaunch fails after a good install", async () => {
    // The relaunch failing used to report the (installed) update as failed, hiding Restart now.
    let restarts = 0;
    idleTauri({
      restart_app: () => {
        restarts++;
        throw new Error("relaunch denied");
      },
    });
    updater.pending = { version: "1.5.0", downloadAndInstall: () => Promise.resolve() };
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const user = await clickUpdate();
    expect(screen.getByRole("status").textContent).toContain("Update installed");
    expect(screen.getByRole("status").textContent).not.toContain("failed");
    await user.click(screen.getByRole("button", { name: "Restart now" }));
    await flush();
    expect(restarts).toBe(2);
  });
});

describe("QuitGuard", () => {
  it("appears when a quit is blocked, with the count", async () => {
    installTauri();
    render(<QuitGuard />);
    await flush();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    await emitSettled("quit-blocked", 3);
    expect(screen.getByRole("alertdialog").textContent).toContain("3 labs are being worked on");
    // Focus moves into the dialog.
    expect(screen.getByRole("alertdialog").contains(document.activeElement)).toBe(true);
  });

  it("reads a payload that isn't a count as one lab", async () => {
    installTauri();
    await blockQuit(null);
    expect(screen.getByRole("alertdialog").textContent).toContain("A lab is being worked on");
  });

  it("keeps waiting: the dialog closes and nothing quits", async () => {
    const calls = installTauri();
    const user = userEvent.setup();
    await blockQuit();
    await user.click(screen.getByRole("button", { name: "Keep waiting" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(commands(calls)).not.toContain("force_quit");
    expect(commands(calls)).not.toContain("linger_quit");
  });

  it.each([
    ["Quit anyway", "force_quit"],
    ["Finish in background", "linger_quit"],
  ])("%s calls %s once, and re-enables the buttons if it fails", async (label, cmd) => {
    let calls = 0;
    installTauri({
      [cmd]: () => {
        calls++;
        throw new Error("still busy");
      },
    });
    const user = userEvent.setup();
    await blockQuit();
    await user.click(screen.getByRole("button", { name: label }));
    await flush();
    expect(calls).toBe(1);
    expect(screen.getByRole("button", { name: label }).hasAttribute("disabled")).toBe(false);
  });

  it("keeps Tab inside the dialog", async () => {
    installTauri();
    const user = userEvent.setup();
    await blockQuit();
    const buttons = within(screen.getByRole("alertdialog")).getAllByRole("button");
    // jsdom lays nothing out: every button reads as visible for the trap.
    buttons.forEach((b) => Object.defineProperty(b, "offsetParent", { get: () => document.body }));
    buttons[buttons.length - 1].focus();
    await user.tab();
    expect(document.activeElement).toBe(buttons[0]);
    await user.tab({ shift: true });
    expect(document.activeElement).toBe(buttons[buttons.length - 1]);
  });
});

describe("Screen", () => {
  const base = {
    report: null,
    checkError: null,
    auth: null,
    openLab: { slug: null, tick: 0 },
    onRefresh: vi.fn(),
    onNavigate: vi.fn(),
    onAuthChange: vi.fn(),
    onLabChange: vi.fn(),
  };

  it("waits for the machine check on Machine and Labs", () => {
    installTauri();
    const { rerender } = render(<Screen {...base} tab="machine" />);
    expect(document.body.textContent).toMatch(/checking/i);
    // Labs: the list's header and placeholder rows, not a bare spinner.
    rerender(<Screen {...base} tab="labs" />);
    expect(document.querySelector("[aria-busy=true]")).toBeTruthy();
    expect(document.body.textContent).toContain("Labs");
  });

  it("offers a retry when the machine check failed", async () => {
    installTauri();
    const onRefresh = vi.fn();
    const user = userEvent.setup();
    render(<Screen {...base} tab="machine" checkError="docker: permission denied" onRefresh={onRefresh} />);
    expect(screen.getByText("docker: permission denied")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /try again/i }));
    expect(onRefresh).toHaveBeenCalled();
  });

  it("says Events is coming soon", () => {
    installTauri();
    render(<Screen {...base} tab="events" />);
    expect(document.body.textContent).toMatch(/on the way/i);
  });

  it("signs in from the Labs screen", async () => {
    const calls = installTauri({ auth_login: () => ({ loggedIn: true, name: "Ada", email: "ada@example.com" }) });
    const onAuthChange = vi.fn();
    const user = userEvent.setup();
    render(<Screen {...base} tab="labs" report={readyReport()} auth={{ loggedIn: false, name: null, email: null }} onAuthChange={onAuthChange} />);
    await flush(50);
    // One banner above the list signs in, not a button on every row.
    expect(document.querySelector('[title="Sign in to start labs"]')).toBeNull();
    const signIn = screen.getAllByRole("button", { name: "Sign in" });
    expect(signIn).toHaveLength(1);
    await user.click(signIn[0]);
    await flush();
    expect(commands(calls)).toContain("auth_login");
    expect(onAuthChange).toHaveBeenCalledWith({ loggedIn: true, name: "Ada", email: "ada@example.com" });
  });
});

describe("SettingsWindowView", () => {
  it("draws its own title band on macOS only", async () => {
    installTauri();
    mockUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    const { unmount } = render(<SettingsWindowView auth={null} onAuthChange={vi.fn()} />);
    await flush();
    expect(document.querySelector("[data-tauri-drag-region]")?.textContent).toBe("Settings");
    unmount();
    mockUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)");
    render(<SettingsWindowView auth={null} onAuthChange={vi.fn()} />);
    await flush();
    expect(document.querySelector("[data-tauri-drag-region]")).toBeNull();
  });

  it("knows the Settings window by its URL", () => {
    expect(isSettingsWindow()).toBe(false);
    window.history.pushState({}, "", "/?window=settings");
    expect(isSettingsWindow()).toBe(true);
    window.history.pushState({}, "", "/");
  });
});

describe("useSystemCheck", () => {
  it("reports the check, or why it failed, and re-checks on focus at most every 15 s", async () => {
    let fail = true;
    const calls = installTauri({
      system_check: () => {
        if (fail) throw new Error("docker hung");
        return readyReport();
      },
    });
    const now = vi.spyOn(Date, "now").mockReturnValue(1_000_000);
    const { result } = renderHook(() => useSystemCheck());
    await act(() => result.current.check());
    expect(result.current.checkError).toContain("docker hung");
    expect(result.current.report).toBeNull();

    fail = false;
    // Too soon: no re-check.
    now.mockReturnValue(1_005_000);
    fireEvent.focus(window);
    await flush();
    expect(commands(calls).filter((c) => c === "system_check")).toHaveLength(1);
    now.mockReturnValue(1_020_000);
    fireEvent.focus(window);
    await flush();
    expect(commands(calls).filter((c) => c === "system_check")).toHaveLength(2);
    expect(result.current.report?.os).toBe("macos");
    expect(result.current.checkError).toBeNull();
  });
});

describe("AppShell", () => {
  beforeEach(() => {
    localStorage.setItem("cyberctf.onboarded", "1");
  });

  it("shows the onboarding on first launch", async () => {
    localStorage.removeItem("cyberctf.onboarded");
    installTauri();
    render(<AppShell />);
    await flush(50);
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  it("opens the palette with Ctrl+K, and closes it with Escape", async () => {
    installTauri();
    const user = userEvent.setup();
    await openMainWindow();
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.keyboard("{Control>}k{/Control}");
    // The search field takes focus on the next animation frame: Escape is read inside the palette.
    await focusedPalette();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("jumps to the lab search with /", async () => {
    installTauri();
    await openMainWindow();
    fireEvent.keyDown(window, { key: "/" });
    await flush(50);
    expect(document.querySelector("[data-lab-search]")).not.toBeNull();
  });

  it("opens Settings with Ctrl+, on Windows and Linux", async () => {
    const calls = installTauri({ open_settings: () => null });
    await openMainWindow();
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    await flush();
    expect(commands(calls)).toContain("open_settings");
  });

  it("leaves Cmd+, to the macOS menu", async () => {
    mockUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    const calls = installTauri({ open_settings: () => null });
    await openMainWindow();
    fireEvent.keyDown(window, { key: ",", metaKey: true });
    await flush();
    expect(commands(calls)).not.toContain("open_settings");
  });

  it("says when Settings can't open", async () => {
    installTauri({
      open_settings: () => {
        throw new Error("window busy");
      },
    });
    toastsShown();
    await openMainWindow();
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    await flush();
    expect(document.body.textContent).toContain("window busy");
  });

  it("switches screens when Settings asks, and ignores unknown ones", async () => {
    const calls = installTauri();
    await openMainWindow();
    await act(() => emit(NAVIGATE_EVENT, "server"));
    await flush(50);
    expect(commands(calls)).toContain("server_list");
    await act(() => emit(NAVIGATE_EVENT, "nowhere"));
    await flush();
    expect(screen.getByRole("navigation")).toBeTruthy();
  });

  it("shows the onboarding again when Settings asks", async () => {
    installTauri();
    await openMainWindow();
    await act(() => emit(REPLAY_ONBOARDING_EVENT, null));
    await flush(50);
    expect(screen.queryByRole("navigation")).toBeNull();
  });

  it("runs as just the Settings screen in the Settings window", async () => {
    window.history.pushState({}, "", "/?window=settings");
    try {
      installTauri();
      render(<AppShell />);
      await flush(50);
      // No sidebar: the Settings screen alone.
      expect(document.querySelector("aside")).toBeNull();
      expect(document.querySelector("main")).not.toBeNull();
    } finally {
      window.history.pushState({}, "", "/");
    }
  });
});

describe("LaunchConfirm", () => {
  const request = { sessionId: "sess-9", repository: "cyber-courses/lab-s3", commit: "0123456789abcdef0123", target: "AWS sandbox" };

  it.each([
    ["Run on my cloud", true],
    ["Decline", false],
  ])("%s answers the agent once, then closes", async (label, approve) => {
    const calls = installTauri({ confirm_launch: () => null });
    const user = userEvent.setup();
    await askLaunch(request);
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.textContent).toContain("AWS sandbox");
    expect(dialog.textContent).toContain("cyber-courses/lab-s3");
    // The commit, shortened.
    expect(dialog.textContent).toContain("commit: 0123456789ab");
    expect(dialog.textContent).not.toContain("0123456789abc");
    await user.click(screen.getByRole("button", { name: label }));
    await flush();
    expect(calls.filter((c) => c.cmd === "confirm_launch").map((c) => c.args)).toEqual([{ sessionId: "sess-9", approve }]);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("says when the answer couldn't be sent", async () => {
    installTauri({
      confirm_launch: () => {
        throw new Error("agent gone");
      },
    });
    toastsShown();
    const user = userEvent.setup();
    render(
      <>
        <LaunchConfirm />
        <Toaster />
      </>,
    );
    await flush();
    await emitSettled("launch-confirm", request);
    await user.click(screen.getByRole("button", { name: "Decline" }));
    await flush();
    expect(document.body.textContent).toContain("agent gone");
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

describe("useAuthSession", () => {
  it("reads as signed out when the first read fails", async () => {
    installTauri({
      auth_status: () => {
        throw new Error("keychain locked");
      },
    });
    const { result } = renderHook(() => useAuthSession());
    act(() => {
      result.current.init();
    });
    await flush();
    expect(result.current.auth).toEqual({ loggedIn: false, name: null, email: null });
  });

  it("reads the session again on focus, a signed-out action, or another window's change", async () => {
    let status = { loggedIn: true, name: "Ada" as string | null, email: "ada@example.com" as string | null };
    const calls = installTauri({ auth_status: () => status });
    const { result } = renderHook(() => useAuthSession());
    act(() => {
      result.current.init();
    });
    await flush();
    expect(result.current.auth?.loggedIn).toBe(true);
    status = { loggedIn: false, name: null, email: null };
    fireEvent(window, new Event(SIGNED_OUT_EVENT));
    await flush();
    expect(result.current.auth?.loggedIn).toBe(false);
    const before = calls.filter((c) => c.cmd === "auth_status").length;
    fireEvent.focus(window);
    await act(() => emit(AUTH_CHANGED_EVENT, null));
    await flush();
    expect(calls.filter((c) => c.cmd === "auth_status").length).toBe(before + 2);
  });

  it("keeps the last session when a re-read fails, and tells other windows of a change", async () => {
    let fail = false;
    installTauri({
      auth_status: () => {
        if (fail) throw new Error("offline");
        return { loggedIn: true, name: "Ada", email: "ada@example.com" };
      },
    });
    const heard = vi.fn();
    await listen(AUTH_CHANGED_EVENT, heard);
    const { result } = renderHook(() => useAuthSession());
    act(() => {
      result.current.init();
    });
    await flush();
    fail = true;
    act(() => {
      result.current.reread();
    });
    await flush();
    expect(result.current.auth?.name).toBe("Ada");
    act(() => {
      result.current.changed({ loggedIn: false, name: null, email: null });
    });
    await flush();
    expect(result.current.auth?.loggedIn).toBe(false);
    expect(heard).toHaveBeenCalled();
  });
});

describe("AppShell palette", () => {
  const paletteField = () => must(screen.getByRole("dialog").querySelector("input"), "the palette's search field");

  it("runs a screen or an action from the palette", async () => {
    const calls = installTauri({ open_settings: () => null });
    const user = userEvent.setup();
    await openMainWindow(80);
    await user.click(screen.getByRole("button", { name: /find/i }));
    await user.type(paletteField(), "Cloud");
    await user.keyboard("{Enter}");
    await flush(50);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("button", { current: "page" }).textContent).toContain("Cloud");
    await user.keyboard("{Control>}k{/Control}");
    await user.type(paletteField(), "settings");
    await user.keyboard("{Enter}");
    await flush();
    expect(commands(calls)).toContain("open_settings");
  });
});
