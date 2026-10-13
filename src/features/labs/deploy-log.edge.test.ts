import { describe, expect, it } from "vitest";

import { deriveSteps, detectTarget, failedMachineStep } from "@/features/labs/deploy-log";
import type { T } from "@/lib/i18n";

// Labels as their message keys, so the tests read which step a line landed in.
const t = ((key: string) => key) as unknown as T;
const timed = (lines: string[]) => lines.map((line, i) => ({ line, at: i * 1000 }));
const steps = (lines: string[]) => deriveSteps(t, timed(lines)).map((s) => [s.id, s.label, s.rows.length]);

describe("deriveSteps: odd Vagrant output", () => {
  it("reads ==> lines with odd spacing and dotted or hyphenated machine names", () => {
    expect(steps(["   ==>dc01.lab:   Importing base box", "==>   web-1: Booting VM..."])).toEqual([
      ["m:dc01.lab", "dc01.lab", 1],
      ["m:web-1", "web-1", 1],
    ]);
    expect(detectTarget("==>dc01:")).toBe("vagrant");
  });

  it("lines before the first ==> are preparation, named after the launcher's narration when that comes first", () => {
    expect(steps(["Clearing leftover VM state", "Bringing machine 'isoloom-dc01' up with 'virtualbox' provider...", "==> isoloom-dc01: Importing"])).toEqual([
      ["prepare", "labs.deploy.steps.prepareMachine", 2],
      ["m:isoloom-dc01", "dc01", 1],
    ]);
    // The step keeps the label of its first line.
    expect(steps(["Bringing machine 'x' up with 'y' provider...", "Recovering the lab", "==> x: Booting"])[0]).toEqual([
      "prepare",
      "labs.deploy.steps.start",
      2,
    ]);
  });

  it("keeps the launcher's download and placement lines in the download step, before the target's output", () => {
    expect(steps(["Downloading lab files", "Lab installed", "Running on server host esxi-1", "==> dc01: Booting"])).toEqual([
      ["download", "labs.deploy.steps.download", 3],
      ["m:dc01", "dc01", 1],
    ]);
  });

  it("a blank line joins the current machine's step", () => {
    expect(steps(["==> dc01: Booting", "   ", ""])).toEqual([["m:dc01", "dc01", 3]]);
  });

  it("prefers Vagrant over Docker Compose when a VM lab's output mentions containers", () => {
    expect(detectTarget("==> dc01: Booting\nContainer web Started")).toBe("vagrant");
  });
});

describe("deriveSteps: Docker Compose and Terraform order", () => {
  it("follows the log's order of phases, and later lines join the current one", () => {
    expect(steps(["Network lab_default Creating", "Image api Pulling", "Container api Started", "✓ Lab ready"])).toEqual([
      ["create", "labs.deploy.phases.create", 1],
      ["images", "labs.deploy.phases.images", 1],
      ["start", "labs.deploy.phases.start", 2],
    ]);
  });

  it("lines before the first Compose phase are preparation", () => {
    expect(steps(["Preparing the lab", "Container api Creating"])).toEqual([
      ["prepare", "labs.deploy.steps.prepareLab", 1],
      ["create", "labs.deploy.phases.create", 1],
    ]);
  });

  it("a Terraform run starts in init whatever its first line, and unmatched lines join the step before", () => {
    expect(steps(["Random words", "Initializing the backend...", "aws_instance.lab: Creating...", "Error: timeout"])).toEqual([
      ["tf-init", "labs.deploy.steps.tfInit", 2],
      ["tf-apply", "labs.deploy.steps.tfApply", 2],
    ]);
  });

  it("a cloud run stays Terraform when Docker lines appear on the host later", () => {
    const lines = ["Terraform has been successfully initialized!", "aws_instance.lab: Creation complete", "Container web Started"];
    expect(detectTarget(lines.join("\n"))).toBe("terraform");
    expect(steps(lines).at(-1)).toEqual(["tf-apply", "labs.deploy.steps.tfApply", 2]);
  });

  it("an empty log has no target and no steps", () => {
    expect(detectTarget("")).toBe("unknown");
    expect(deriveSteps(t, [])).toEqual([]);
  });
});

describe("failedMachineStep", () => {
  it("reads a machine name with dots and hyphens, and none with a space in it", () => {
    expect(failedMachineStep("An error occurred while executing the action on the 'isoloom-dc-01.lab' machine. Please")).toBe("m:isoloom-dc-01.lab");
    expect(failedMachineStep("executing the action on the 'dc 01' machine")).toBeNull();
  });
});
