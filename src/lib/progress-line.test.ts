import { describe, expect, it } from "vitest";
import { progressText, uploadPercent } from "@/lib/progress-line";

describe("progress lines", () => {
  it("reads the OVF Tool's upload progress, as Vagrant passes it on or alone", () => {
    expect(uploadPercent("Disk progress: 8%")).toBe(8);
    expect(uploadPercent("    probe: Disk progress: 100%")).toBe(100);
    expect(uploadPercent("Transfer Completed")).toBeNull();
    expect(uploadPercent("Progress: 45% (Rate: 3 MB/s)")).toBeNull();
  });

  it("says an upload's progress in words, any other line as it is", () => {
    expect(progressText("Disk progress: 8%")).toBe("Uploading: 8%");
    expect(progressText("Opening VMX source: /tmp/probe.vmx")).toBe("Opening VMX source: /tmp/probe.vmx");
  });
});
