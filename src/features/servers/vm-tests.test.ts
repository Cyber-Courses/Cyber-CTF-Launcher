// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { loadVmTests, saveVmTest } from "@/features/servers/vm-tests";
import { seedSettings } from "@/test/settings";

describe("remembered VM tests", () => {
  it("keeps each host's last result beside the others'", () => {
    seedSettings({ serverTests: { pve: { result: "ok", at: 1, why: null } } });
    expect(loadVmTests()).toEqual({ pve: { result: "ok", at: 1 } });
    expect(saveVmTest("esxi", "fail").esxi?.result).toBe("fail");
    expect(Object.keys(loadVmTests()).sort()).toEqual(["esxi", "pve"]);
  });
});
