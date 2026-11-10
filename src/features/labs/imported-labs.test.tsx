// @vitest-environment jsdom
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { importedLab, isImportedId } from "@/features/labs/imported-labs";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { containerMachine, labStatus, upStatus } from "@/features/labs/labs-test-kit";
import type { ImportedLab, LabStatus } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// A lab imported from a folder has no catalogue entry: its page shows what the folder's `.ctf/`
// defines (read by the core from the installed copy), and nothing where it defines nothing.

const METADATA = {
  title: "Supplier portal API",
  description: "Globex Finance gives suppliers a small read-only API to check their invoices.",
  category: "web",
  difficulty: 1,
  capabilities: ["rest-api-read-http", "http-request-headers"],
};
const OBJECTIVES: ImportedLab["objectives"] = [
  {
    key: "main",
    title: "Read the flag",
    prompt: "Find the GenericAll edge and read the flag.",
    optional: false,
    points: null,
    hintCount: 3,
    steps: [{ key: "ace", title: "Name the ACE", prompt: "Which right does hodor hold?", hintCount: 0 }],
  },
  { key: "bonus", title: "Bonus round", prompt: null, optional: true, points: 50, hintCount: 0, steps: [] },
];

const imported = (p: Partial<ImportedLab> = {}): ImportedLab => ({
  id: "local-invoice-portal-api",
  name: "invoice-portal-api",
  metadata: null,
  objectives: null,
  problems: [],
  ...p,
});

const running: LabStatus = upStatus(
  [containerMachine("api", "running", { ports: [{ published: 8080, target: 8080 }], services: [{ name: "api", kind: "web", ports: [8080] }] })],
  { url: "http://127.0.0.1:8080" },
);

/** Not running here: never started, or stopped. */
const stopped: LabStatus = labStatus();

/** The status strip under the page's title (its state, where, runtime, level, category). */
const statusStrip = () => screen.getAllByText(/^(Running|Not started)$/)[0].closest("div")?.textContent ?? "";

/** The page of `lab` in `status`, once its first reads are in; the commands it invoked. */
async function openPage(lab: ImportedLab, status: LabStatus = running) {
  const calls = installTauri({ lab_status: () => status, deploying_labs: () => [], active_operations: () => [] });
  renderLabPage(importedLab(lab), { status, loggedIn: false });
  await flush(40);
  return calls;
}

/** The GraphQL operations the page asked the backend. */
const backendOps = (calls: Awaited<ReturnType<typeof openPage>>) =>
  calls.filter((c) => c.cmd === "api_query").map((c) => (c.args as { operation?: string } | undefined)?.operation);

describe("an imported lab's page", () => {
  it("knows an imported lab by its id", () => {
    expect(isImportedId("local-invoice-portal-api")).toBe(true);
    expect(isImportedId("66d71cbf-9b4e-4a78-b3cf-8208227a96b2")).toBe(false);
  });

  it("shows the title, description, level, category, skills and read-only objectives of a full .ctf folder", async () => {
    const calls = await openPage(imported({ metadata: METADATA, objectives: OBJECTIVES }));
    expect(screen.getByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
    expect(screen.getByText(METADATA.description)).toBeTruthy();
    expect(statusStrip()).toMatch(/Running.*Easy.*web/);
    expect(screen.getByText("rest-api-read-http")).toBeTruthy();
    // The challenges as the file lists them, with their steps, to read only.
    expect(screen.getByText("Objectives")).toBeTruthy();
    expect(screen.getByText("2 objectives")).toBeTruthy();
    expect(screen.getByText("Read the flag")).toBeTruthy();
    expect(screen.getByText("Find the GenericAll edge and read the flag.")).toBeTruthy();
    expect(screen.getByText("Name the ACE")).toBeTruthy();
    expect(screen.getByText("Bonus round")).toBeTruthy();
    expect(screen.getByText(/Progress is tracked for published labs/)).toBeTruthy();
    // No answer field, no hint to open, no sign-in prompt: nothing is checked or saved.
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByRole("button", { name: /hint|steps|submit|sign in/i })).toBeNull();
    // Nothing of it asked of the backend, which doesn't know the lab: objectives, progress, brief.
    expect(backendOps(calls)).toEqual([]);
    expect(screen.queryByText("Brief")).toBeNull();
  });

  it("has no objectives section when the .ctf folder has only metadata.json", async () => {
    await openPage(imported({ metadata: METADATA }));
    expect(screen.getByRole("heading", { name: "Supplier portal API" })).toBeTruthy();
    expect(screen.getByText(METADATA.description)).toBeTruthy();
    expect(screen.queryByText("Objectives")).toBeNull();
    expect(screen.queryByText(/Progress is tracked for published labs/)).toBeNull();
  });

  it("is a minimal page without a .ctf folder: name, status, deployment, network, attack box, actions", async () => {
    const calls = await openPage(imported());
    // Its spec's name, nothing the folder doesn't define.
    expect(screen.getByRole("heading", { name: "invoice-portal-api" })).toBeTruthy();
    expect(screen.queryByText(/Easy|Medium|Hard/)).toBeNull();
    expect(screen.queryByText("Objectives")).toBeNull();
    expect(screen.queryByText("Brief")).toBeNull();
    // No category, and no empty slot for it in the status strip: no separator ends it.
    expect(statusStrip()).not.toMatch(/web/);
    expect(statusStrip().trim().endsWith("·")).toBe(false);
    expect(statusStrip()).not.toMatch(/·\s*·/);
    // What it does have.
    expect(screen.getAllByText("Running").length).toBeGreaterThan(0);
    expect(screen.getByText("Deployment")).toBeTruthy();
    // The diagram, its code loaded on first use (slow under a full parallel run).
    expect((await screen.findAllByText(/10\.42\.0\.3/, {}, { timeout: 10_000 })).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Attack box").length).toBeGreaterThan(0);
    expect(screen.getByText("Runs on")).toBeTruthy();
    expect(screen.getByRole("button", { name: /open in browser/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /stop & remove/i })).toBeTruthy();
    expect(backendOps(calls)).toEqual([]);
    expect(commands(calls)).toContain("attack_box_status");
  });

  it("offers to import the folder again, not a catalogue start, once it is stopped", async () => {
    await openPage(imported({ metadata: METADATA }), stopped);
    // Signed out too: an imported lab needs no sign-in.
    expect(screen.queryByRole("button", { name: /start lab|sign in/i })).toBeNull();
    const again = screen.getByRole("button", { name: /import a lab/i });
    expect(again.getAttribute("title")).toMatch(/import the folder again/);
  });

  it("says which .ctf file can't be read, and why, and shows the rest", async () => {
    await openPage(imported({ objectives: OBJECTIVES, problems: [{ file: "metadata.json", error: "missing field `title` at line 3 column 1" }] }));
    expect(screen.getByText("metadata.json can't be read: missing field `title` at line 3 column 1")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "invoice-portal-api" })).toBeTruthy();
    expect(screen.getByText("Read the flag")).toBeTruthy();
  });
});
