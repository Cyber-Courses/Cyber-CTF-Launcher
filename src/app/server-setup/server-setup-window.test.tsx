// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { readyReport } from "@/test/fixtures";
import { installTauri } from "@/test/tauri";
import { button, flush } from "@/test/ui";

// The setup window opened from the Cloud page's "Connect" on the Azure row: the
// provider is chosen by that click, so the window skips "Choose a cloud provider" and opens on
// Azure's next step, with Azure fixed.

// The window's query string: a new cloud account, on the provider the row named.
vi.mock("next/navigation", () => ({ useSearchParams: () => new URLSearchParams("kind=cloud&provider=azure") }));
const close = vi.fn(() => Promise.resolve());
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close }) }));

const { default: ServerSetupWindow } = await import("@/app/server-setup/page");

describe("the setup window opened on a provider", () => {
  it("skips choosing the provider and opens on its tools, with Cancel first and Azure fixed", async () => {
    // The Azure CLI installed too (the sample machine has only the AWS one): the tools step lets the
    // setup go on.
    const report = readyReport();
    report.cloudClis.azure = { installed: true, version: "2.67.0" };
    installTauri({ system_check: () => report });
    const user = userEvent.setup();
    render(<ServerSetupWindow />);
    await flush(30);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Set up cloud provider");
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Command-line tools");
    expect(document.body.textContent).toContain("Step 1 of 4");
    expect(document.body.textContent).toContain("The Azure CLI and Terraform");
    expect(screen.queryByRole("radio")).toBeNull();
    expect(screen.queryByRole("button", { name: /back/i })).toBeNull();
    // Its next step is Azure's sign-in, not AWS's "How to connect".
    await user.click(button(/^continue$/i));
    await flush(30);
    expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Azure subscription");
    // Back from there returns to the tools, whose Cancel closes the window.
    await user.click(button(/^\s*back$/i));
    await user.click(button(/^cancel$/i));
    expect(close).toHaveBeenCalled();
  });
});
