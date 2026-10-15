// The app shell's windows and dialogs as the shell tests open them. Not part of the app: only
// tests import it.

import { render, screen, waitFor } from "@testing-library/react";
import { expect } from "vitest";
import { AppShell } from "@/components/app-shell";
import { emitSettled } from "@/components/test-support";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { QuitGuard } from "@/features/app/quit-guard";
import { flush } from "@/test/ui";

/** The main window, past the onboarding, once its first reads have settled (`ms`). */
export async function openMainWindow(ms = 50) {
  localStorage.setItem("cyberctf.onboarded", "1");
  render(<AppShell />);
  await screen.findByRole("navigation");
  await flush(ms);
}

/** The quit guard, listening, after a quit was blocked by `count` labs at work. */
export async function blockQuit(count: unknown = 1) {
  render(<QuitGuard />);
  await flush();
  await emitSettled("quit-blocked", count);
}

/** The cloud launch confirmation, listening, after the agent asked for `request`. */
export async function askLaunch(request: unknown) {
  render(<LaunchConfirm />);
  await flush();
  await emitSettled("launch-confirm", request);
}

/** The command palette once its search field has the focus. */
export async function focusedPalette() {
  const dialog = screen.getByRole("dialog");
  await waitFor(() => {
    expect(dialog.contains(document.activeElement)).toBe(true);
  });
  return dialog;
}
