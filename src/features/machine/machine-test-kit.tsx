// The Machine page and the setup steps as their tests read them. Not part of the app: only tests
// import it.

import { render, screen, within } from "@testing-library/react";
import type userEvent from "@testing-library/user-event";
import { vi } from "vitest";
import { must } from "@/components/test-support";
import { MachineScreen } from "@/features/machine/machine-screen";
import { readyReport } from "@/test/fixtures";
import { flush } from "@/test/ui";

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

/** Presses Test on the first lab type of the Engines panel. */
export async function pressFirstTest(user: ReturnType<typeof userEvent.setup>) {
  const engines = must(screen.getByText("Engines").closest("section, div")?.parentElement, "the Engines panel");
  await user.click(within(engines).getAllByRole("button", { name: /^test$/i })[0]);
}
