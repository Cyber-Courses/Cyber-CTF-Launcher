// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { readSession, writeSession } from "@/lib/local-store";

describe("a window's session storage", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    sessionStorage.clear();
  });

  it("keeps a value for the session, and reads as none where storage refuses", () => {
    expect(readSession("cyberctf.test")).toBeNull();
    writeSession("cyberctf.test", "1");
    expect(readSession("cyberctf.test")).toBe("1");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(readSession("cyberctf.test")).toBeNull();
    expect(() => {
      writeSession("cyberctf.test", "2");
    }).not.toThrow();
  });
});
