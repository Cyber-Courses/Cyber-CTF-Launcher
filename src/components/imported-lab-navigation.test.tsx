// @vitest-environment jsdom
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { openMainWindow } from "@/components/shell-test-kit";
import { containerMachine, upStatus } from "@/features/labs/labs-test-kit";
import type { ImportedLab } from "@/lib/tauri";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

vi.mock("@tauri-apps/plugin-updater", () => import("@/components/updater-double"));

// A lab imported from a folder opens its page from wherever a catalogue lab's opens: the sidebar's
// "Running", the Labs list, the Overview. The catalogue doesn't list it; its `.ctf/` folder names it.

const ID = "local-invoice-portal-api";
const invoice: ImportedLab = {
  id: ID,
  name: "invoice-portal-api",
  metadata: { title: "Supplier portal API", description: "Look an invoice up.", category: "web", difficulty: 1, capabilities: [] },
  objectives: null,
  problems: [],
};
const running = upStatus([containerMachine("api")]);

/** The core with `imported()` installed here and `local-invoice-portal-api` up. */
const installImported = (imported: () => ImportedLab[]) =>
  installTauri({
    imported_labs: imported,
    machine_workloads: () => [{ id: ID, kind: "docker", count: 1, memBytes: 0, provider: null }],
    running_labs: () => [ID],
    lab_status: (a) => (a?.id === ID ? running : upStatus([], { running: false })),
    deploying_labs: () => [],
    active_operations: () => [],
  });

/** The main window with `imported` installed here and `local-invoice-portal-api` up. */
async function withImported(imported: ImportedLab[]) {
  installImported(() => imported);
  await openMainWindow(120);
}

const sidebar = () => screen.getByRole("navigation");

describe("an imported lab in the sidebar", () => {
  it("opens its page, titled by its .ctf folder", async () => {
    const user = userEvent.setup();
    await withImported([invoice]);
    await user.click(await within(sidebar()).findByRole("button", { name: /Supplier portal API/ }));
    await flush(80);
    expect(await screen.findByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
    expect(screen.getByText("Look an invoice up.")).toBeTruthy();
  });

  it("opens its page by its id while the imported labs aren't read yet, once they are", async () => {
    const user = userEvent.setup();
    let installed: ImportedLab[] = [];
    await withImported([]);
    // Named by its id: no list has it yet.
    const entry = await within(sidebar()).findByRole("button", { name: /Local Invoice Portal Api/ });
    installed = [invoice];
    installImported(() => installed);
    await user.click(entry);
    await flush(80);
    expect(await screen.findByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
  });
});

describe("an imported lab in the lists", () => {
  it("has its own panel on the Labs screen, whose row opens its page", async () => {
    const user = userEvent.setup();
    await withImported([invoice]);
    await user.click(within(sidebar()).getByRole("button", { name: /^Labs/ }));
    await flush(80);
    expect(await screen.findByText("Imported from a folder")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Open Supplier portal API" }));
    await flush(80);
    expect(await screen.findByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
  });

  it("is a row of the Overview's running labs that opens its page", async () => {
    const user = userEvent.setup();
    await withImported([invoice]);
    await user.click(await screen.findByRole("button", { name: "Open Supplier portal API" }));
    await flush(80);
    expect(await screen.findByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
  });
});
