// The Machine page and the setup steps as their tests read them. Not part of the app: only tests
// import it.

import { render, screen, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { expect, vi } from "vitest";
import { must } from "@/components/test-support";
import { MachineScreen } from "@/features/machine/machine-screen";
import { readyReport } from "@/test/fixtures";
import type { Call } from "@/test/tauri";
import { button, flush } from "@/test/ui";

/** The choice card (a radio) whose text includes `label`. */
export const radioWith = (label: string) =>
  must(
    screen.getAllByRole("radio").find((r) => r.textContent.includes(label)),
    `a choice showing ${label}`,
  );

/** The Machine page on a ready machine, once its first reads have settled. */
export async function renderMachineScreen() {
  render(<MachineScreen report={readyReport()} onRefresh={vi.fn()} onNavigate={vi.fn()} />);
  await flush(50);
}

/** Presses Test on the first provider of the Providers panel (the engine labs use). */
export async function pressFirstTest(user: ReturnType<typeof userEvent.setup>) {
  const engines = must(screen.getByText("Providers").closest("section, div")?.parentElement, "the Providers panel");
  await user.click(within(engines).getAllByRole("button", { name: /^test$/i })[0]);
}

/** Presses "Install Docker Desktop": the launcher asks for Docker's install (`calls` records it). */
export async function installDockerDesktop(user: ReturnType<typeof userEvent.setup>, calls: Call[]) {
  await user.click(button(/install docker desktop/i));
  await flush();
  expect(calls.find((c) => c.cmd === "install_dependency")?.args).toMatchObject({ dependency: "docker" });
}
