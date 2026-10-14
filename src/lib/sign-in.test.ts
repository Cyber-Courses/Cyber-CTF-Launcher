// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { commands, installTauri, stream } from "@/test/tauri";
import { cancelSignIn, signIn, useSignIn } from "@/lib/sign-in";

const SIGN_IN_CANCELLED = { code: "signInCancelled", message: "Sign-in cancelled." };
import { act, renderHook } from "@testing-library/react";

const ada = { loggedIn: true, name: "Ada", email: "ada@example.com" };

/** An `auth_login` that gives its link, then waits for `finish` (a status, or an error to throw). */
function pendingLogin() {
  let finish: (outcome: unknown) => void = () => {};
  const handler = (args: Record<string, unknown> | undefined) => {
    stream(args, "https://auth.example/oauth2/authorize?state=s1");
    return new Promise(
      (resolve, reject) =>
        (finish = (o) => (o instanceof Error || typeof o === "string" || (typeof o === "object" && o !== null && "code" in o) ? reject(o) : resolve(o))),
    );
  };
  return { handler, finish: (o: unknown) => finish(o) };
}

describe("signIn", () => {
  it("waits with the sign-in link, and a second sign-in joins the first", async () => {
    const login = pendingLogin();
    const calls = installTauri({ auth_login: login.handler });
    const first = signIn();
    const second = signIn();
    expect(second).toBe(first);
    await Promise.resolve();
    const { result } = renderHook(() => useSignIn());
    expect(result.current).toEqual({ waiting: true, url: "https://auth.example/oauth2/authorize?state=s1", error: null });
    login.finish(ada);
    expect(await first).toEqual(ada);
    expect(commands(calls).filter((c) => c === "auth_login")).toHaveLength(1);
  });

  it("ends a cancelled sign-in quietly, as null", async () => {
    const login = pendingLogin();
    const calls = installTauri({ auth_login: login.handler, auth_login_cancel: () => login.finish(SIGN_IN_CANCELLED) });
    const signingIn = signIn();
    await cancelSignIn();
    expect(await signingIn).toBeNull();
    expect(commands(calls)).toContain("auth_login_cancel");
    const { result } = renderHook(() => useSignIn());
    expect(result.current).toEqual({ waiting: false, url: null, error: null });
  });

  it("keeps why a sign-in failed, until the next one", async () => {
    const login = pendingLogin();
    installTauri({ auth_login: login.handler });
    const signingIn = signIn();
    login.finish("Sign-in wasn't finished within 5 minutes, so it was cancelled. Try again.");
    await expect(signingIn).rejects.toBe("Sign-in wasn't finished within 5 minutes, so it was cancelled. Try again.");
    const { result } = renderHook(() => useSignIn());
    expect(result.current.error).toMatch(/within 5 minutes/);
    act(() => void signIn());
    expect(result.current.error).toBeNull();
  });
});
