// @vitest-environment jsdom
import { act, render, screen, within } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { AppShell } from "@/components/app-shell";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

describe("AppShell", () => {
  it("renders the shell and walks every screen", async () => {
    localStorage.setItem("cyberctf.onboarded", "1");
    installTauri();
    const user = userEvent.setup();
    render(<AppShell />);
    const nav = await screen.findByRole("navigation");
    for (const name of ["Labs", "Events", "Machine", "Servers", "Cloud", "Overview"]) {
      await user.click(within(nav).getAllByRole("button", { name: new RegExp(name, "i") })[0]);
      await act(async () => {
        await new Promise((r) => setTimeout(r, 50));
      });
    }
    expect(document.body.textContent).toBeTruthy();
  });

  it("shows a cloud launch asked from the website while the onboarding is up", async () => {
    // First run: no onboarded flag.
    installTauri();
    render(<AppShell />);
    await screen.findAllByText(/welcome/i);
    await act(() => emit("launch-confirm", { sessionId: "sess-1", repository: "cyber-courses/lab-s3", commit: "0123456789abcdef", target: "AWS sandbox" }));
    await flush();
    expect(screen.getByRole("alertdialog").textContent).toContain("AWS sandbox");
  });
});
