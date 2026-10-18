// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatAgo, formatBytes, formatDuration, formatElapsed, prettyLabId } from "@/lib/format";
import { setLocalePreference } from "@/lib/i18n";
import {
  ATTACK_PRESETS,
  ATTACK_VM_PRESETS,
  attackImageLabel,
  CYBERCTF_ATTACK_REPO,
  DEFAULT_ATTACK_BOX,
  DEFAULT_ATTACK_IMAGE,
  getAttackBox,
  getAttackImage,
  getAutoAttackBox,
  getLastTest,
  getPortMode,
  getVmProvider,
  OLD_ATTACK_IMAGES,
  setAttackBox,
  setAttackImage,
  setAutoAttackBox,
  setLastTest,
  setPortMode,
  setVmProvider,
} from "@/lib/settings";
import { cn } from "@/lib/utils";
import { isMac } from "@/lib/platform";

describe("format", () => {
  beforeEach(() => {
    setLocalePreference("en");
  });
  afterEach(() => {
    setLocalePreference("system");
  });

  it("formats a running step's time", () => {
    expect(formatDuration(400)).toBe("0.4s");
    expect(formatDuration(-5)).toBe("0.0s");
    expect(formatDuration(12_000)).toBe("12s");
    expect(formatDuration(65_000)).toBe("1:05");
  });

  it("formats whole seconds", () => {
    expect(formatElapsed(42_900)).toBe("42s");
    expect(formatElapsed(3_600_000)).toBe("60:00");
    expect(formatElapsed(-1)).toBe("0s");
  });

  it("formats how long ago, with plurals", () => {
    const now = 1_000_000_000_000;
    expect(formatAgo(now - 10_000, now)).toBe("just now");
    expect(formatAgo(now - 60_000, now)).toMatch(/^1 min/);
    expect(formatAgo(now - 5 * 60_000, now)).toMatch(/^5 min/);
    expect(formatAgo(now - 3 * 3_600_000, now)).toMatch(/^3 h/);
    expect(formatAgo(now - 86_400_000, now)).toBe("yesterday");
    expect(formatAgo(now - 4 * 86_400_000, now)).toBe("4 days ago");
    expect(formatAgo(now + 5000, now)).toBe("just now");
  });

  it("formats sizes", () => {
    expect(formatBytes(12e9)).toBe("12 GB");
    expect(formatBytes(3.4e9)).toBe("3.4 GB");
    expect(formatBytes(512e6)).toBe("512 MB");
    expect(formatBytes(8000)).toBe("8 kB");
    expect(formatBytes(10)).toBe("1 kB");
  });

  it("makes a lab id readable when its title isn't known", () => {
    expect(prettyLabId("lab-goad")).toBe("Goad");
    expect(prettyLabId("goad-light")).toBe("Goad Light");
    expect(prettyLabId("lab_web_sqli")).toBe("Web Sqli");
    expect(prettyLabId("lab-")).toBe("lab-");
  });

  it("follows the language's decimal separator", () => {
    setLocalePreference("fr");
    expect(formatBytes(3.4e9)).toMatch(/^3,4/);
    expect(formatDuration(400)).toMatch(/^0,4/);
  });
});

