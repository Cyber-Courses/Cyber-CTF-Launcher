// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

const OLD = `cyberctf/attack-box@sha256:${"1".repeat(64)}`;
const NEW = `cyberctf/attack-box@sha256:${"2".repeat(64)}`;

// A launcher whose default was bumped once: the bump PR moved the old digest to `previous`.
vi.mock("../../src-tauri/attack-box-image.json", () => ({ default: { image: NEW, previous: [OLD] } }));

afterEach(() => {
  localStorage.clear();
});

describe("a bumped default attack box", () => {
  it("moves a saved earlier pinned default to the new one", async () => {
    const { DEFAULT_ATTACK_IMAGE, getAttackImage } = await import("@/lib/settings");
    expect(DEFAULT_ATTACK_IMAGE).toBe(NEW);
    localStorage.setItem("cyberctf.attackbox.image", OLD);
    expect(getAttackImage()).toBe(NEW);
    expect(localStorage.getItem("cyberctf.attackbox.image")).toBeNull();
  });

  it("leaves a custom image alone", async () => {
    const { getAttackImage } = await import("@/lib/settings");
    localStorage.setItem("cyberctf.attackbox.image", "kalilinux/kali-rolling");
    expect(getAttackImage()).toBe("kalilinux/kali-rolling");
  });
});
