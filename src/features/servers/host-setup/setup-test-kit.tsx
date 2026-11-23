// The server / cloud setup window as its tests drive it. Not part of the app: only tests import it.

import { render, screen } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { must } from "@/components/test-support";
import { HostSetupPage } from "@/features/servers/host-setup";
import type { ServerHostInput, SystemReport } from "@/lib/tauri";
import { readyReport } from "@/test/fixtures";
import { rejects, type Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

export type User = ReturnType<typeof userEvent.setup>;

/** The arguments of each call to `cmd`, in order. */
export const argsOf = (calls: Call[], cmd: string) => calls.filter((c) => c.cmd === cmd).map((c) => c.args);

/** The step's title. */
export const heading = () => screen.getByRole("heading", { level: 2 }).textContent;

/** The window's title. */
export const title = () => screen.getByRole("heading", { level: 1 }).textContent;

/** The step's drop-downs, in order. */
export const selects = () => screen.queryAllByRole<HTMLSelectElement>("combobox");

/** The text field showing `placeholder`. */
export const field = (placeholder: string) => screen.getByPlaceholderText<HTMLInputElement>(placeholder);

/** The input matching `selector` (the password field, say); the test fails when there is none. */
export const input = (selector: string) => must(document.querySelector<HTMLInputElement>(selector), selector);

/** A command that fails with `message`, as the Rust core's `other` error. */
export const failsWith = (message: string) => rejects({ code: "other", message });

/** The setup window on `initial`, with `report` as this machine's tools; its callbacks, to check. */
export async function renderSetup(initial: ServerHostInput, report: SystemReport | null = readyReport()) {
  const handlers = { onRefresh: vi.fn(), onSaved: vi.fn(), onDone: vi.fn() };
  render(<HostSetupPage initial={initial} report={report} {...handlers} />);
  await flush(30);
  return handlers;
}

/** Presses the button named `name`, then lets the step settle. */
export async function go(user: User, name: RegExp) {
  await user.click(button(name));
  await flush(30);
}
