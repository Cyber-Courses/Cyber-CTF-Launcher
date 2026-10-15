// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Account, initials } from "@/features/account/account";
import { setLocalePreference } from "@/lib/i18n";
import { commands, installTauri, rejects, rejectWith, stream } from "@/test/tauri";
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
      auth_login: () =>
        new Promise(
          (r) =>
            (finish = () => {
              r({ loggedIn: true, name: "Ada", email: "ada@example.com" });
            }),
        ),
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

  it("while waiting, offers the sign-in link to copy and a Cancel that ends it quietly", async () => {
    let cancel: () => void = () => {};
    const calls = installTauri({
      auth_login: (args) => {
        stream(args, "https://auth.example/oauth2/authorize?state=s1");
        return new Promise<void>((resolve) => {
          cancel = resolve;
        }).then(() => rejectWith({ code: "signInCancelled", message: "Sign-in cancelled." }));
      },
      auth_login_cancel: () => {
        cancel();
      },
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<Account status={signedOut} onChange={onChange} />);
    await user.click(button(/^sign in$/i));
    await flush();
    expect(screen.getByText("Browser didn't open?")).toBeTruthy();
    await user.click(button(/copy the sign-in link/i));
    expect(writeText).toHaveBeenCalledWith("https://auth.example/oauth2/authorize?state=s1");
    await user.click(button(/^cancel$/i));
    await flush();
    expect(commands(calls)).toContain("auth_login_cancel");
    // Back to Sign in, with no error and no change of session.
    expect(button(/^sign in$/i).hasAttribute("disabled")).toBe(false);
    expect(screen.queryByText(/cancelled/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /^cancel$/i })).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("shows why the sign-in failed", async () => {
    installTauri({
      auth_login: () => rejectWith("browser closed"),
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Account status={signedOut} onChange={onChange} onSettings={vi.fn()} />);
    await user.click(button(/^sign in$/i));
    await flush();
    expect(screen.getByText("browser closed")).toBeTruthy();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("says a sign-in that timed out in the player's language", async () => {
    setLocalePreference("fr");
    installTauri({ auth_login: rejects({ code: "loginTimeout", args: { minutes: 5 }, message: "Sign-in wasn't finished within 5 minutes" }) });
    const user = userEvent.setup();
    render(<Account status={signedOut} onChange={vi.fn()} />);
    await user.click(button(/^se connecter$/i));
    await flush();
    expect(screen.getByText("La connexion n'a pas été terminée en 5 minutes, elle a donc été annulée. Réessayez.")).toBeTruthy();
    setLocalePreference("en");
  });

  it("names its icon-only buttons for screen readers, not by a tooltip alone", () => {
    render(<Account status={{ loggedIn: true, name: "Ada", email: null }} onChange={vi.fn()} onSettings={vi.fn()} />);
    expect(screen.getByTitle("Settings").getAttribute("aria-label")).toBe("Settings");
    expect(screen.getByTitle("Sign out").getAttribute("aria-label")).toBe("Sign out");
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
      auth_logout: () => rejectWith("keychain locked"),
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
