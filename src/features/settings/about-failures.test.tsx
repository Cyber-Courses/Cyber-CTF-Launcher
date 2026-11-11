// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/features/settings/about-section";
import { UpdateBanner } from "@/components/update-banner";
import { Toaster } from "@/components/ui/toaster";
import { toastsShown } from "@/components/test-support";
import { resetUpdater, updater } from "@/components/updater-double";
import { commands, installTauri, rejects, rejectWith } from "@/test/tauri";
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
      <AboutSection version="1.4.0" report={null} agent={null} updates="inApp" />
      <Toaster />
    </>,
  );
}

/** The update banner with the Toaster, its launch check settled. */
async function renderBanner() {
  render(
    <>
      <UpdateBanner />
      <Toaster />
    </>,
  );
  await flush();
}

/** In the About section: checks for updates, then installs what it offers. */
async function checkThenInstall(user: ReturnType<typeof userEvent.setup>) {
  await user.click(button(/Check for updates/i));
  await flush();
  await user.click(button(/Install update/i));
  await flush();
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
    const calls = installTauri({ busy_labs: () => [], active_operations: () => [], restart_app: rejects({ code: "other", message: "relaunch denied" }) });
    const user = userEvent.setup();
    renderAbout();
    await checkThenInstall(user);
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
  it("doesn't install while it can't tell whether a lab deploys, nor hide a failed install, and says so", async () => {
    const downloadAndInstall = vi.fn(() => Promise.reject(new Error("signature mismatch")));
    updater.pending = { version: "1.5.0", downloadAndInstall };
    let deploying: () => unknown = () => {
      rejectWith("worker list unreadable");
    };
    installTauri({ busy_labs: () => deploying(), active_operations: () => [] });
    const user = userEvent.setup();
    renderAbout();
    await checkThenInstall(user);
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(alerts().join("\n")).toContain("Couldn't tell whether a lab is starting or stopping");
    // A lab deploying: said, not silently skipped.
    deploying = () => ["web-sqli"];
    await user.click(button(/Install update/i));
    await flush();
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Wait until your labs finish starting, stopping or pausing");
    // Nothing deploying: the install runs, and its failure is told; it can be tried again.
    deploying = () => [];
    await user.click(button(/Install update/i));
    await flush();
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(alerts().join("\n")).toContain("Update failed");
    expect(alerts().join("\n")).toContain("signature mismatch");
    expect(button(/Install update/i)).toBeTruthy();
  });
});

describe("UpdateBanner failures", () => {
  it("doesn't install while it can't tell whether a lab deploys, and says so", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    updater.pending = { version: "9.9.9", downloadAndInstall };
    installTauri({ busy_labs: rejects({ code: "other", message: "worker list unreadable" }), active_operations: () => [] });
    const user = userEvent.setup();
    await renderBanner();
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(alerts().join("\n")).toContain("Couldn't tell whether a lab is starting or stopping");
    expect(screen.getAllByRole("status")[0].textContent).not.toContain("Update failed");
  });

  it("says the relaunch after the install failed, as Settings does, and again when Restart now fails too", async () => {
    updater.pending = { version: "9.9.9", downloadAndInstall: () => Promise.resolve() };
    const calls = installTauri({ busy_labs: () => [], active_operations: () => [], restart_app: rejects({ code: "other", message: "relaunch denied" }) });
    const user = userEvent.setup();
    await renderBanner();
    await user.click(screen.getByRole("button", { name: "Update" }));
    await flush();
    // The update is installed: a failed relaunch is not a failed update, but it is said, with the
    // one message Settings gives. Regression: the banner said nothing, Settings told it.
    expect(screen.getByRole("status").textContent).toContain("Update installed");
    expect(commands(calls)).toContain("restart_app");
    expect(alerts().join("\n")).toContain(RESTART_FAILED);
    await user.click(screen.getByRole("button", { name: "Restart now" }));
    await flush();
    expect(alerts().filter((a) => a.includes(RESTART_FAILED))).toHaveLength(2);
    expect(alerts().join("\n")).toContain("relaunch denied");
    // The fallback stays for another try.
    expect(screen.getByRole("button", { name: "Restart now" })).toBeTruthy();
  });
});
