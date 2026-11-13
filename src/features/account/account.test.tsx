// @vitest-environment jsdom
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Account, Avatar, initials } from "@/features/account/account";
import { Toaster } from "@/components/ui/toaster";
import { setLocalePreference } from "@/lib/i18n";
import { commands, installTauri, rejects, rejectWith, stream } from "@/test/tauri";
import { button, flush } from "@/test/ui";

const signedOut = { loggedIn: false, name: null, email: null };

/** An `auth_login` that gives its link, then waits until `cancel` ends it as cancelled. */
function cancellableLogin() {
  let cancel: () => void = () => {};
  const handler = (args: Record<string, unknown> | undefined) => {
    stream(args, "https://auth.example/oauth2/authorize?state=s1");
    return new Promise<void>((resolve) => {
      cancel = resolve;
    }).then(() => rejectWith({ code: "signInCancelled", message: "Sign-in cancelled." }));
  };
  return {
    handler,
    cancel: () => {
      cancel();
    },
  };
}

/** Renders the signed-out account with a clipboard to copy into, and presses Sign in. */
async function startSignIn() {
  const onChange = vi.fn();
  const user = userEvent.setup();
  const writeText = vi.fn(() => Promise.resolve());
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  render(<Account status={signedOut} onChange={onChange} />);
  await user.click(button(/^sign in$/i));
  await flush();
  return { onChange, user, writeText };
}

