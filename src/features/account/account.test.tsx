// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Account, initials } from "@/features/account/account";
import { commands, installTauri } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const signedOut = { loggedIn: false, name: null, email: null };

describe("initials", () => {
  it("takes the first letters of a name, else of the email", () => {
    expect(initials("Florian Amette", null)).toBe("FA");
    expect(initials("florian", null)).toBe("FL");
    expect(initials(null, "player@example.com")).toBe("PL");
    expect(initials(null, null)).toBe("?");
    expect(initials("  ", null)).toBe("?");
  });
});

describe("Account", () => {
  it("renders nothing until the auth status is known", () => {
    const { container } = render(<Account status={null} onChange={vi.fn()} />);
    expect(container.innerHTML).toBe("");
  });

  it("signs in through the browser, busy meanwhile", async () => {
    let finish: () => void = () => {};
    const calls = installTauri({
      auth_login: () => new Promise((r) => (finish = () => r({ loggedIn: true, name: "Ada", email: "ada@example.com" }))),
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Account status={signedOut} onChange={onChange} />);
    await user.click(button(/^sign in$/i));
    expect(screen.getByText("Waiting for the browser…")).toBeTruthy();
    expect(button(/waiting/i).hasAttribute("disabled")).toBe(true);
    finish();
    await flush();
    expect(commands(calls)).toContain("auth_login");
    expect(onChange).toHaveBeenCalledWith({ loggedIn: true, name: "Ada", email: "ada@example.com" });
  });

  it("shows why the sign-in failed", async () => {
    installTauri({
      auth_login: () => {
        throw "browser closed";
      },
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Account status={signedOut} onChange={onChange} onSettings={vi.fn()} />);
    await user.click(button(/^sign in$/i));
    await flush();
    expect(screen.getByText("browser closed")).toBeTruthy();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("opens Settings signed out and signed in", async () => {
    installTauri();
    const onSettings = vi.fn();
    const user = userEvent.setup();
    const { rerender } = render(<Account status={signedOut} onChange={vi.fn()} onSettings={onSettings} />);
    await user.click(button(/settings/i));
    rerender(<Account status={{ loggedIn: true, name: "Ada Lovelace", email: null }} onChange={vi.fn()} onSettings={onSettings} online />);
    await user.click(screen.getByTitle("Settings"));
    expect(onSettings).toHaveBeenCalledTimes(2);
    expect(screen.getByText("AL")).toBeTruthy();
    expect(screen.getByText("signed in · keychain")).toBeTruthy();
    expect(screen.getByTitle(/Online/)).toBeTruthy();
  });

  it("names a player with no name as signed in", () => {
    render(<Account status={{ loggedIn: true, name: null, email: "ada@example.com" }} onChange={vi.fn()} />);
    expect(screen.getByText("Signed in")).toBeTruthy();
    expect(screen.getByText("ada@example.com")).toBeTruthy();
    expect(screen.queryByTitle(/Online/)).toBeNull();
  });

  it("signs out locally even when the backend sign-out fails, without an unhandled rejection", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const calls = installTauri({
      auth_logout: () => {
        throw "keychain locked";
      },
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Account status={{ loggedIn: true, name: "Ada", email: "ada@example.com" }} onChange={onChange} />);
    await user.click(screen.getByTitle("Sign out"));
    await flush();
    expect(commands(calls)).toContain("auth_logout");
    expect(onChange).toHaveBeenCalledWith(signedOut);
    expect(warn).toHaveBeenCalledWith("sign out:", "keychain locked");
    warn.mockRestore();
  });
});
