// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { SignInPopover } from "@/app/screen/sign-in";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";
import { expectMaskedPassword } from "@/test/secret";

// The attack box's screen window: its toolbar's Sign-in opens a popover with the account and its
// password, masked until Show and copied either way; no Sign-in for any other machine.

const PASSWORD = "k7mpq-zrt4w-hn8xc-ls2vd";

describe("the screen window's sign-in", () => {
  it("opens on the box's account and its masked password, shown and copied on request", async () => {
    installTauri({ screen_sign_in: () => ({ user: "player", password: PASSWORD }) });
    const user = userEvent.setup();
    render(<SignInPopover />);
    await flush();
    const open = screen.getByRole("button", { name: /Sign-in/ });
    expect(open.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("dialog")).toBeNull();
    await user.click(open);
    const dialog = screen.getByRole("dialog", { name: "Sign in to the attack box" });
    expect(dialog.textContent).toContain("sign in as player with this password");
    expect(screen.getByText("player")).toBeTruthy();
    await expectMaskedPassword(user, PASSWORD);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("offers nothing on another machine's screen", async () => {
    const calls = installTauri({ screen_sign_in: () => null });
    render(<SignInPopover />);
    await flush();
    expect(calls.map((c) => c.cmd)).toContain("screen_sign_in");
    expect(screen.queryByRole("button", { name: /Sign-in/ })).toBeNull();
  });
});