describe("initials", () => {
  it("takes the first letters of a name, else of the email", () => {
    expect(initials("Ada Lovelace", null)).toBe("FA");
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
    render(
      <>
        <Account status={signedOut} onChange={onChange} />
        <Toaster />
      </>,
    );
    await user.click(button(/^sign in$/i));
    // The wait takes the button's place, from the first moment (the link isn't in yet: no way out to show).
    expect(screen.getByText("Continue in your browser")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^sign in$/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /cancel/i })).toBeNull();
    finish();
    await flush();
    expect(commands(calls)).toContain("auth_login");
    expect(onChange).toHaveBeenCalledWith({ loggedIn: true, name: "Ada", email: "ada@example.com" });
    // Said once, wherever the sign-in was started from.
    expect(screen.getByText("Signed in as Ada")).toBeTruthy();
  });

  it("while waiting, offers the sign-in link to copy and a Cancel that ends it quietly", async () => {
    const login = cancellableLogin();
    const calls = installTauri({ auth_login: login.handler, auth_login_cancel: login.cancel });
    const { onChange, user, writeText } = await startSignIn();
    expect(screen.getByText("Continue in your browser")).toBeTruthy();
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

  it("switches to a code entered on another device when the browser can't be used", async () => {
    let approve: () => void = () => {};
    const login = cancellableLogin();
    const calls = installTauri({
      auth_login: login.handler,
      auth_login_device: (args) => {
        // The core ends the browser sign-in waiting as soon as one with a code starts.
        login.cancel();
        stream(args, {
          userCode: "WDJB-MJHT",
          verificationUri: "https://auth.example/device",
          verificationUriComplete: "https://auth.example/device?user_code=WDJB-MJHT",
          expiresIn: 900,
        });
        return new Promise((resolve) => {
          approve = () => {
            resolve({ loggedIn: true, name: "Ada", email: "ada@example.com" });
          };
        });
      },
    });
    const { onChange, user, writeText } = await startSignIn();
    await user.click(button(/use a code instead/i));
    await flush();
    expect(screen.getByText("Enter it at https://auth.example/device on any device.")).toBeTruthy();
    expect(screen.getByText("WDJB-MJHT")).toBeTruthy();
    expect(screen.getByText("Sign in with a code")).toBeTruthy();
    expect(screen.getByText("Expires in 15:00")).toBeTruthy();
    await user.click(button(/copy the link with the code/i));
    expect(writeText).toHaveBeenCalledWith("https://auth.example/device?user_code=WDJB-MJHT");
    approve();
    await flush();
    expect(commands(calls)).toEqual(expect.arrayContaining(["auth_login", "auth_login_device"]));
    // The sidebar's sign-in reports the session the code brought.
    expect(onChange).toHaveBeenCalledWith({ loggedIn: true, name: "Ada", email: "ada@example.com" });
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

  it("says why the player is signed out when it wasn't their doing", () => {
    const { rerender } = render(<Account status={{ ...signedOut, sessionExpired: true }} onChange={vi.fn()} />);
    expect(screen.getByText("Your session expired. Sign in again to continue.")).toBeTruthy();
    rerender(<Account status={{ ...signedOut, keychainError: "the keychain is locked" }} onChange={vi.fn()} />);
    expect(screen.getByText("the keychain is locked")).toBeTruthy();
    expect(screen.queryByText(/session expired/)).toBeNull();
    rerender(<Account status={signedOut} onChange={vi.fn()} />);
    expect(screen.queryByText(/session expired|keychain/)).toBeNull();
  });

  it("shows the account's picture over the initials, and the initials alone when it can't load", () => {
    const ada = { loggedIn: true, name: "Ada Lovelace", email: null, picture: "https://cdn.example/ada.png" };
    const { container } = render(<Account status={ada} onChange={vi.fn()} />);
    const img = container.querySelector("img");
    if (!img) throw new Error("no picture shown");
    expect(img.getAttribute("src")).toBe("https://cdn.example/ada.png");
    expect(screen.getByText("AL")).toBeTruthy();
    fireEvent.error(img);
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("AL")).toBeTruthy();
    // No picture: initials only, no empty image.
    const { container: plain } = render(<Avatar status={{ name: null, email: "ada@example.com" }} />);
    expect(plain.querySelector("img")).toBeNull();
    expect(plain.textContent).toBe("AD");
  });

  it("says when the code ran out", async () => {
    const login = cancellableLogin();
    installTauri({
      auth_login: login.handler,
      auth_login_device: (args) => {
        login.cancel();
        stream(args, { userCode: "AAAA-BBBB", verificationUri: "https://auth.example/device", verificationUriComplete: null, expiresIn: 0 });
        return new Promise(() => {});
      },
    });
    const { user } = await startSignIn();
    await user.click(button(/use a code instead/i));
    // The countdown ticks once a second.
    await flush(1100);
    expect(screen.getByText("The code expired. Cancel and start again.")).toBeTruthy();
  });

  it("names a player with no name as signed in", () => {
    render(<Account status={{ loggedIn: true, name: null, email: "ada@example.com" }} onChange={vi.fn()} />);
    expect(screen.getByText("Signed in")).toBeTruthy();
    expect(screen.getByText("ada@example.com")).toBeTruthy();
    expect(screen.queryByTitle(/Online/)).toBeNull();
  });

  it("says a sign-out the keychain refused, and shows the session it kept, not a sign-out", async () => {
    // Regression: a keychain that kept the session read as signed out, until the next start
    // signed the player in again.
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const kept = { loggedIn: true, name: "Ada", email: "ada@example.com" };
    const calls = installTauri({
      auth_logout: () => rejectWith("keychain locked"),
      auth_status: () => kept,
    });
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Account status={kept} onChange={onChange} />);
    await user.click(screen.getByTitle("Sign out"));
    await flush();
    expect(commands(calls)).toEqual(expect.arrayContaining(["auth_logout", "auth_status"]));
    expect(onChange).toHaveBeenCalledWith(kept);
    expect(onChange).not.toHaveBeenCalledWith(signedOut);
    expect(warn).toHaveBeenCalledWith("Couldn’t sign out:", "keychain locked");
    warn.mockRestore();
  });

  it("keeps the session shown when a failed sign-out can't read the one it kept, and logs why", async () => {
    // Regression: the read's rejection escaped the sign-out unhandled.
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    installTauri({ auth_logout: () => rejectWith("keychain locked"), auth_status: () => rejectWith("keychain locked") });
    const kept = { loggedIn: true, name: "Ada", email: "ada@example.com" };
    const onChange = vi.fn();
    render(<Account status={kept} onChange={onChange} />);
    await userEvent.setup().click(screen.getByTitle("Sign out"));
    await flush();
    await new Promise((r) => setTimeout(r, 0));
    process.off("unhandledRejection", unhandled);
    expect(onChange).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledWith("reading the session a failed sign-out kept:", "keychain locked");
    expect(unhandled).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});
