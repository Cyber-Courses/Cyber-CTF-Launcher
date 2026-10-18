// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useHostedLabs, type HostedSession, type HostedState } from "@/features/hosted/use-hosted-labs";
import { must } from "@/components/test-support";
import { installTauri, type Call } from "@/test/tauri";

const session = (state: HostedState, over: Partial<HostedSession> = {}): HostedSession => ({
  id: "s1",
  labId: "lab-sqli",
  target: "hosted",
  state,
  endpoints: state === "RUNNING" ? [{ port: 80, url: "https://s1.labs.cyberctf.org" }] : [],
  message: null,
  expiresAt: "2026-10-11T12:00:00Z",
  ...over,
});

type Api = (operation: string, variables: Record<string, unknown> | undefined) => unknown;
/** Answers `api_query` with `api`, by the operation's name. */
const withApi = (api: Api): Call[] =>
  installTauri({ api_query: (a) => api(typeof a?.operation === "string" ? a.operation : "", a?.variables as Record<string, unknown> | undefined) });
const queries = (calls: Call[], word: string) => calls.filter((c) => c.cmd === "api_query" && String(c.args?.operation).includes(word));

/** Lets the mocked invokes resolve and React apply what they set, `ms` of fake time later. */
const advance = (ms = 0) =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("useHostedLabs", () => {
  it("picks up the player's running hosted session on mount", async () => {
    withApi((q) => (q.includes("activeSession") ? { myActiveLabSession: session("RUNNING") } : {}));
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    expect(result.current.session?.state).toBe("RUNNING");
  });

  it.each([
    ["a session on another target", session("RUNNING", { target: "agent-1" })],
    ["a failed session", session("FAILED")],
    ["an expired session", session("EXPIRED")],
    ["no session", null],
  ])("ignores %s on mount", async (_, s) => {
    withApi(() => ({ myActiveLabSession: s }));
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    expect(result.current.session).toBeNull();
  });

  it("stays empty when the session can't be read (signed out)", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    withApi(() => {
      throw new Error("You're signed out");
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    expect(result.current.session).toBeNull();
    expect(warn).toHaveBeenCalled();
  });

  it("launches, polls until running, then stops polling", async () => {
    const states: HostedState[] = ["CLAIMED", "PULLING", "RUNNING"];
    const calls = withApi((q, v) => {
      if (q.includes("launchHosted")) {
        expect(v).toEqual({ labId: "lab-sqli" });
        return { requestLabLaunch: session("REQUESTED") };
      }
      if (q.includes("labSession")) return { labSession: session(states.shift() ?? "RUNNING") };
      return { myActiveLabSession: null };
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    let launching: Promise<void> = Promise.resolve();
    act(() => {
      launching = result.current.launch("lab-sqli");
    });
    expect(result.current.busyLab).toBe("lab-sqli");
    await act(() => launching);
    expect(result.current.busyLab).toBeNull();
    expect(result.current.session?.state).toBe("REQUESTED");

    await advance(2500);
    expect(result.current.session?.state).toBe("CLAIMED");
    await advance(2500);
    expect(result.current.session?.state).toBe("PULLING");
    await advance(2500);
    expect(result.current.session?.state).toBe("RUNNING");
    expect(result.current.session?.endpoints[0].url).toBe("https://s1.labs.cyberctf.org");
    // Settled: no more polls.
    const polled = queries(calls, "labSession").length;
    await advance(10_000);
    expect(queries(calls, "labSession")).toHaveLength(polled);
  });

  it("keeps polling after a failed or empty poll", async () => {
    // A single failed poll used to stop the polling for good: the session sat at PULLING.
    const answers: (() => unknown)[] = [
      () => {
        throw new Error("network down");
      },
      () => ({ labSession: null }),
      () => ({ labSession: session("RUNNING") }),
    ];
    withApi((q) => {
      if (q.includes("activeSession")) return { myActiveLabSession: session("PULLING") };
      return must(answers.shift(), "a poll answer")();
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    expect(result.current.session?.state).toBe("PULLING");
    await advance(2500);
    await advance(2500);
    expect(result.current.session?.state).toBe("PULLING");
    await advance(2500);
    expect(result.current.session?.state).toBe("RUNNING");
  });

  it("gives up on a launch that never comes up, saying so", async () => {
    withApi((q) => {
      if (q.includes("activeSession")) return { myActiveLabSession: session("PULLING") };
      return { labSession: session("PULLING") };
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    await advance(11 * 60 * 1000);
    expect(result.current.error).toMatch(/taking/i);
  });

  it("says why a launch failed", async () => {
    withApi((q) => {
      if (q.includes("launchHosted")) throw new Error("No capacity left");
      return { myActiveLabSession: null };
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    await act(() => result.current.launch("lab-sqli"));
    expect(result.current.error).toContain("No capacity left");
    expect(result.current.busyLab).toBeNull();
    expect(result.current.session).toBeNull();
  });

  it("stops the session at once, and says when the stop fails", async () => {
    const calls = withApi((q) => {
      if (q.includes("stopSession")) throw new Error("backend down");
      return { myActiveLabSession: session("RUNNING") };
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    await act(() => result.current.stop());
    expect(result.current.session).toBeNull();
    expect(queries(calls, "stopSession")[0].args?.variables).toEqual({ id: "s1" });
    expect(result.current.error).toContain("backend down");
  });

  it("does not bring back a stopped session with a poll already in flight", async () => {
    let answer: (v: unknown) => void = () => {};
    withApi((q) => {
      if (q.includes("activeSession")) return { myActiveLabSession: session("PULLING") };
      if (q.includes("labSession")) return new Promise((r) => (answer = r));
      return { stopLabSession: { id: "s1", state: "STOPPED" } };
    });
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    await advance(2500);
    await act(() => result.current.stop());
    answer({ labSession: session("RUNNING") });
    await advance();
    expect(result.current.session).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it("does nothing on stop without a session", async () => {
    const calls = withApi(() => ({ myActiveLabSession: null }));
    const { result } = renderHook(() => useHostedLabs());
    await advance();
    await act(() => result.current.stop());
    expect(queries(calls, "stopSession")).toHaveLength(0);
  });
});
