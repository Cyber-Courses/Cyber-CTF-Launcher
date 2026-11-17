import { describe, expect, it } from "vitest";

import { deriveSteps } from "@/features/labs/deploy-log";
import type { T } from "@/lib/i18n";
import type { DeployStep } from "@/lib/tauri/generated";
import type { SaidStep } from "@/lib/tauri/run-events";

// Labels as their message keys, so the tests read which step a line landed in.
// The launcher's own lines below are the ones it prints (Rust's `runtime::deploy_log`, whose tests
// hold them to these words); the steps are the runner's (its `step` run events).
const EMULATING =
  "Emulating x86_64 on this aarch64 machine with QEMU, many times slower than native: a Windows machine takes 15 to 40 minutes to boot, and the lab's setup can take hours.";
const t = ((key: string) => key) as unknown as T;
const timed = (lines: string[]) => lines.map((line, i) => ({ line, at: i * 1000 }));

/** A step the runner said, after `line` lines: `images started 1/2`, `up:dc01 done`. */
function said(line: number, text: string): SaidStep {
  const [name, state, count] = text.split(" ");
  const [step, label = null] = name.split(":");
  const parts = count ? count.split("/").map(Number) : [];
  const counted = count ? { done: parts[0], total: parts.length > 1 ? parts[1] : null } : null;
  return { step: { step, state: state as DeployStep["state"], label, count: counted }, at: line * 1000 - 1, line };
}

const steps = (lines: string[], marks: SaidStep[] = []) =>
  deriveSteps(t, timed(lines), marks).map((s) => [s.id, s.label, s.rows.length, s.said?.state ?? null, s.said?.detail ?? null]);

describe("deriveSteps", () => {
  it("reads Docker Compose's steps as the runner says them, each with how far it got", () => {
    expect(
      steps(
        ["Downloading lab", " web Pulling", " web Pulled", " Network lab_default  Creating", " Container web-1  Started", " Container web-1  Healthy"],
        [
          said(1, "images started"),
          said(2, "images started 1/1"),
          said(3, "images done 1/1"),
          said(3, "create started"),
          said(4, "create done 1/1"),
          said(4, "start started 0/1"),
          said(5, "start done 1/1"),
          said(5, "health started 0/1"),
          said(6, "health done 1/1"),
        ],
      ),
    ).toEqual([
      ["download", "labs.deploy.steps.download", 1, null, null],
      ["images", "labs.deploy.phases.images", 2, "ok", "1/1"],
      ["create", "labs.deploy.phases.create", 1, "ok", "1/1"],
      ["start", "labs.deploy.phases.start", 1, "ok", "1/1"],
      ["health", "labs.deploy.phases.health", 1, "ok", "1/1"],
    ]);
  });

  it("gives each Vagrant machine one step, without the isoloom- prefix, saying what it does while it runs", () => {
    expect(
      steps(
        [
          "Clearing a previous, stopped run…",
          "==> isoloom-dc01: Importing box",
          "==> isoloom-ws01: Booting",
          "==> isoloom-dc01: Running provisioner: shell...",
          "    ws01 output",
        ],
        [said(1, "up:isoloom-dc01 started"), said(2, "up:isoloom-ws01 started"), said(3, "up:isoloom-dc01 done"), said(3, "provision:isoloom-dc01 started")],
      ),
    ).toEqual([
      ["prepare", "labs.deploy.steps.prepareMachine", 1, null, null],
      // A line goes to the step started last: here dc01's set-up, ws01's line too.
      ["machine:isoloom-dc01", "dc01", 3, "running", "labs.deploy.phases.provision"],
      ["machine:isoloom-ws01", "ws01", 1, "running", "labs.deploy.phases.up"],
    ]);
  });

  it("gives the boxes downloaded before the machines their own step, the lab's download its own", () => {
    expect(
      steps(
        [
          "Downloading CyberCTF/ad@0123456789ab",
          "==> dc01: Box 'bento/debian-12' could not be found.",
          "    dc01: Progress: 42% (Rate: 1201k/s, Estimated time remaining: 0:04:10)",
          "==> dc01: Importing base box 'bento/debian-12'...",
        ],
        [said(1, "boxes started 0/1"), said(3, "boxes done 1/1"), said(3, "up:dc01 started")],
      ),
    ).toEqual([
      ["download", "labs.deploy.steps.download", 1, null, null],
      ["boxes", "labs.deploy.phases.boxes", 2, "ok", "1/1"],
      ["machine:dc01", "dc01", 1, "running", "labs.deploy.phases.up"],
    ]);
  });

  it("reads Terraform's init, apply and the lab's set-up on its host", () => {
    expect(
      steps(
        [
          "Running in your AWS account Labs (eu-west-1)",
          "Initializing the backend...",
          "aws_instance.lab: Creating...",
          "aws_instance.lab: Creation complete",
          "null_resource.setup: Provisioning…",
        ],
        [said(1, "init started"), said(2, "init done"), said(2, "apply started 0/2"), said(4, "apply started 1/2"), said(4, "ready started")],
      ),
    ).toEqual([
      ["download", "labs.deploy.steps.download", 1, null, null],
      ["init", "labs.deploy.phases.init", 1, "ok", null],
      ["apply", "labs.deploy.phases.apply", 2, "running", "1/2"],
      ["ready", "labs.deploy.phases.ready", 1, "running", null],
    ]);
  });

  it("marks the step the runner said failed, and the ones it cut short", () => {
    expect(
      steps(
        ["==> dc01: Booting", "==> ws01: Booting", "An error occurred while executing the action on the 'dc01' machine."],
        [said(0, "up:dc01 started"), said(1, "up:ws01 started"), said(3, "up:dc01 failed"), said(3, "up:ws01 skipped")],
      ),
    ).toEqual([
      ["machine:dc01", "dc01", 1, "fail", null],
      ["machine:ws01", "ws01", 2, "skipped", null],
    ]);
  });

  it("files a line under the step said last once none runs, and names a step it doesn't know by its id", () => {
    expect(steps(["a", "b"], [said(0, "images started"), said(1, "images done"), said(2, "warmup started")])).toEqual([
      ["images", "labs.deploy.phases.images", 2, "ok", null],
      ["warmup", "warmup", 0, "running", null],
    ]);
  });

  it("keeps the launcher's own lines in the download and preparation steps before the runner's first", () => {
    expect(steps(["Downloading CyberCTF/ad@0123456789ab", "Lab installed", "Running on server host esxi-1 (vmware_esxi)", EMULATING, "x"])).toEqual([
      ["download", "labs.deploy.steps.download", 3, null, null],
      ["prepare", "labs.deploy.steps.prepareMachine", 2, null, null],
    ]);
    expect(steps(["Importing local-web from /home/ada/labs/web", "Lab imported", "hello"])).toEqual([
      ["download", "labs.deploy.steps.download", 2, null, null],
      ["prepare", "labs.deploy.steps.preparing", 1, null, null],
    ]);
  });

  it("reads no step out of a tool's words: a log the runner said no step of stays one preparing step", () => {
    expect(steps(["==> dc01: Booting", " Container web-1  Started", "aws_instance.lab: Creating..."])).toEqual([
      ["prepare", "labs.deploy.steps.preparing", 3, null, null],
    ]);
  });

  it("an empty log has no steps", () => {
    expect(deriveSteps(t, [])).toEqual([]);
  });
});
