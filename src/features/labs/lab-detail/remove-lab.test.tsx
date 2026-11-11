// @vitest-environment jsdom
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { importedLab } from "@/features/labs/imported-labs";
import { renderLabPage } from "@/features/labs/lab-detail/lab-detail-test-kit";
import { containerMachine, labStatus, testLab, upStatus } from "@/features/labs/labs-test-kit";
import { getLastRun, setLastRun } from "@/features/labs/runs/last-run";
import type { LabStatus } from "@/lib/tauri";
import { TOAST_EVENT, type Toast } from "@/lib/toast";
import { answersDialog, commands, dialogsAsked, installTauri, rejects, type Handler } from "@/test/tauri";
import { flush } from "@/test/ui";

// Remove lab, on a lab's page: the app's copy of the lab and what it keeps for it go, once asked,
// never while the lab is up; the player lands back on the list, told.

const supplier = importedLab({
  id: "local-supplier-portal",
  name: "supplier-portal",
  metadata: { title: "Supplier portal", description: null, category: null, difficulty: null, capabilities: [] },
  objectives: null,
  problems: [],
});

const running: LabStatus = upStatus([containerMachine("api")], { url: "http://127.0.0.1:8080" });

/** The toasts shown meanwhile. */
const toasts: Toast[] = [];
const onToast = (e: Event) => toasts.push((e as CustomEvent<Toast>).detail);
window.addEventListener(TOAST_EVENT, onToast);
afterEach(() => {
  toasts.length = 0;
});

/** The page of `lab` in `status`, its first reads in, with `overrides` of the core's answers. */
async function openPage(lab = supplier, status: LabStatus = labStatus(), overrides: Partial<Record<string, Handler>> = {}) {
  const calls = installTauri({ lab_status: () => status, busy_labs: () => [], active_operations: () => [], lab_remove: () => ({ left: [] }), ...overrides });
  const page = renderLabPage(lab, { status, loggedIn: true });
  await flush(40);
  return { calls, page };
}

const removeButton = () => screen.getByRole("button", { name: /^remove lab$/i });

