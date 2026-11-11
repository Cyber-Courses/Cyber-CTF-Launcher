// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const installDevMock = vi.fn<() => void>();
vi.mock("@/lib/dev-mock", () => ({
  installDevMock: () => {
    installDevMock();
  },
}));

const APP_NOTICE =
  "This is the Cyber CTF desktop app's interface. It only runs inside the app (pnpm tauri dev). For the browser preview with sample data, run pnpm dev:web.";

/** A fresh copy of the gate: it decides on NODE_ENV and NEXT_PUBLIC_DEV_MOCK, and installs the
 *  mock once, per module. */
async function gate() {
  vi.resetModules();
  return (await import("@/components/dev-mock-gate")).DevMockGate;
}

/** Renders a fresh gate around a `child` paragraph. */
async function renderGate() {
  const DevMockGate = await gate();
  return render(
    <DevMockGate>
      <p>child</p>
    </DevMockGate>,
  );
}

/** The app's dev server shows the notice, and nothing else. */
async function expectAppNotice(container: HTMLElement) {
  await screen.findByRole("main");
  expect(container.textContent).toBe(APP_NOTICE);
}

/** The browser preview's build (`pnpm dev:web`). */
const previewBuild = () => vi.stubEnv("NEXT_PUBLIC_DEV_MOCK", "1");
const at = (url: string) => {
  window.history.replaceState(null, "", url);
};
const tick = () => new Promise((ok) => setTimeout(ok, 20));

beforeEach(() => {
  at("/?mock");
});

afterEach(() => {
  installDevMock.mockReset();
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  delete (window as { __CYBERCTF_MOCK__?: unknown }).__CYBERCTF_MOCK__;
});

/** What a web end-to-end's forced commands put in the page before it loads (tests/web/fixtures.ts):
 *  Tauri's internals, marked as the mock's so the mock installs over them. */
const mockStandIn = () => {
  Object.assign(window, { __TAURI_INTERNALS__: {}, __CYBERCTF_MOCK__: true });
};

describe("DevMockGate", () => {
  it("on the app's dev server, shows a browser the notice, ?mock or not, and never loads the mock", async () => {
    vi.stubEnv("NEXT_PUBLIC_DEV_MOCK", "");
    await expectAppNotice((await renderGate()).container);
    expect(screen.queryByText("child")).toBeNull();
    await tick();
    expect(installDevMock).not.toHaveBeenCalled();
  });

  it("inside the app's window, renders its children without the mock", async () => {
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    await renderGate();
    expect(await screen.findByText("child")).toBeTruthy();
    expect(installDevMock).not.toHaveBeenCalled();
  });

  it("in the browser preview with ?mock, renders its children once the mock is installed, installing it once", async () => {
    previewBuild();
    const DevMockGate = await gate();
    let installedFirst = false;
    installDevMock.mockImplementation(() => (installedFirst = !screen.queryByText("child")));
    render(
      <>
        <DevMockGate>
          <p>child</p>
        </DevMockGate>
        <DevMockGate>
          <p>other</p>
        </DevMockGate>
      </>,
    );
    expect(screen.queryByText("child")).toBeNull();
    expect(await screen.findByText("child")).toBeTruthy();
    expect(await screen.findByText("other")).toBeTruthy();
    expect(installedFirst).toBe(true);
    expect(installDevMock).toHaveBeenCalledTimes(1);
  });

  it("in the browser preview, installs the mock over internals marked as the mock's", async () => {
    previewBuild();
    mockStandIn();
    await renderGate();
    expect(await screen.findByText("child")).toBeTruthy();
    expect(installDevMock).toHaveBeenCalledTimes(1);
  });

  it("on the app's dev server, takes internals marked as the mock's for a browser's", async () => {
    vi.stubEnv("NEXT_PUBLIC_DEV_MOCK", "");
    mockStandIn();
    await expectAppNotice((await renderGate()).container);
    expect(installDevMock).not.toHaveBeenCalled();
  });

  it("in the browser preview without ?mock, points at it instead of installing the mock", async () => {
    previewBuild();
    at("/");
    await renderGate();
    expect((await screen.findByRole("link", { name: "?mock" })).getAttribute("href")).toBe("?mock");
    expect(screen.queryByText("child")).toBeNull();
    expect(installDevMock).not.toHaveBeenCalled();
  });

  it("still renders its children when the mock fails to install", async () => {
    previewBuild();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    installDevMock.mockImplementation(() => {
      throw new Error("broken mock");
    });
    await renderGate();
    expect(await screen.findByText("child")).toBeTruthy();
    expect(error).toHaveBeenCalledWith("dev mock failed to load", expect.any(Error));
  });

  it("does nothing once unmounted before the mock is in", async () => {
    previewBuild();
    const { unmount, container } = await renderGate();
    unmount();
    await tick();
    expect(installDevMock).toHaveBeenCalledTimes(1);
    expect(container.innerHTML).toBe("");
  });

  it("in production, renders its children straight away and never loads the mock, even built for the preview", async () => {
    vi.stubEnv("NODE_ENV", "production");
    previewBuild();
    await renderGate();
    expect(screen.getByText("child")).toBeTruthy();
    await tick();
    expect(installDevMock).not.toHaveBeenCalled();
  });
});
