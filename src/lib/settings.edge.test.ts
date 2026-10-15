// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { DEFAULT_ATTACK_IMAGE, getAttackImage, getAutoAttackBox, getLastTest, getPortMode, setAttackImage, setLastTest } from "@/lib/settings";

// What each getter does with a value that is in storage but not one it wrote: an older
// version's format, a hand edit in dev tools, or a partial write.
describe("settings: stray values in storage", () => {
  it("reads a self-test saved as JSON null as no test, and keeps kinds apart", () => {
    localStorage.setItem("cyberctf.selftest.docker", "null");
    expect(getLastTest("docker")).toBeNull();
    vi.spyOn(Date, "now").mockReturnValue(1_700_000_000_000);
    setLastTest("vm", "fail");
    vi.restoreAllMocks();
    expect(getLastTest("vm")).toEqual({ result: "fail", at: 1_700_000_000_000 });
    expect(getLastTest("docker")).toBeNull();
  });

  it("a self-test record of the wrong shape is no result", () => {
    // Only broken JSON is caught: `42`, `[]` or an object without `at` parse fine and come
    // back typed as LastTest, so `last.at` is undefined and "tested … ago" is computed from
    // NaN. Suggested fix: return null unless `parsed.result` is "ok" | "fail" and
    // `typeof parsed.at === "number"`.
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
