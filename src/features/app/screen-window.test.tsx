// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ScreenConnection } from "@/lib/tauri";
import { deferred } from "@/test/deferred";
import { commands, installTauri, rejectWith } from "@/test/tauri";
import { flush } from "@/test/ui";

// A lab machine's screen window in each of its states: connecting, showing (VNC), a Remote
// Desktop, not available (and why), and a connection that fails or is lost.

const query = vi.hoisted(() => ({ value: "id=goad&runtime=VM&machine=dc01" }));
vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams(query.value) }));

// noVNC draws on a canvas jsdom lacks: a stand-in recording how it was made, its events fired
// by the test.
type Made = { url: string; password?: string; protocols?: string[]; viewer: EventTarget; disconnected: boolean };
const made = vi.hoisted(() => [] as Made[]);
vi.mock("@/vendor/novnc/core/rfb.js", () => ({
  default: class extends EventTarget {
    scaleViewport = false;
    resizeSession = true;
    focusOnClick = false;
    entry: Made;
    constructor(_: HTMLElement, url: string, options: { credentials?: { password: string }; wsProtocols?: string[] }) {
      super();
      this.entry = { url, password: options.credentials?.password, protocols: options.wsProtocols, viewer: this, disconnected: false };
      made.push(this.entry);
    }
    disconnect() {
      this.entry.disconnected = true;
    }
    sendCredentials() {}
  },
}));

const { default: ScreenWindow } = await import("@/app/screen/page");
const { SHOW_WITHIN_MS } = await import("@/app/screen/use-screen");

const VNC: ScreenConnection = {
  protocol: "vnc",
  url: "ws://127.0.0.1:41234/0123456789abcdef",
  password: "pw-vnc",
  address: null,
  user: null,
  hasPassword: false,
  client: null,
  note: null,
  tunnelled: false,
};

const RDP: ScreenConnection = {
  protocol: "rdp",
  url: null,
  password: null,
  address: "127.0.0.1:24001",
  user: "isoloom",
  hasPassword: true,
  client: "Windows App",
  note: null,
  tunnelled: true,
};

const fire = (m: Made, type: string, detail?: unknown) => m.viewer.dispatchEvent(new CustomEvent(type, { detail }));

describe("the screen window", () => {
  it("connects its viewer to the bridge's one-time URL with the password, and says when it shows", async () => {
    made.length = 0;
    const answer = deferred<ScreenConnection>();
    installTauri({ screen_connect: () => answer.promise });
    render(<ScreenWindow />);
    expect(await screen.findByText("Connecting to the machine's screen…")).toBeTruthy();
    answer.resolve(VNC);
    await flush();
    expect(made).toHaveLength(1);
    expect(made[0]).toMatchObject({ url: VNC.url, password: "pw-vnc", protocols: ["binary"] });
    fire(made[0], "connect");
    await flush();
    expect(screen.getByText("dc01 · screen")).toBeTruthy();
    expect(screen.queryByText("Connecting to the machine's screen…")).toBeNull();
  });

  it("says a lost connection, and Try again asks the core for a fresh one", async () => {
    made.length = 0;
    const calls = installTauri({ screen_connect: () => VNC });
    const user = userEvent.setup();
    render(<ScreenWindow />);
    await flush();
    fire(made[0], "connect");
    fire(made[0], "disconnect", { clean: false });
    await flush();
    expect(screen.getByText("The connection to the machine's screen was lost.")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Try again/ }));
    await flush();
    expect(commands(calls).filter((c) => c === "screen_connect")).toHaveLength(2);
    expect(made[0].disconnected).toBe(true);
    expect(made).toHaveLength(2);
  });

  it("says where the screen didn't answer when the bridge closes before it ever shows", async () => {
    made.length = 0;
    installTauri({ screen_connect: () => ({ ...VNC, address: "127.0.0.1:5903" }) });
    render(<ScreenWindow />);
    await flush();
    fire(made[0], "disconnect", { clean: false });
    await flush();
    expect(screen.getByText("The VM's screen didn't answer at 127.0.0.1:5903.")).toBeTruthy();
    expect(screen.getByText("dc01 · connection lost")).toBeTruthy();
  });

  it("never spins for good: a screen that doesn't show in time says so and stops connecting", async () => {
    made.length = 0;
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      installTauri({ screen_connect: () => ({ ...VNC, address: "127.0.0.1:5901" }) });
      render(<ScreenWindow />);
      await flush();
      expect(screen.getByText("Connecting to the machine's screen…")).toBeTruthy();
      vi.advanceTimersByTime(SHOW_WITHIN_MS);
      await flush();
      expect(screen.getByText("The VM's screen didn't answer at 127.0.0.1:5901.")).toBeTruthy();
      expect(screen.queryByText("Connecting to the machine's screen…")).toBeNull();
      expect(made[0].disconnected).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("says why a machine has no screen", async () => {
    installTauri({
      screen_connect: () => rejectWith({ code: "other", message: "machine `dc01` has no graphical console: UTM shows a VM's screen over SPICE only" }),
    });
    render(<ScreenWindow />);
    await flush();
    expect(screen.getByText("This machine has no screen to show")).toBeTruthy();
    expect(screen.getByText(/UTM shows a VM's screen over SPICE only/)).toBeTruthy();
    expect(screen.getByText("dc01 · no screen")).toBeTruthy();
  });

  it("says a refused password as a failure", async () => {
    made.length = 0;
    installTauri({ screen_connect: () => VNC });
    render(<ScreenWindow />);
    await flush();
    fire(made[0], "securityfailure");
    await flush();
    expect(screen.getByText("The machine's screen refused the password it was given.")).toBeTruthy();
  });

  it("opens a Remote Desktop in the system's client and copies its password only on request", async () => {
    const calls = installTauri({ screen_connect: () => RDP, screen_open_rdp: () => null, screen_password: () => "pw-rdp" });
    const user = userEvent.setup();
    const written: string[] = [];
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: (t: string) => (written.push(t), Promise.resolve()) } });
    render(<ScreenWindow />);
    await flush();
    expect(screen.getByText("This machine's screen opens in Remote Desktop")).toBeTruthy();
    expect(screen.getByText("127.0.0.1:24001 · isoloom")).toBeTruthy();
    expect(commands(calls)).not.toContain("screen_password");
    await user.click(screen.getByRole("button", { name: /Open in Windows App/ }));
    await user.click(screen.getByRole("button", { name: /Copy password/ }));
    await flush();
    expect(commands(calls)).toEqual(expect.arrayContaining(["screen_open_rdp", "screen_password"]));
    expect(written).toEqual(["pw-rdp"]);
  });

  it("says what to install when there is no Remote Desktop app", async () => {
    installTauri({ screen_connect: () => ({ ...RDP, client: null }) });
    render(<ScreenWindow />);
    await flush();
    expect(screen.getByText(/No Remote Desktop app is installed/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Open in/ })).toBeNull();
  });
});
