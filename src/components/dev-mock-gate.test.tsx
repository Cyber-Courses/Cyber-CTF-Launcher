// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const installDevMock = vi.fn();
vi.mock("@/lib/dev-mock", () => ({ installDevMock: () => installDevMock() }));

/** A fresh copy of the gate: it decides on NODE_ENV, and installs the mock once, per module. */
async function gate() {
  vi.resetModules();
  return (await import("@/components/dev-mock-gate")).DevMockGate;
}

afterEach(() => {
  installDevMock.mockReset();
  vi.unstubAllEnvs();
  vi.restoreAllMocks();
});

describe("DevMockGate", () => {
  it("in development, renders its children once the mock is installed, installing it once", async () => {
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

  it("still renders its children when the mock fails to install", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    installDevMock.mockImplementation(() => {
      throw new Error("broken mock");
    });
    const DevMockGate = await gate();
    render(
      <DevMockGate>
        <p>child</p>
      </DevMockGate>,
    );
    expect(await screen.findByText("child")).toBeTruthy();
    expect(error).toHaveBeenCalledWith("dev mock failed to load", expect.any(Error));
  });

  it("does nothing once unmounted before the mock is in", async () => {
    const DevMockGate = await gate();
    const { unmount, container } = render(
      <DevMockGate>
        <p>child</p>
      </DevMockGate>,
    );
    unmount();
    await new Promise((ok) => setTimeout(ok, 20));
    expect(installDevMock).toHaveBeenCalledTimes(1);
    expect(container.innerHTML).toBe("");
  });

  it("in production, renders its children straight away and never loads the mock", async () => {
    vi.stubEnv("NODE_ENV", "production");
    const DevMockGate = await gate();
    render(
      <DevMockGate>
        <p>child</p>
      </DevMockGate>,
    );
    expect(screen.getByText("child")).toBeTruthy();
    await new Promise((ok) => setTimeout(ok, 20));
    expect(installDevMock).not.toHaveBeenCalled();
  });
});
