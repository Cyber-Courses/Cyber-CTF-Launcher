import { describe, expect, it } from "vitest";
import { failureOf, readFailedLine } from "@/lib/log-marks";

describe("readFailedLine", () => {
  it("reads the error's code off a worker's failed line, and shows the line without it", () => {
    expect(readFailedLine("✗[portInUse] Host port 8000 is already in use on this machine.")).toEqual({
      line: "✗ Host port 8000 is already in use on this machine.",
      code: "portInUse",
    });
    expect(failureOf([readFailedLine("✗[other] boom").line])).toBe("boom");
  });

  it("leaves any other line as it is", () => {
    for (const line of ["✗ boom", "==> web: Booting VM...", "✓ Lab is running", "✗[unclosed"]) expect(readFailedLine(line)).toEqual({ line, code: null });
  });
});
