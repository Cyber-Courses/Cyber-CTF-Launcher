// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const logError = vi.fn<(message: string) => Promise<void>>(async () => {});
const logWarn = vi.fn<(message: string) => Promise<void>>(async () => {});
vi.mock("@tauri-apps/plugin-log", () => ({ error: (message: string) => logError(message), warn: (message: string) => logWarn(message) }));
vi.mock("@tauri-apps/api/path", () => ({ appLogDir: async () => "/logs/org.cyberctf.desktop" }));

const { describeError, installErrorLogging, logFolder, logFrontendError } = await import("@/lib/error-log");
const { tell, warn } = await import("@/lib/failure");

describe("describeError", () => {
  it("keeps an error's stack", () => {
    const err = new Error("boom");
    expect(describeError(err)).toContain("boom");
    expect(describeError(err)).toContain("error-log.test");
  });

  it("describes what isn't an Error", () => {
    expect(describeError("plain")).toBe("plain");
    expect(describeError({ code: 7 })).toBe('{"code":7}');
    expect(describeError(undefined)).toBe("undefined");
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    expect(describeError(cyclic)).toBe("[object Object]");
  });
});

describe("logFrontendError", () => {
  beforeEach(() => logError.mockClear());
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("does nothing outside Tauri (the browser preview)", () => {
    logFrontendError("test", new Error("x"));
    expect(logError).not.toHaveBeenCalled();
  });

  it("writes where it happened and on which page", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    logFrontendError("uncaught", "bad thing");
    expect(logError).toHaveBeenCalledWith(expect.stringMatching(/^\[uncaught\] \/.*: bad thing$/));
  });

  it("never throws, even when the log command fails", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    logError.mockRejectedValueOnce(new Error("no log plugin"));
    expect(() => logFrontendError("x", "y")).not.toThrow();
    logError.mockImplementationOnce(() => {
      throw new Error("sync failure");
    });
    expect(() => logFrontendError("x", "y")).not.toThrow();
  });

  it("logs uncaught errors and unhandled rejections, installed once", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    installErrorLogging();
    installErrorLogging();
    window.dispatchEvent(new ErrorEvent("error", { error: new Error("kaboom") }));
    const rejection = new Event("unhandledrejection") as Event & { reason?: unknown };
    rejection.reason = "rejected";
    window.dispatchEvent(rejection);
    expect(logError).toHaveBeenCalledTimes(2);
    expect(logError.mock.calls[0][0]).toMatch(/^\[uncaught\][\s\S]*kaboom/);
    expect(logError.mock.calls[1][0]).toMatch(/^\[unhandled rejection\].*rejected$/);
  });
});

describe("warnings", () => {
  beforeEach(() => logWarn.mockClear());
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("go to the log file too, not only the console nobody sees in a release build", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const quiet = vi.spyOn(console, "warn").mockImplementation(() => {});
    warn("lab status poll")(new Error("docker hung"));
    tell("Couldn't stop the lab")("vagrant destroy failed");
    quiet.mockRestore();
    expect(logWarn.mock.calls.map(([m]) => m)).toEqual([
      expect.stringMatching(/^\[lab status poll\] \/.*: Error: docker hung/),
      expect.stringMatching(/^\[Couldn't stop the lab\] \/.*: vagrant destroy failed$/),
    ]);
  });
});

describe("logFolder", () => {
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("is the app's log folder in Tauri, and unknown in the browser preview", async () => {
    expect(await logFolder()).toBeNull();
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(await logFolder()).toBe("/logs/org.cyberctf.desktop");
  });
});
