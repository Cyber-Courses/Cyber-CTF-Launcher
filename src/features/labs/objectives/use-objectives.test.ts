// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { emptyProgress, loadObjectives, loadProgress, type Challenge, type LabProgress } from "@/features/labs/objectives/objectives";
import { objectivesState, useObjectives } from "@/features/labs/objectives/use-objectives";
import { deferred, type Deferred } from "@/test/deferred";
import { settle } from "@/test/ui";

vi.mock("@/features/labs/objectives/objectives", async (actual) => ({
  ...(await actual<typeof import("@/features/labs/objectives/objectives")>()),
  loadObjectives: vi.fn(() => Promise.resolve([])),
  loadProgress: vi.fn(),
  submitObjective: vi.fn(() => Promise.resolve({ correct: true, alreadySolved: false, labCompleted: false })),
}));

const main: Challenge = { key: "main", title: "Main", prompt: "?", hintCount: 0, optional: false, points: null, steps: [] };
const progress = emptyProgress([main]);

describe("objectivesState", () => {
  it("is loading until the challenges are read, and unavailable, with why, when they couldn't be", () => {
    expect(objectivesState(undefined, progress, true, true)).toEqual({ kind: "loading" });
    expect(objectivesState({ unread: "No connection" }, progress, true, true)).toEqual({ kind: "unavailable", why: "No connection" });
  });
  it("has nothing to show for a lab with neither objectives nor a question", () => {
    expect(objectivesState(null, progress, true, true)).toEqual({ kind: "none" });
  });
  it("carries the challenges, the progress, the sign-in and whether the progress is in", () => {
    expect(objectivesState([main], progress, false, false)).toEqual({ kind: "ready", challenges: [main], progress, signedIn: false, progressRead: false });
  });
});

describe("useObjectives: the challenges", () => {
  it("keeps why the objectives couldn't be read, in the player's words", async () => {
    vi.mocked(loadObjectives).mockRejectedValueOnce({ code: "other", message: "The Cyber CTF API answered 503" });
    vi.mocked(loadProgress).mockResolvedValue(null);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const { result } = renderHook(() => useObjectives("lab-x", "What is the flag?", "Objective", true));
    await settle(() => {});
    expect(result.current.state).toEqual({ kind: "unavailable", why: "The Cyber CTF API answered 503" });
  });
});

describe("useObjectives: the player's progress", () => {
  it("drops a refresh still out when the page moved to another lab", async () => {
    const reads: Deferred<LabProgress | null>[] = [];
    vi.mocked(loadProgress).mockImplementation(() => {
      const d = deferred<LabProgress | null>();
      reads.push(d);
      return d.promise;
    });
    const solved = (key: string): LabProgress => ({ ...progress, solved: [key] });
    const { result, rerender } = renderHook(({ id }) => useObjectives(id, "?", "Lab", true), { initialProps: { id: "lab-a" } });
    await settle(() => {
      reads[0].resolve(solved("a-first"));
    });
    // A correct answer on lab A reads its progress again; the page moves to lab B meanwhile.
    await act(async () => {
      await result.current.submit("main", "flag");
    });
    rerender({ id: "lab-b" });
    await settle(() => {
      reads[2].resolve(solved("b"));
    });
    await settle(() => {
      reads[1].resolve(solved("a-again"));
    });
    const state = result.current.state;
    expect(state.kind === "ready" && [state.progressRead, state.progress.solved]).toEqual([true, ["b"]]);
  });
});
