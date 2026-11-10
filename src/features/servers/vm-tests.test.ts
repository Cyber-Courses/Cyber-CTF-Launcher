// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadVmTests, saveVmTest } from "@/features/servers/vm-tests";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("remembered VM tests", () => {
  it("keeps each host's last result, and only well-formed ones", () => {
    localStorage.setItem("cyberctf.server.vmtest", JSON.stringify({ pve: { result: "ok", at: 1 }, bad: { result: "maybe", at: 2 } }));
    expect(loadVmTests()).toEqual({ pve: { result: "ok", at: 1 } });
    expect(saveVmTest("esxi", "fail").esxi?.result).toBe("fail");
    expect(Object.keys(loadVmTests()).sort()).toEqual(["esxi", "pve"]);
  });

  it("says why the results couldn't be read or kept, and goes on without them", () => {
    // Regression: both were swallowed by a bare catch.
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    localStorage.setItem("cyberctf.server.vmtest", "{ cut short");
    expect(loadVmTests()).toEqual({});
    expect(warned).toHaveBeenCalledWith("reading the remembered VM tests:", expect.any(SyntaxError));
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(saveVmTest("pve", "ok").pve?.result).toBe("ok");
    expect(warned).toHaveBeenCalledWith("saving cyberctf.server.vmtest:", expect.any(Error));
  });
});
