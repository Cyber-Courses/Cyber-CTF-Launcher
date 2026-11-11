// @vitest-environment jsdom
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderList, ProviderRow, statusTone, type RowStatus } from "@/features/machine/provider-row";
import { RowTestButton, type RowProps } from "@/features/machine/lab-row-parts";
import { providerRows } from "@/features/machine/machine-test-kit";
import { readyReport } from "@/test/fixtures";

// One provider row of the Machine page: the same status words whichever provider is the default,
// each with its tooltip, and the columns (status, default slot, action) always there, filled or
// not, so a row reads the same shape in every state.

/** A row with what each test sets; the rest plain. */
function row(over: Partial<Parameters<typeof ProviderRow>[0]> = {}) {
  return (
    <ProviderRow
      kind="docker"
      icon={<span>logo</span>}
      title="OrbStack"
      status="ready"
      detail="not tested yet"
      actions={<button type="button">Test</button>}
      testing={false}
      onTestDone={() => {}}
      {...over}
    />
  );
}

/** The tooltip that explains the row's status. */
const statusTip = (item: HTMLElement) => within(item).getAllByRole("tooltip")[0].textContent;

const WORDS: Record<RowStatus, string> = {
  ready: "Ready",
  testFailed: "Test failed",
  noPermission: "No permission",
  stopped: "Stopped",
  needsSetup: "Needs setup",
  cantRunHere: "Can't run here",
};

const TIPS: Record<Exclude<RowStatus, "ready">, string> = {
  testFailed: "It looks ready, but its last self-test failed: run Test to see why.",
  noPermission: "Its engine is running, but your account may not use it yet.",
  stopped: "Installed, but its engine isn't running: start it to run labs on it.",
  needsSetup: "Something it needs isn't installed yet: Fix sets it up.",
  cantRunHere: "Installed, but this machine can't run it: the line under its name says why.",
};

describe("ProviderRow", () => {
  for (const [status, word] of Object.entries(WORDS) as [RowStatus, string][]) {
    it(`says ${word}, with a tooltip saying what it means`, () => {
      render(<ProviderList>{row({ status })}</ProviderList>);
      const item = screen.getByRole("listitem");
      expect(providerRows()[0].status).toBe(word);
      if (status !== "ready") expect(statusTip(item)).toBe(TIPS[status]);
      // Reachable from the keyboard, where its tooltip shows too.
      expect(item.querySelector('[data-slot="status"]')?.getAttribute("tabindex")).toBe("0");
    });
  }

  it("says what Ready means for an engine and for a hypervisor", () => {
    const { unmount } = render(row({ kind: "docker" }));
    expect(statusTip(screen.getByRole("listitem"))).toBe("Its engine is running and your account may use it: container labs can run on it.");
    unmount();
    render(row({ kind: "vm" }));
    expect(statusTip(screen.getByRole("listitem"))).toBe("Installed, with Vagrant and its plugin: VM labs can run on it.");
  });

  it("reads the same whether or not it is the default: only the default slot changes", async () => {
    const onUseDefault = vi.fn();
    const user = userEvent.setup();
    render(
      <ProviderList>
        {row({ title: "Docker Desktop", isDefault: true })}
        {row({ title: "OrbStack", onUseDefault })}
        {row({ title: "Colima", status: "stopped" })}
      </ProviderList>,
    );
    expect(providerRows()).toEqual([
      { name: "Docker Desktop", status: "Ready", slot: "Default", detail: "not tested yet" },
      { name: "OrbStack", status: "Ready", slot: "Use by default", detail: "not tested yet" },
      { name: "Colima", status: "Stopped", slot: "", detail: "not tested yet" },
    ]);
    // The Default mark says what it means.
    const [first] = screen.getAllByRole("listitem");
    expect(
      within(first)
        .getAllByRole("tooltip")
        .map((t) => t.textContent),
    ).toContain("Container labs run on this engine.");
    await user.click(screen.getByRole("button", { name: "Use by default" }));
    expect(onUseDefault).toHaveBeenCalledOnce();
  });

  it("always lays out the same five cells, filled or not, on the list's shared columns", () => {
    render(
      <ProviderList>
        {row({ isDefault: true })}
        {row({ onUseDefault: () => {} })}
        {row({ status: "needsSetup", actions: null })}
      </ProviderList>,
    );
    const list = screen.getByRole("list", { name: "Providers" });
    expect(list.className).toContain("grid-cols-[auto_minmax(0,1fr)_auto_auto_auto]");
    for (const item of within(list).getAllByRole("listitem")) {
      const cells = item.firstElementChild;
      expect(cells?.className).toContain("grid-cols-subgrid");
      expect(cells?.children).toHaveLength(5);
      // The default slot and the action cell are there even when empty, so nothing moves into them.
      expect(item.querySelector('[data-slot="default"]')).not.toBeNull();
      expect(item.querySelector('[data-slot="actions"]')).not.toBeNull();
    }
  });

  it("idles a stopped engine labs don't use, and warns when labs use it", () => {
    expect(statusTone("stopped", false)).toBe("muted");
    expect(statusTone("stopped", true)).toBe("warn");
    expect(statusTone("ready", false)).toBe("ok");
    expect(statusTone("testFailed", true)).toBe("fail");
  });
});

describe("a row's Test button", () => {
  const props = (testing: string | null, onToggleTest = vi.fn()) =>
    ({
      report: readyReport(),
      last: null,
      now: 0,
      memFree: null,
      testing,
      onToggleTest,
      onTestDone: vi.fn(),
      onFix: vi.fn(),
      onRefresh: vi.fn(),
    }) satisfies RowProps;

  for (const [testing, name, unseen] of [
    [null, "Test", "Hide test"],
    ["docker:orbstack", "Hide test", "Test"],
  ] as [string | null, string, string][]) {
    it(`reads "${name}" and keeps room for "${unseen}", so opening the test moves nothing`, async () => {
      const onToggleTest = vi.fn();
      const user = userEvent.setup();
      render(<RowTestButton row={props(testing, onToggleTest)} id="docker:orbstack" />);
      const button = screen.getByRole("button", { name });
      // The other label is only reserved (generated content), not part of the button's text.
      expect(button.textContent).toBe(name);
      expect(button.querySelector(".keeps-width")?.getAttribute("data-unseen")).toBe(unseen);
      await user.click(button);
      expect(onToggleTest).toHaveBeenCalledWith("docker:orbstack");
    });
  }
});
