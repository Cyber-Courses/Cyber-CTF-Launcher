import { describe, expect, it } from "vitest";
import { mainButton } from "./lib/support.mjs";

// The specs' shared selectors (lib/support.mjs), as WebdriverIO gets them.

describe("a main-pane button by its labels", () => {
  it("is the first match, so a page showing two of the labels never trips WebdriverIO's strict selectors", () => {
    const asked: string[] = [];
    const b = {
      $: (selector: string) => {
        asked.push(selector);
        return selector;
      },
    };
    mainButton(b, "Stop & remove", "Stop & clean up");
    expect(asked).toEqual(['(//main//button[normalize-space()="Stop & remove" or normalize-space()="Stop & clean up"])[1]']);
  });
});
