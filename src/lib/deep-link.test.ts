import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-deep-link", () => ({ getCurrent: () => Promise.resolve(null), onOpenUrl: () => Promise.resolve(() => {}) }));

const { firstLabSlug, takeLaunchLink } = await import("@/lib/deep-link");

describe("deep links", () => {
  it("the launch link is read once per session", () => {
    // Regression: getCurrent() keeps returning the launch URL, so each visit to Labs pulled
    // the player back into that lab.
    expect(takeLaunchLink()).toBe(true);
    expect(takeLaunchLink()).toBe(false);
    expect(takeLaunchLink()).toBe(false);
  });

  it("finds the first lab link in a batch and ignores others", () => {
    expect(firstLabSlug(["https://example.com", "cyberctf://labs/invoice-portal-api/", "cyberctf://labs/goad"])).toBe("invoice-portal-api");
    expect(firstLabSlug(["cyberctf://login?code=x"])).toBeNull();
    expect(firstLabSlug(null)).toBeNull();
  });
});
