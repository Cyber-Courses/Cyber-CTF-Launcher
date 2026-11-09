// @vitest-environment jsdom
import { act, render, screen } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it } from "vitest";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The launch-confirm event's payload comes from another process: anything that isn't a request
// (a wrong shape, a missing field, a plain string) is dropped, and a proper one opens the dialog.

const request = {
  sessionId: "s-1",
  labName: "Web Shop",
  repository: "org/lab",
  commit: "abcdef1234567890",
  place: "cloud",
  target: "aws",
  privileged: false,
  expiresAtMs: Date.now() + 180_000,
};

/** Emits `launch-confirm` with `payload` and lets what it causes render. */
async function confirmAsked(payload: unknown) {
  await act(() => emit("launch-confirm", payload));
  await flush();
}

describe("LaunchConfirm", () => {
  it("drops a malformed payload and opens for a proper request", async () => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked("start it");
    await confirmAsked(null);
    await confirmAsked({ ...request, sessionId: 1 });
    await confirmAsked({ ...request, labName: undefined });
    await confirmAsked({ ...request, place: "mars" });
    await confirmAsked({ ...request, privileged: "yes" });
    await confirmAsked({ ...request, expiresAtMs: undefined });
    expect(screen.queryByRole("alertdialog")).toBeNull();

    await confirmAsked(request);
    const dialog = screen.getByRole("alertdialog");
    expect(dialog.textContent).toContain("Run this lab on your cloud account?");
    expect(dialog.textContent).toContain("repo: org/lab");
    expect(dialog.textContent).toContain("commit: abcdef123456");
  });

  it("keeps the open request when a malformed payload follows it", async () => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked(request);
    await confirmAsked({ nonsense: true });
    expect(screen.getByRole("alertdialog").textContent).toContain("repo: org/lab");
  });

  // Regression: only a launch onto a cloud account was asked about; one on this computer or a
  // server ran unattended. Each now names the lab and where it would run.
  it.each([
    ["here", "Run this lab on this computer?", "wants to start Web Shop on this computer.", "Run here"],
    ["server", "Run this lab on your server?", "wants to start Web Shop on homelab.", "Run on my server"],
    ["cloud", "Run this lab on your cloud account?", "wants to start Web Shop on homelab. This starts billable resources", "Run on my cloud"],
  ])("asks about a launch %s with the lab and where it runs", async (place, title, body, approve) => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked({ ...request, place, target: place === "here" ? "" : "homelab" });
    const dialog = screen.getByRole("alertdialog", { name: "Confirm a lab launch from the website" });
    expect(dialog.textContent).toContain(title);
    expect(dialog.textContent).toContain(body);
    expect(screen.getByRole("button", { name: approve })).toBeTruthy();
    expect(dialog.textContent).not.toContain("privileged");
  });

  it("says when the lab has privileged machines", async () => {
    installTauri();
    render(<LaunchConfirm />);
    await flush();
    await confirmAsked({ ...request, place: "here", target: "", privileged: true });
    expect(screen.getByRole("alertdialog").textContent).toContain(
      "This lab has privileged machines: they get kernel-level access to the computer that runs them.",
    );
  });
});
