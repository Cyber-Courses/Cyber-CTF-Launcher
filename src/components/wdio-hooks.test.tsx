// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const loaded = vi.fn<(hook: string) => void>();
vi.mock("@/components/wdio-launch", () => {
  loaded("launch");
  return { WdioLaunch: () => <p>launch hook</p> };
});
vi.mock("@/components/wdio-attack", () => {
  loaded("attack");
  return { WdioAttack: () => <p>attack hook</p> };
});

/** A fresh copy of the hooks: they decide on the build's environment once, per module. */
async function hooks() {
  vi.resetModules();
  return (await import("@/components/wdio-hooks")).WdioHooks;
}

afterEach(() => {
  loaded.mockReset();
  vi.unstubAllEnvs();
});

describe("WdioHooks", () => {
  it("mounts both e2e hooks in a development build", async () => {
    const WdioHooks = await hooks();
    render(<WdioHooks />);
    expect(await screen.findByText("launch hook")).toBeTruthy();
    expect(await screen.findByText("attack hook")).toBeTruthy();
  });

  it("leaves them out of a production build, never loading their code", async () => {
    // Regression: every build mounted them, so a release app started the lab folder a
    // localStorage key named.
    vi.stubEnv("NODE_ENV", "production");
    const WdioHooks = await hooks();
    const { container } = render(<WdioHooks />);
    await Promise.resolve();
    expect(container.innerHTML).toBe("");
    expect(loaded).not.toHaveBeenCalled();
  });

  it("mounts them in a production build made for the e2e (NEXT_PUBLIC_WDIO=1)", async () => {
    vi.stubEnv("NODE_ENV", "production");
    vi.stubEnv("NEXT_PUBLIC_WDIO", "1");
    const WdioHooks = await hooks();
    render(<WdioHooks />);
    expect(await screen.findByText("launch hook")).toBeTruthy();
  });
});
