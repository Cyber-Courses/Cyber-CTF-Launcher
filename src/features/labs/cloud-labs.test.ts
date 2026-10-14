import { describe, expect, it } from "vitest";

import { cloudView, deriveLabState, type LabStateInput } from "@/features/labs/lab-detail/lab-state";
import { runtimeWord } from "@/features/labs/lab-detail/labels";
import { initialTarget } from "@/features/labs/lab-detail/use-run-target";
import { costEstimate } from "@/features/labs/cloud-panel";
import { filterLabs, NO_FILTERS } from "@/features/labs/lab-filters";
import { needsCloudAccount, setupNeeded } from "@/features/labs/lab-readiness";
import { cloudAccountsFor, cloudNames, isCloudLab, localProviders, runPlaces, runtimeLabel } from "@/features/labs/lab-runtime";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus, ServerHost, SystemReport } from "@/lib/tauri";
import { translate } from "@/lib/i18n";

const rt = (runtime: LabRuntimeInfo["runtime"], providers: string[]) => ({ runtime, providers, architectures: [] }) as unknown as LabRuntimeInfo;
const cloud = rt("CLOUD", ["aws"]);
const lab = (runtime: LabRuntimeInfo, title = "Lab") =>
  ({ id: title, title, category: "", description: "", skills: [], difficulty: 1, runtime }) as unknown as Lab;
const host = (id: string, provider: string) => ({ id, name: id, provider }) as unknown as ServerHost;
const report = (terraform: boolean) =>
  ({ docker: { installed: false }, dockerRunning: false, vmProviders: [], terraform: { installed: terraform } }) as unknown as SystemReport;
const status = (p: Partial<LabStatus>): LabStatus => ({
  running: false,
  parked: null,
  machines: [],
  networks: [],
  url: null,
  host: null,
  expiresAt: null,
  place: null,
  provider: null,
  attacker: null,
  outputs: [],
  message: null,
  reapError: null,
  ...p,
});
const derive = (p: Partial<LabStateInput>) =>
  deriveLabState({
    status: undefined,
    runtime: cloud,
    hostArch: "x86_64",
    readyVms: [],
    busy: false,
    backendDeploying: false,
    backendOp: undefined,
    acting: null,
    parkable: true,
    provisionable: true,
    ...p,
  });

describe("cloud labs", () => {
  it("run in a cloud account only, never here", () => {
    expect(
      runPlaces(cloud, "arm64")
        .filter((p) => p.available)
        .map((p) => p.key),
    ).toEqual(["cloud"]);
    expect(localProviders(cloud, "arm64")).toEqual([]);
    expect(isCloudLab(cloud)).toBe(true);
    expect(isCloudLab(rt("DOCKER", ["aws"]))).toBe(false);
  });

  it("read as cloud, docker or vm, and name their clouds", () => {
    expect(runtimeLabel(cloud)).toBe("cloud");
    expect(runtimeLabel(rt("DOCKER", []))).toBe("docker");
    expect(runtimeLabel(rt("VM", []))).toBe("vm");
    expect(cloudNames(["aws", "gcp", "docker"])).toEqual(["AWS", "Google Cloud"]);
  });

  it("run on the player's accounts of their clouds", () => {
    const hosts = [host("a", "aws"), host("z", "azure"), host("e", "vmware_esxi")];
    expect(cloudAccountsFor(cloud, hosts).map((h) => h.id)).toEqual(["a"]);
    expect(cloudAccountsFor(rt("DOCKER", ["aws"]), hosts)).toEqual([]);
  });

  it("need an account of their cloud, then Terraform", () => {
    const l = lab(cloud);
    expect(setupNeeded(l, report(true), [])).toBe("Needs an account on AWS");
    expect(needsCloudAccount(l, [host("z", "azure")])).toBe(true);
    expect(setupNeeded(l, report(false), [host("a", "aws")])).toBe("Needs Terraform");
    expect(setupNeeded(l, report(true), [host("a", "aws")])).toBeNull();
    expect(needsCloudAccount(l, [host("a", "aws")])).toBe(false);
    expect(needsCloudAccount(lab(rt("DOCKER", ["aws"])), [])).toBe(false);
  });

  it("are kept by the Cloud filter, with the labs a cloud account runs", () => {
    const labs = [lab(cloud, "goat"), lab(rt("DOCKER", ["aws"]), "web"), lab(rt("VM", ["virtualbox"]), "ad")];
    const titles = (runtime: typeof NO_FILTERS.runtime) => filterLabs(labs, { ...NO_FILTERS, runtime }, () => false, new Set()).map((l) => l.title);
    expect(titles("CLOUD")).toEqual(["goat", "web"]);
    expect(titles("DOCKER")).toEqual(["web"]);
    expect(titles("all")).toEqual(["goat", "web", "ad"]);
  });

  it("start on their first account, never a default server", () => {
    const hosts = [host("esxi", "vmware_esxi"), host("a", "aws"), host("b", "aws")];
    expect(initialTarget(cloud, hosts, "esxi", false)).toEqual({ kind: "host", id: "a" });
    expect(initialTarget(cloud, [host("esxi", "vmware_esxi")], "esxi", false)).toEqual({ kind: "local" });
    const vm = rt("VM", ["vmware_esxi"]);
    expect(initialTarget(vm, hosts, "esxi", false)).toEqual({ kind: "host", id: "esxi" });
    expect(initialTarget(vm, hosts, "esxi", true)).toEqual({ kind: "local" });
  });

  it("have no machines, attack box, pause or setup to re-run", () => {
    const up = derive({ status: status({ running: true, host: "aws1", place: "cloud", provider: "aws" }) });
    expect(up.isCloud && !up.isDocker).toBe(true);
    expect(up.canShutdown || up.canPause || up.canProvision).toBe(false);
    expect(cloudView(up, { busy: false })).toBe("services");
    expect(cloudView(up, { busy: true })).toBe("none");
    expect(runtimeWord(translate as never, up)).toBe("cloud services");
  });

  it("left behind by a cut-off start, say so", () => {
    const left = derive({ status: status({ host: "aws1", provider: "aws" }) });
    expect(left.interrupted).toBe(true);
    expect(cloudView(left, { busy: false })).toBe("cloudLeftover");
    const idle = derive({ status: status({}) });
    expect(idle.interrupted).toBe(false);
    expect(cloudView(idle, { busy: false })).toBe("cloudIdle");
    expect(cloudView(derive({ backendDeploying: true }), { busy: false })).toBe("none");
  });

  it("estimate their cost from the lab's hourly figure", () => {
    const t = translate as never;
    expect(costEstimate(t, 0.02, 4)).toBe("about $0.02 an hour (about $0.08 until it auto-stops in 4 h)");
    expect(costEstimate(t, 0.5, 0)).toBe("about $0.50 an hour");
    expect(costEstimate(t, null, 4)).toBeNull();
  });
});