describe("settings", () => {
  it("defaults the attack box image and VM box", () => {
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    expect(getAttackBox()).toBe(DEFAULT_ATTACK_BOX);
  });

  it("defaults to the Cyber CTF box pinned by digest, shown as its repository", () => {
    expect(DEFAULT_ATTACK_IMAGE).toMatch(/^cyberctf\/attack-box@sha256:[0-9a-f]{64}$/);
    expect(ATTACK_PRESETS[0].image).toBe(DEFAULT_ATTACK_IMAGE);
    expect(attackImageLabel(DEFAULT_ATTACK_IMAGE)).toBe(CYBERCTF_ATTACK_REPO);
    expect(attackImageLabel("ghcr.io/me/box")).toBe("ghcr.io/me/box");
    expect(OLD_ATTACK_IMAGES).toEqual(expect.arrayContaining(["cyberctf/attack-box", "cyberctf/attack-box:latest"]));
    expect(OLD_ATTACK_IMAGES).not.toContain(DEFAULT_ATTACK_IMAGE);
  });

  it.each(["cyberctf/attack-box", "cyberctf/attack-box:latest"])("moves a saved old default (%s) to the pinned default", (old) => {
    localStorage.setItem("cyberctf.attackbox.image", old);
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    expect(localStorage.getItem("cyberctf.attackbox.image")).toBeNull();
  });

  it("keeps a custom image, even another tag or digest of the Cyber CTF box", () => {
    for (const image of ["ghcr.io/me/box:1", "cyberctf/attack-box:2026-10-05", `cyberctf/attack-box@sha256:${"a".repeat(64)}`]) {
      localStorage.setItem("cyberctf.attackbox.image", image);
      expect(getAttackImage()).toBe(image);
      expect(localStorage.getItem("cyberctf.attackbox.image")).toBe(image);
    }
  });

  it("doesn't store the default, so a later pinned default applies", () => {
    setAttackImage("ghcr.io/me/box");
    setAttackImage(DEFAULT_ATTACK_IMAGE);
    expect(localStorage.getItem("cyberctf.attackbox.image")).toBeNull();
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    setAttackImage("cyberctf/attack-box:latest");
    expect(localStorage.getItem("cyberctf.attackbox.image")).toBeNull();
  });

  it("saves trimmed images and falls back to the default when blank", () => {
    setAttackImage("  ghcr.io/me/box  ");
    expect(getAttackImage()).toBe("ghcr.io/me/box");
    setAttackImage("   ");
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    setAttackBox(" generic/debian12 ");
    expect(getAttackBox()).toBe("generic/debian12");
    setAttackBox("");
    expect(getAttackBox()).toBe(DEFAULT_ATTACK_BOX);
  });

  it("keeps or clears the chosen hypervisor", () => {
    expect(getVmProvider()).toBeNull();
    setVmProvider("virtualbox");
    expect(getVmProvider()).toBe("virtualbox");
    setVmProvider(null);
    expect(getVmProvider()).toBeNull();
  });

  it("remembers the last self-test per kind", () => {
    expect(getLastTest("docker")).toBeNull();
    vi.spyOn(Date, "now").mockReturnValue(1234);
    setLastTest("docker", "ok");
    vi.restoreAllMocks();
    expect(getLastTest("docker")).toEqual({ result: "ok", at: 1234 });
    expect(getLastTest("vm")).toBeNull();
    localStorage.setItem("cyberctf.selftest.vm", "{not json");
    expect(getLastTest("vm")).toBeNull();
  });

  it("starts the attack box automatically unless turned off", () => {
    expect(getAutoAttackBox()).toBe(true);
    setAutoAttackBox(false);
    expect(getAutoAttackBox()).toBe(false);
    setAutoAttackBox(true);
    expect(getAutoAttackBox()).toBe(true);
  });

  it("reads only known port modes", () => {
    expect(getPortMode()).toBeNull();
    setPortMode("default");
    expect(getPortMode()).toBe("default");
    localStorage.setItem("cyberctf.labs.ports", "weird");
    expect(getPortMode()).toBeNull();
    setPortMode("random");
    setPortMode(null);
    expect(getPortMode()).toBeNull();
  });

  it("falls back to defaults when storage is unavailable", () => {
    const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const remove = vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(getAttackImage()).toBe(DEFAULT_ATTACK_IMAGE);
    expect(getAttackBox()).toBe(DEFAULT_ATTACK_BOX);
    expect(getVmProvider()).toBeNull();
    expect(getLastTest("vm")).toBeNull();
    expect(getAutoAttackBox()).toBe(true);
    expect(getPortMode()).toBeNull();
    expect(() => {
      setAttackImage("x");
      setAttackBox("x");
      setVmProvider("virtualbox");
      setVmProvider(null);
      setLastTest("vm", "fail");
      setAutoAttackBox(false);
      setPortMode("random");
      setPortMode(null);
    }).not.toThrow();
    get.mockRestore();
    set.mockRestore();
    remove.mockRestore();
  });

  it("describes every preset in the current language", () => {
    for (const p of [...ATTACK_PRESETS, ...ATTACK_VM_PRESETS]) expect(p.note.length).toBeGreaterThan(0);
    for (const p of ATTACK_PRESETS) if (p.terms !== undefined) expect(typeof p.terms).toBe("string");
  });
});

describe("utils", () => {
  it("joins class names, skipping falsy values and flattening arrays", () => {
    expect(cn("a", null, undefined, false, "", ["b", ["c", false]], 0, 1)).toBe("a b c 0 1");
    expect(cn()).toBe("");
  });

  it("detects macOS from the user agent", () => {
    const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    expect(isMac()).toBe(true);
    ua.mockReturnValue("Mozilla/5.0 (X11; Linux x86_64)");
    expect(isMac()).toBe(false);
    ua.mockRestore();
  });
});
