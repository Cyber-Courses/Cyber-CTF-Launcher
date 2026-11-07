// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The setup window opened from the Cloud page's "Connect" on the Microsoft Azure row: its first
// step has Azure picked, not AWS.

// The window's query string: a new cloud account, on the provider the row named.
vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams("kind=cloud&provider=azure") }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: () => Promise.resolve() }) }));

const { default: ServerSetupWindow } = await import("@/app/server-setup/page");

describe("the setup window", () => {
  it("opens on the cloud provider its query names, picked at the first step", async () => {
    installTauri();
    render(<ServerSetupWindow />);
    await flush(30);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Set up cloud provider");
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Choose a cloud provider");
    const picked = screen.getAllByRole("radio").filter((r) => r.getAttribute("aria-checked") === "true");
    expect(picked).toHaveLength(1);
    expect(picked[0].textContent).toContain("Microsoft Azure");
    expect(picked[0].textContent).not.toContain("Amazon");
  });
});