describe("Remove lab", () => {
  it("is offered for a stopped imported lab, asks first naming the lab and keeping its folder, then lands back on the list", async () => {
    setLastRun(supplier.id);
    const { calls, page } = await openPage();
    expect(screen.getByText("On this computer")).toBeTruthy();
    expect(screen.getByText(/the folder you imported it from is kept/)).toBeTruthy();
    expect((removeButton() as HTMLButtonElement).disabled).toBe(false);

    await userEvent.click(removeButton());
    await flush();

    const [asked] = dialogsAsked(calls);
    expect(asked.title).toBe("Remove Supplier portal from this computer?");
    expect(asked.message).toMatch(/deletes its copy of the lab.*run records, generated files and deploy logs/);
    expect(asked.message).toMatch(/The folder you imported it from is kept as it is\./);
    expect(calls.find((c) => c.cmd === "plugin:dialog|message")?.args?.buttons).toEqual({ OkCancelCustom: ["Remove lab", "Cancel"] });
    expect(calls.filter((c) => c.cmd === "lab_remove").map((c) => c.args)).toEqual([{ id: "local-supplier-portal" }]);
    // Back on the list, told; the imported labs read again (its row goes), its recency forgotten.
    expect(page.onBack).toHaveBeenCalledTimes(1);
    expect(toasts.map((t) => [t.title, t.body, t.tone])).toEqual([["Supplier portal removed", "The folder you imported it from is untouched.", "ok"]]);
    expect(commands(calls)).toContain("imported_labs");
    expect(getLastRun(supplier.id)).toBeNull();
    // An imported lab is installed by definition: not asked.
    expect(commands(calls)).not.toContain("lab_installed");
  });

  it("does nothing on Cancel", async () => {
    const { calls, page } = await openPage(supplier, labStatus(), { "plugin:dialog|message": answersDialog(false) });
    await userEvent.click(removeButton());
    await flush();
    expect(dialogsAsked(calls)).toHaveLength(1);
    expect(commands(calls)).not.toContain("lab_remove");
    expect(page.onBack).not.toHaveBeenCalled();
  });

  it("is disabled with 'Stop the lab first' while the lab runs, is parked, deploys or left machines", async () => {
    const cases: [string, LabStatus, Partial<Record<string, Handler>>][] = [
      ["running", running, {}],
      ["parked", labStatus({ parked: "shutdown", machines: [containerMachine("api", "exited")] }), {}],
      ["interrupted", labStatus({ machines: [containerMachine("api", "exited")] }), {}],
      [
        "deploying",
        labStatus(),
        {
          busy_labs: () => [supplier.id],
          active_operations: () => [{ labId: supplier.id, op: "launch", machine: null, deploying: true, step: null }],
        },
      ],
    ];
    for (const [what, status, overrides] of cases) {
      const { calls, page } = await openPage(supplier, status, overrides);
      const button = removeButton() as HTMLButtonElement;
      expect(button.disabled, what).toBe(true);
      expect(
        screen.getAllByRole("tooltip").map((t) => t.textContent),
        what,
      ).toContain("Stop the lab first");
      await userEvent.click(button);
      expect(dialogsAsked(calls), what).toEqual([]);
      expect(commands(calls), what).not.toContain("lab_remove");
      page.unmount();
    }
  });

  it("says a partial removal in a dialog naming what stayed, and still lands back on the list", async () => {
    const left = [{ path: "/data/runs/cyberctf-local-supplier-portal", error: "Resource busy (os error 16)" }];
    const { calls, page } = await openPage(supplier, labStatus(), { lab_remove: () => ({ left }) });
    await userEvent.click(removeButton());
    await flush();
    const [, said] = dialogsAsked(calls);
    expect(said.title).toBe("Supplier portal removed, but some files stayed");
    expect(said.message).toMatch(/delete them by hand:\n\n\/data\/runs\/cyberctf-local-supplier-portal \(Resource busy \(os error 16\)\)$/);
    expect(toasts).toEqual([]);
    expect(page.onBack).toHaveBeenCalledTimes(1);
  });

  it("stays on the page and says why when the core refuses", async () => {
    const { page } = await openPage(supplier, labStatus(), {
      lab_remove: rejects({
        code: "labRunningHere",
        message: "This lab is still running on this machine (or its machines are still there). Stop it, then try again.",
      }),
    });
    document.hasFocus = () => true;
    await userEvent.click(removeButton());
    await flush();
    expect(page.onBack).not.toHaveBeenCalled();
    expect(toasts.map((t) => [t.title, t.tone])).toEqual([["Couldn't remove the lab", "fail"]]);
    expect(toasts[0].body).toMatch(/still running on this machine/);
    expect((removeButton() as HTMLButtonElement).disabled).toBe(false);
  });

  it("is offered for a catalogue lab once installed, saying the account keeps its progress", async () => {
    const lab = testLab("lab-s3", { title: "Leaky bucket" });
    const { calls, page } = await openPage(lab, labStatus(), { lab_installed: () => true });
    expect(calls.filter((c) => c.cmd === "lab_installed").map((c) => c.args)).toContainEqual({ id: "lab-s3" });
    expect(screen.getByText(/keeps this lab's files here since its first start/)).toBeTruthy();
    await userEvent.click(removeButton());
    await flush();
    const [asked] = dialogsAsked(calls);
    expect(asked.title).toBe("Remove Leaky bucket from this computer?");
    expect(asked.message).toMatch(/Your progress stays in your Cyber CTF account/);
    expect(page.onBack).toHaveBeenCalledTimes(1);
    expect(toasts.map((t) => [t.title, t.body])).toEqual([["Leaky bucket removed", undefined]]);
  });

  it("isn't offered for a catalogue lab that was never installed here", async () => {
    await openPage(testLab("lab-s3", { title: "Leaky bucket" }), labStatus(), { lab_installed: () => false });
    expect(screen.queryByText("On this computer")).toBeNull();
    expect(screen.queryByRole("button", { name: /^remove lab$/i })).toBeNull();
  });
});
