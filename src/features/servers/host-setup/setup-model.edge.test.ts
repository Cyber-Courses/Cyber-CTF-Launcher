import { describe, expect, it } from "vitest";
import type { ServerHostInput, SystemReport } from "@/lib/tauri";
import { EMPTY_CLOUD, EMPTY_HOST } from "@/features/servers/host-setup/constants";
import { connectionReady, machineTools, saveName, setupSteps, withProvider } from "@/features/servers/host-setup/setup-model";

const form = (patch: Partial<ServerHostInput>): ServerHostInput => ({ ...EMPTY_HOST, ...patch });
const cloud = (patch: Partial<ServerHostInput>): ServerHostInput => ({ ...EMPTY_CLOUD, ...patch });
const tool = (installed: boolean) => ({ installed, version: null });
const report = (patch: Partial<SystemReport> = {}) =>
  ({
    vagrant: tool(true),
    terraform: tool(true),
    ovftool: tool(true),
    cloudClis: { aws: tool(true), azure: tool(false), gcloud: tool(true) },
    vmProviders: [],
    ...patch,
  }) as SystemReport;

describe("connectionReady: edges", () => {
  it("whitespace-only address or user does not count, for a server or a cloud", () => {
    expect(connectionReady(form({ provider: "vmware_esxi", host: "   ", username: "root", password: "pw" }), false)).toBe(false);
    expect(connectionReady(form({ provider: "vmware_esxi", host: "10.0.0.5", username: " ", password: "pw" }), false)).toBe(false);
    expect(connectionReady(form({ provider: "vmware_esxi", host: " 10.0.0.5 ", username: " root ", password: "pw" }), false)).toBe(true);
    expect(connectionReady(cloud({ provider: "oci", host: "eu-frankfurt-1", username: "\t" }), false)).toBe(false);
  });

  it("when editing, the stored secret stands in for every provider that needs one", () => {
    expect(connectionReady(cloud({ provider: "aws", useCliCreds: false, host: "eu-west-3", username: "AKIA", password: null }), true)).toBe(true);
    expect(connectionReady(cloud({ provider: "aws", useCliCreds: false, host: "eu-west-3", username: "AKIA", password: null }), false)).toBe(false);
    expect(connectionReady(cloud({ provider: "linode", host: "eu-central", password: null }), true)).toBe(true);
    expect(connectionReady(cloud({ provider: "linode", host: "eu-central", password: null }), false)).toBe(false);
  });
});

describe("withProvider: edges", () => {
  it("replaces a default name even with spaces around it, keeps one that differs in case or wording", () => {
    expect(withProvider(cloud({ name: "  AWS  " }), "gcp").name).toBe("GCP");
    expect(withProvider(cloud({ name: "aws" }), "gcp").name).toBe("aws");
    expect(withProvider(cloud({ name: "Work account" }), "gcp").name).toBe("Work account");
    expect(withProvider(cloud({ name: "   " }), "oci").name).toBe("Oracle Cloud");
  });

  it("the AWS budget and profile travel only to AWS, and the GCP organisation never travels", () => {
    const aws = cloud({ provider: "aws", monthlyLimit: 50, awsProfile: "work", node: "org-1" });
    expect(withProvider(aws, "azure")).toMatchObject({ monthlyLimit: null, awsProfile: null, node: null, useCliCreds: false });
    expect(withProvider(aws, "aws")).toMatchObject({ monthlyLimit: 50, awsProfile: null, node: null, useCliCreds: true });
  });
});

describe("setupSteps: edges", () => {
  it("editing skips choosing and the tools for every provider, and only AWS keeps its account step", () => {
    expect(setupSteps("proxmox", true)).toEqual(["connection", "placement", "test"]);
    expect(setupSteps("oci", true)).toEqual(["credentials", "options", "test"]);
    expect(setupSteps("aws", true)).toEqual(["account", "credentials", "options", "test"]);
    expect(setupSteps("oci", false)).toEqual(["provider", "tools", "credentials", "options", "test"]);
  });
});

describe("machineTools and saveName: edges", () => {
  it("nothing is ready before the machine report is in", () => {
    for (const p of ["vmware_esxi", "proxmox", "aws", "digitalocean"] as const) expect(machineTools(p, null).toolsOk, p).toBe(false);
  });

  it("a token cloud and Proxmox need Terraform alone; a CLI cloud its CLI too", () => {
    expect(machineTools("digitalocean", report())).toMatchObject({ toolsOk: true, cloudHasCli: false, cloudCliTool: undefined });
    expect(machineTools("proxmox", report())).toMatchObject({ toolsOk: true });
    expect(machineTools("proxmox", report({ terraform: tool(false) })).toolsOk).toBe(false);
    expect(machineTools("azure", report())).toMatchObject({ toolsOk: false, cloudCliOk: false, cloudDep: "azurecli" });
    expect(machineTools("gcp", report())).toMatchObject({ toolsOk: true, cloudDep: "gcloud" });
  });

  it("saves a cloud under its short name and a server under its trimmed address when no name is typed", () => {
    expect(saveName(cloud({ provider: "oci", name: " ", host: "eu-frankfurt-1" }))).toBe("Oracle Cloud");
    expect(saveName(form({ provider: "proxmox", name: "", host: " pve.local " }))).toBe("pve.local");
    expect(saveName(form({ provider: "proxmox", name: " Home lab ", host: "pve.local" }))).toBe("Home lab");
  });
});
