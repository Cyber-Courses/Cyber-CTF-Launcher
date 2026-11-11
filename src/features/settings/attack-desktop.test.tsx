// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { getAttackDesktop, setAttackDesktop } from "@/lib/settings";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// The attack VM's desktop choice: headless by default, the desktop's sizes said, the choice kept.

describe("the attack VM desktop setting", () => {
  it("is headless by default, says what the desktop takes, and keeps the choice", async () => {
    setAttackDesktop(false);
    installTauri();
    const onSaved = vi.fn();
    const user = userEvent.setup();
    render(<AttackBoxRows onSaved={onSaved} />);
    await flush();
    expect(getAttackDesktop()).toBe(false);
    expect(screen.getByText(/about 1 GB more to download, 3 GB more on its disk, and 4 GB of memory and 2 CPUs/)).toBeTruthy();
    expect(screen.getByText(/A container box is always headless/)).toBeTruthy();
    await user.click(screen.getByText("Desktop"));
    expect(getAttackDesktop()).toBe(true);
    expect(onSaved).toHaveBeenCalled();
    await user.click(screen.getByText("Headless"));
    expect(getAttackDesktop()).toBe(false);
  });
});
