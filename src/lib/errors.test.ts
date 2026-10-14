import { afterEach, describe, expect, it } from "vitest";
import { commandError, describeError, errorCode, isSignedOut, type CommandError } from "@/lib/errors";
import { setLocalePreference } from "@/lib/i18n";
import en from "@/messages/en";

const signedOut: CommandError = { code: "signedOut", message: "You're signed out. Sign in again to continue." };
const port: CommandError = { code: "portInUse", args: { port: 8080 }, message: "Host port 8080 is already in use" };

describe("command errors", () => {
  afterEach(() => setLocalePreference("en"));

  it("are read from what a command rejects with, and nothing else", () => {
    expect(commandError(port)).toBe(port);
    expect(errorCode(port)).toBe("portInUse");
    for (const other of [null, undefined, "boom", new Error("boom"), { message: "no code" }, { code: 1, message: "x" }]) expect(errorCode(other)).toBeNull();
  });

  it("say a known code in the player's language, with its values", () => {
    setLocalePreference("fr");
    expect(describeError(signedOut)).toBe("Vous êtes déconnecté. Reconnectez-vous pour continuer.");
    expect(describeError(port)).toContain("Le port 8080");
    setLocalePreference("ja");
    expect(describeError(port)).toContain("8080");
    expect(describeError(port)).not.toContain("already in use");
  });

  it("keep the core's own message for an error it has no code for", () => {
    setLocalePreference("de");
    expect(describeError({ code: "other", message: "lab `x` is not installed" })).toBe("lab `x` is not installed");
  });

  it("say a JS error by its message, without the 'Error: ' String() adds", () => {
    expect(describeError(new Error("Pick a cloud account"))).toBe("Pick a cloud account");
    expect(describeError("plain text")).toBe("plain text");
  });

  it("tell a signed-out error by its code, not its words", () => {
    expect(isSignedOut(signedOut)).toBe(true);
    expect(isSignedOut({ code: "sessionExpired", message: "" })).toBe(true);
    expect(isSignedOut({ code: "other", message: "You're signed out." })).toBe(false);
    expect(isSignedOut("You're signed out.")).toBe(false);
  });

  it("have an English message for every code the core sends", () => {
    // The generated union lists every code; `describeError` type-checks one key per code, and
    // the catalog tests make every language carry the same keys.
    expect(Object.keys(en.errors.codes).sort()).toEqual(
      [
        "alreadyInProgress",
        "commandFailed",
        "dockerDenied",
        "dockerDeniedWindows",
        "dockerNotRunning",
        "loginTimeout",
        "noBrowser",
        "noPasswordStored",
        "offlineReady",
        "portInUse",
        "sessionExpired",
        "signInCancelled",
        "signedOut",
        "toolMissing",
        "unreachable",
      ].sort(),
    );
  });
});
