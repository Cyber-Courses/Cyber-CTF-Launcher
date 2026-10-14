// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearOnboarded, markOnboarded, readOnboarded } from "@/components/shell/onboarded-flag";

describe("onboarded flag", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("is unset on a new machine, set once the onboarding is done", () => {
    localStorage.clear();
    expect(readOnboarded()).toBe(false);
    markOnboarded();
    expect(localStorage.getItem("cyberctf.onboarded")).toBe("1");
    expect(readOnboarded()).toBe(true);
    clearOnboarded();
    expect(readOnboarded()).toBe(false);
  });

  it("takes the onboarding as done without storage", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("no storage");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("no storage");
    });
    expect(readOnboarded()).toBe(true);
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("no storage");
    });
    expect(() => markOnboarded()).not.toThrow();
    expect(() => clearOnboarded()).not.toThrow();
  });
});
