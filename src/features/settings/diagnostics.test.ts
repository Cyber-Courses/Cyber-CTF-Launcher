import { describe, expect, it } from "vitest";
import { diagnosticsText } from "@/features/settings/diagnostics";
import { freshReport, readyReport } from "@/test/fixtures";

const settings = { attackImage: "cyberctf/attack-box", attackBox: "kalilinux/rolling", vmProvider: null };

describe("diagnosticsText", () => {
  it("lists the version, the system, the tools and the settings", () => {
    const report = readyReport();
    const text = diagnosticsText("1.4.0", report, { installId: "c0ffee", name: "mbp", arch: "arm64", capabilities: [] }, settings);
    const lines = text.split("\n");
    expect(lines[0]).toBe("Cyber CTF v1.4.0");
    expect(lines[1]).toBe(`OS: ${report.os} ${report.arch}`);
    expect(lines[2]).toMatch(/^Docker: running \(/);
    expect(text).toContain("Attack box: cyberctf/attack-box");
    expect(text).toContain("Attack VM: kalilinux/rolling");
    expect(text).toContain("VM provider: automatic");
    expect(lines.at(-1)).toBe("Install ID: c0ffee");
  });

  it("leaves out the system lines before the machine check, and says when signed out", () => {
    const text = diagnosticsText(null, null, null, { ...settings, vmProvider: "qemu" });
    expect(text.split("\n")).toEqual([
      "Cyber CTF (unknown version)",
      "Attack box: cyberctf/attack-box",
      "Attack VM: kalilinux/rolling",
      "VM provider: qemu",
      "Not signed in",
    ]);
  });

  it("describes a fresh machine", () => {
    const text = diagnosticsText("1.4.0", freshReport(), null, settings);
    expect(text).toContain("Docker: not installed");
    expect(text).toContain("Vagrant: not installed, Terraform: not installed");
    expect(text).toContain("Hypervisors ready: none");
  });
});
