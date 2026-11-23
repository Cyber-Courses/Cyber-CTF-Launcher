// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_ATTACK_IMAGE, getAttackImage, getLastTest, setAttackImage, setLastTest, validImage } from "@/lib/settings";

// The setters' records as the getters read them back (what the core does with a value it can't
// read is its own tests', src-tauri/src/settings/tests.rs).
describe("settings: what a setter keeps", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it("keeps the kinds' self-tests apart", () => {
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

  it("falls back to the default image for an empty value, but keeps inner whitespace as typed", () => {
    setAttackImage("");
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    // The field is free text: trimmed at the ends only, nothing inside is validated.
    setAttackImage("  ghcr.io/me/my box  ");
    expect(getAttackImage()).toBe("ghcr.io/me/my box");
  });
});

describe("validImage", () => {
  it("takes a plain image reference, the core's own pattern", () => {
    for (const image of ["cyberctf/attack-box", "ghcr.io/org/img:1.2@sha256:abc", "registry.local:5000/team/wordpress:6.4", "a-", "a".repeat(200)]) {
      expect(validImage(image), image).toBe(true);
    }
  });

  it("refuses what reads as a docker option, an empty or too long name, and characters a name hasn't", () => {
    for (const image of ["", "-v", "--privileged", "a b", "img; rm -rf /", "é", "img\n", "a".repeat(201)]) {
      expect(validImage(image), JSON.stringify(image)).toBe(false);
    }
  });
});
