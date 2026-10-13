import { describe, expect, it } from "vitest";
import { emptyProgress, type Challenge } from "@/features/labs/objectives/objectives";
import { objectivesState } from "@/features/labs/objectives/use-objectives";

const main: Challenge = { key: "main", title: "Main", prompt: "?", hintCount: 0, optional: false, points: null, steps: [] };
const progress = emptyProgress([main]);

describe("objectivesState", () => {
  it("is loading until the challenges are read, and unavailable without the objectives API", () => {
    expect(objectivesState(undefined, progress, true, true)).toEqual({ kind: "loading" });
    expect(objectivesState("unavailable", progress, true, true)).toEqual({ kind: "unavailable" });
  });
  it("has nothing to show for a lab with neither objectives nor a question", () => {
    expect(objectivesState(null, progress, true, true)).toEqual({ kind: "none" });
  });
  it("carries the challenges, the progress, the sign-in and whether the progress is in", () => {
    expect(objectivesState([main], progress, false, false)).toEqual({ kind: "ready", challenges: [main], progress, signedIn: false, progressRead: false });
  });
});
