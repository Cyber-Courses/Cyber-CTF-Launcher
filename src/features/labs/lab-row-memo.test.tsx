// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabRow } from "@/features/labs/lab-row";
import type { Lab } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";

// LabRow is memoized (a list passes every row the same handlers, which take the lab), and its
// title is the stretched button that opens it. A poll that changes nothing about a row must not
// re-render it: count the renders of one of its cells.
const cells = vi.hoisted(() => ({ title: 0 }));
vi.mock("@/features/labs/lab-row-parts", async (orig) => {
  const mod = await orig<typeof import("@/features/labs/lab-row-parts")>();
  return {
    ...mod,
    RowTitle: (p: Parameters<typeof mod.RowTitle>[0]) => {
      cells.title++;
      return mod.RowTitle(p);
    },
  };
});

const lab: Lab = {
  id: "lab-invoice",
  slug: "invoice-portal-api",
  title: "Invoice portal API",
  description: "A billing portal",
  question: null,
  difficulty: 2,
  category: "Web",
  runtime: { runtime: "DOCKER", architectures: ["x86_64"], providers: [] },
  skills: [],
};
const status = {
  running: true,
  parked: null,
  machines: [],
  networks: [],
  url: "http://127.0.0.1:8080",
  host: null,
  place: "container",
} as unknown as LabStatus;

describe("LabRow memo", () => {
  it("skips an idle tick with the same props and handlers, and opens with the lab from its title", async () => {
    installTauri();
    const user = userEvent.setup();
    const h = { onOpen: vi.fn(), onStop: vi.fn(), onResume: vi.fn() };
    const row = (s: LabStatus) => <LabRow lab={lab} status={s} loggedIn hostArch="x86_64" {...h} />;
    const { rerender } = render(row(status));
    const first = cells.title;
    expect(first).toBeGreaterThan(0);

    // An idle poll: the parent renders again with equal props and the same handlers.
    rerender(row(status));
    rerender(row(status));
    expect(cells.title).toBe(first);

    // Something about this lab changed: it renders again.
    rerender(row({ ...status, url: null }));
    expect(cells.title).toBeGreaterThan(first);

    await user.click(screen.getByRole("button", { name: "Open Invoice portal API" }));
    expect(h.onOpen).toHaveBeenCalledWith(lab);
  });
});
