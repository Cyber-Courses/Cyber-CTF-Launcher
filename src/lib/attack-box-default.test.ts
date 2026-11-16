// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";

const OLD = `cyberctf/attack-box@sha256:${"1".repeat(64)}`;
const NEW = `cyberctf/attack-box@sha256:${"2".repeat(64)}`;

// A launcher whose default was bumped once: the bump PR moved the old digest to `previous`.
vi.mock("../../src-tauri/attack-box/image.json", () => ({ default: { image: NEW, previous: [OLD] } }));

describe("a bumped default attack box", () => {
  it("runs the new default and cleans the earlier one up (the core no longer keeps it saved)", async () => {
    const { DEFAULT_ATTACK_IMAGE, OLD_ATTACK_IMAGES, getAttackImage } = await import("@/lib/settings");
    expect(DEFAULT_ATTACK_IMAGE).toBe(NEW);
    expect(getAttackImage()).toBe(NEW);
    expect(OLD_ATTACK_IMAGES).toContain(OLD);
    expect(OLD_ATTACK_IMAGES).not.toContain(NEW);
  });

  it("leaves a custom image alone", async () => {
    const { getAttackImage, setAttackImage } = await import("@/lib/settings");
    setAttackImage("kalilinux/kali-rolling");
    expect(getAttackImage()).toBe("kalilinux/kali-rolling");
  });
});
