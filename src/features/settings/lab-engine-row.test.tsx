// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LabEngineRow } from "@/features/settings/lab-engine-row";
import { installTauri, rejectWith } from "@/test/tauri";
import { flush } from "@/test/ui";

const text = () => document.body.textContent;

describe("LabEngineRow", () => {
  it("reads the saved engine, then saves and describes the one picked", async () => {
    const calls = installTauri({ lab_engine: () => "launcher", set_lab_engine: () => null });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<LabEngineRow onSaved={onSaved} />);
    await flush();
    expect(text()).toContain("Labs run through Cyber CTF's own code");
    await user.click(screen.getByRole("radio", { name: "Isothrum runner" }));
    await flush();
    expect(calls.find((c) => c.cmd === "set_lab_engine")?.args).toEqual({ engine: "isothrum" });
    expect(text()).toContain("Labs run through the Isothrum runner");
    expect(screen.getByRole("radio", { name: "Isothrum runner" }).getAttribute("aria-checked")).toBe("true");
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it("keeps the engine shown when saving fails", async () => {
    installTauri({ lab_engine: () => "isothrum", set_lab_engine: () => rejectWith("denied") });
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<LabEngineRow onSaved={onSaved} />);
    await flush();
    expect(screen.getByRole("radio", { name: "Isothrum runner" }).getAttribute("aria-checked")).toBe("true");
    await user.click(screen.getByRole("radio", { name: "Built-in" }));
    await flush();
    expect(screen.getByRole("radio", { name: "Isothrum runner" }).getAttribute("aria-checked")).toBe("true");
    expect(onSaved).not.toHaveBeenCalled();
  });
});
