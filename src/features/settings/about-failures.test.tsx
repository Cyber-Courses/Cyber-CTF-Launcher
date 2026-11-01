// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/features/settings/about-section";
import { UpdateBanner } from "@/components/update-banner";
import { Toaster } from "@/components/ui/toaster";
import { toastsShown } from "@/components/test-support";
import { resetUpdater, updater } from "@/components/updater-double";
import { commands, installTauri, rejects } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// What the About section and the update banner say when a step the player asked for fails:
// the replay of the first run, opening the log folder, restarting after an update.

vi.mock("@tauri-apps/plugin-updater", () => import("@/components/updater-double"));

// The event bus: `emit` fails while `bus.fail` is set (the main window is gone, say).
const bus = vi.hoisted(() => ({ fail: false }));
vi.mock("@tauri-apps/api/event", async (importOriginal) => {
  const real = await importOriginal<typeof import("@tauri-apps/api/event")>();
  return {
    ...real,
    emit: (...args: Parameters<typeof real.emit>) => (bus.fail ? Promise.reject(new Error("no window to receive it")) : real.emit(...args)),
  };
});

const RESTART_FAILED = "Couldn't restart Cyber CTF";
/** The failure toasts on screen, as text. */
const alerts = () => screen.queryAllByRole("alert").map((a) => a.textContent);

/** The About section (no report or agent) with the Toaster, so what it tells shows. */
function renderAbout() {
  render(
    <>
      <AboutSection version="1.4.0" report={null} agent={null} />
      <Toaster />
    </>,
  );
}

beforeEach(() => {
  toastsShown();
  resetUpdater();
  bus.fail = false;
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("AboutSection failures", () => {
  it("says the first run couldn't be replayed when the main window can't be told", async () => {
    bus.fail = true;
    installTauri();
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/^\s*Replay$/));
    await flush();
    expect(alerts().join("\n")).toContain("Couldn't replay the setup");
    expect(alerts().join("\n")).toContain("no window to receive it");
  });

  it("says the log folder couldn't be opened", async () => {
    const calls = installTauri({ open_log_folder: rejects({ code: "other", message: "xdg-open: command not found" }) });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/Open folder/));
    await flush();
    expect(commands(calls)).toContain("open_log_folder");
    expect(alerts().join("\n")).toContain("Couldn't open the log folder");
    expect(alerts().join("\n")).toContain("xdg-open: command not found");
  });

  it("keeps the update installed and says the restart failed, after the install and from Restart now", async () => {
    updater.pending = { version: "1.5.0", downloadAndInstall: () => Promise.resolve() };
    const calls = installTauri({ deploying_labs: () => [], restart_app: rejects({ code: "other", message: "relaunch denied" }) });
    const user = userEvent.setup();
    renderAbout();
    await user.click(button(/Check for updates/i));
    await flush();
    await user.click(button(/Install update/i));
    await flush();
    // Installed: the automatic relaunch failed, which is said, and Restart now stays.
    expect(document.body.textContent).toMatch(/Version 1\.5\.0 is installed/);
    expect(alerts().filter((a) => a.includes(RESTART_FAILED))).toHaveLength(1);
    expect(alerts().join("\n")).toContain("relaunch denied");
    await user.click(button(/Restart now/i));
    await flush();
    expect(commands(calls).filter((c) => c === "restart_app")).toHaveLength(2);
    expect(alerts().filter((a) => a.includes(RESTART_FAILED))).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Restart now/i })).toBeTruthy();
  });
});

describe("UpdateBanner failures", () => {
  it("stays quiet when the relaunch after the install fails, and says so when Restart now fails too", async () => {
    updater.pending = { version: "9.9.9", downloadAndInstall: () => Promise.resolve() };
    const calls = installTauri({ deploying_labs: () => [], restart_app: rejects({ code: "other", message: "relaunch denied" }) });
    const user = userEvent.setup();
    render(
      <>
        <UpdateBanner />
        <Toaster />
      </>,
    );
    await flush();
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    // The update is installed: a failed relaunch is not a failed update, so nothing is told yet.
    expect(screen.getByRole("status").textContent).toContain("Update installed");
    expect(commands(calls)).toContain("restart_app");
    expect(alerts()).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Restart now" }));
    await flush();
    expect(alerts().join("\n")).toContain(RESTART_FAILED);
    expect(alerts().join("\n")).toContain("relaunch denied");
    // The fallback stays for another try.
    expect(screen.getByRole("button", { name: "Restart now" })).toBeTruthy();
  });
});
