// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_ATTACK_IMAGE,
  getAttackImage,
  getAutoAttackBox,
  getDockerEngine,
  getLastTest,
  getPortMode,
  getVmProvider,
  setAttackImage,
  setLastTest,
} from "@/lib/settings";

// What each getter does with a value that is in storage but not one it wrote: an older
// version's format, a hand edit in dev tools, or a partial write.
describe("settings: stray values in storage", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("reads a saved hypervisor or engine that names none as none, so no stale id is sent", () => {
    // Regression: the saved string was cast and sent to the backend as the preferred one.
    localStorage.setItem("cyberctf.vm.provider", "virtualbox");
    localStorage.setItem("cyberctf.docker.engine", "orbstack");
    expect([getVmProvider(), getDockerEngine()]).toEqual(["virtualbox", "orbstack"]);
    localStorage.setItem("cyberctf.vm.provider", "vmware_esxi");
    localStorage.setItem("cyberctf.docker.engine", "lima");
    expect([getVmProvider(), getDockerEngine()]).toEqual([null, null]);
  });

  it("logs a setting storage refused to save, and reads none where storage can't be read", () => {
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("no storage");
    });
    setAttackImage("kalilinux/kali-rolling");
    expect(warned).toHaveBeenCalledWith("saving cyberctf.attackbox.image:", expect.any(Error));
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
  });

  it("reads a self-test saved as JSON null as no test, and keeps kinds apart", () => {
    localStorage.setItem("cyberctf.selftest.docker", "null");
    expect(getLastTest("docker")).toBeNull();
    vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    setLastTest("vm", "fail");
    vi.restoreAllMocks();
    expect(getLastTest("vm")).toEqual({ result: "fail", at: 1_700_000_000_000, why: null });
    expect(getLastTest("docker")).toBeNull();
  });

  it("keeps a provider's own test apart from the kind's, which only the one labs run on writes", () => {
    vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    // Another hypervisor's test: its own record, the VM labs' untouched.
    setLastTest("vm", "fail", { provider: "vmware_desktop", primary: false, why: "no box for vmware_desktop" });
    expect(getLastTest("vm", "vmware_desktop")).toEqual({ result: "fail", at: 1_700_000_000_000, why: "no box for vmware_desktop" });
    expect(getLastTest("vm")).toBeNull();
    expect(getLastTest("vm", "qemu")).toBeNull();
    // The default hypervisor's test: its own record and the VM labs' both; a pass keeps no why.
    setLastTest("vm", "ok", { provider: "qemu", why: "stale" });
    expect(getLastTest("vm", "qemu")).toEqual({ result: "ok", at: 1_700_000_000_000, why: null });
    expect(getLastTest("vm")).toEqual({ result: "ok", at: 1_700_000_000_000, why: null });
    vi.restoreAllMocks();
  });

  it("a self-test record of the wrong shape is no result", () => {
    // Regression: only broken JSON was refused: `42`, `[]` or an object without `at` read as a last
    // test, and "tested … ago" was computed from NaN.
    for (const raw of ["42", "[]", '"ok"', '{"result":"ok"}', '{"result":"maybe","at":1}']) {
      localStorage.setItem("cyberctf.selftest.docker", raw);
      expect(getLastTest("docker"), raw).toBeNull();
    }
  });

  it("treats anything but the word false as auto-start on", () => {
    for (const raw of ["", "0", "no", "FALSE", "off"]) {
      localStorage.setItem("cyberctf.attackbox.auto", raw);
      expect(getAutoAttackBox(), JSON.stringify(raw)).toBe(true);
    }
    localStorage.setItem("cyberctf.attackbox.auto", "false");
    expect(getAutoAttackBox()).toBe(false);
  });

  it("falls back to the default image for an empty value, but keeps inner whitespace as typed", () => {
    localStorage.setItem("cyberctf.attackbox.image", "");
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    // The field is free text: trimmed at the ends only, nothing inside is validated.
    setAttackImage("  ghcr.io/me/my box  ");
    expect(getAttackImage()).toBe("ghcr.io/me/my box");
  });

  it("reads the port mode case-sensitively", () => {
    localStorage.setItem("cyberctf.labs.ports", "Random");
    expect(getPortMode()).toBeNull();
    localStorage.setItem("cyberctf.labs.ports", "random");
    expect(getPortMode()).toBe("random");
  });
});
