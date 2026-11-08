import { describe, expect, it } from "vitest";
import type { ServerHostInput, ServerTest, SystemReport } from "@/lib/tauri";
import { EMPTY_CLOUD, EMPTY_HOST } from "@/features/servers/host-setup/constants";
import {
  DEFAULT_REGION,
  REGIONS,
  cloudCli,
  connectionReady,
  isCloud,
  machineTools,
  initialSignIn,
  initialSshKey,
  savePayload,
  saveName,
  serverReady,
  setupSteps,
  setupTitle,
  startingSetup,
  withProvider,
} from "@/features/servers/host-setup/setup-model";

const form = (patch: Partial<ServerHostInput>): ServerHostInput => ({ ...EMPTY_HOST, ...patch });
const cloud = (patch: Partial<ServerHostInput>): ServerHostInput => ({ ...EMPTY_CLOUD, ...patch });
const tool = (installed: boolean) => ({ installed, version: null });
const report = (patch: Partial<SystemReport> = {}) =>
  ({
    vagrant: tool(true),
    terraform: tool(true),
    ovftool: tool(true),
    cloudClis: { aws: tool(true), azure: tool(false), gcloud: tool(true) },
    vmProviders: [{ provider: "vmware_esxi", pluginInstalled: true }],
    ...patch,
  }) as SystemReport;

describe("setupSteps", () => {
  it("walks a server through hypervisor, tools, connection and placement; Proxmox describes the server before signing in", () => {
    expect(setupSteps("proxmox", false)).toEqual(["hypervisor", "tools", "server", "connection", "placement", "test"]);
    expect(setupSteps("proxmox", true)).toEqual(["server", "connection", "placement", "test"]);
    expect(setupSteps("vmware_esxi", false)).toEqual(["hypervisor", "tools", "connection", "placement", "test"]);
    expect(setupSteps("vmware_esxi", true)).toEqual(["connection", "placement", "test"]);
  });

  it("asks only AWS how to connect, and skips choosing when editing", () => {
    expect(setupSteps("aws", false)).toEqual(["provider", "tools", "account", "credentials", "options", "test"]);
    expect(setupSteps("azure", false)).toEqual(["provider", "tools", "credentials", "options", "test"]);
    expect(setupSteps("aws", true)).toEqual(["account", "credentials", "options", "test"]);
    expect(setupSteps("oci", true)).toEqual(["credentials", "options", "test"]);
    // Opened on a chosen provider (a Connect row): no choosing, straight to its tools.
    expect(setupSteps("azure", false, true)).toEqual(["tools", "credentials", "options", "test"]);
    expect(setupSteps("aws", false, true)).toEqual(["tools", "account", "credentials", "options", "test"]);
    expect(setupSteps("proxmox", false, true)).toEqual(["hypervisor", "tools", "server", "connection", "placement", "test"]);
  });
});

describe("Proxmox sign-in", () => {
  const key = { path: " /Users/me/.ssh/id_ed25519 ", user: " root ", port: 22 };

  it("opens a new setup on the SSH key, a saved one on how it signs in", () => {
    expect(initialSignIn(form({}))).toBe("sshKey");
    expect(initialSignIn(form({ provider: "vmware_esxi" }))).toBe("password");
    expect(initialSignIn(form({ id: "a", username: "root@pam" }))).toBe("password");
    expect(initialSignIn(form({ id: "a", username: "root@pam!ci" }))).toBe("token");
    expect(initialSignIn(form({ id: "a", username: "root@pam!cyberctf-1", sshKey: key }))).toBe("sshKey");
    expect(initialSshKey(form({}))).toEqual({ path: "", user: "root", port: 22 });
    expect(initialSshKey(form({ sshKey: key }))).toBe(key);
  });

  it("needs the address and a valid API port on the server step", () => {
    expect(serverReady(form({ host: "10.0.0.50" }))).toBe(true);
    expect(serverReady(form({ host: " " }))).toBe(false);
    expect(serverReady(form({ host: "pve", port: 70000 }))).toBe(false);
  });

  it("needs a key, a user and a port to sign in with a key; no password", () => {
    const v = form({ host: "10.0.0.50" });
    expect(connectionReady(v, false, key)).toBe(true);
    expect(connectionReady(v, false, { ...key, path: " " })).toBe(false);
    expect(connectionReady(v, false, { ...key, user: "" })).toBe(false);
    expect(connectionReady(v, false, { ...key, port: Number.NaN })).toBe(false);
    expect(connectionReady(v, false, { ...key, port: 0 })).toBe(false);
    // Without a key it is the usual user and password.
    expect(connectionReady(v, false)).toBe(false);
  });

  it("sends the key with a key save, and none with a password or token one", () => {
    const v = form({ host: "10.0.0.50", username: "root@pam", password: "pw" });
    expect(savePayload(v, "sshKey", key)).toMatchObject({
      name: "10.0.0.50",
      username: "",
      password: null,
      sshKey: { path: "/Users/me/.ssh/id_ed25519", user: "root", port: 22 },
    });
    expect(savePayload(v, "password", key)).toMatchObject({ username: "root@pam", password: "pw", sshKey: null });
    expect(savePayload(v, "token", key).sshKey).toBeNull();
    expect(savePayload({ ...v, provider: "vmware_esxi" }, "sshKey", key).sshKey).toBeNull();
  });
});

describe("startingSetup", () => {
  it("starts a server, a cloud account, or the named cloud's account with the choice made", () => {
    expect(startingSetup(null, null)).toEqual({ form: EMPTY_HOST, providerChosen: false });
    expect(startingSetup("server", "azure")).toEqual({ form: EMPTY_HOST, providerChosen: false });
    expect(startingSetup("cloud", null)).toEqual({ form: EMPTY_CLOUD, providerChosen: false });
    const azure = startingSetup("cloud", "azure");
    expect(azure.providerChosen).toBe(true);
    expect(azure.form).toMatchObject({ provider: "azure", host: DEFAULT_REGION.azure, useCliCreds: false, name: "Azure" });
    expect(startingSetup("cloud", "gcp").form.provider).toBe("gcp");
  });

  it("starts a plain new account, choice open, on a provider it doesn't know", () => {
    expect(startingSetup("cloud", "proxmox")).toEqual({ form: EMPTY_CLOUD, providerChosen: false });
    expect(startingSetup("cloud", "")).toEqual({ form: EMPTY_CLOUD, providerChosen: false });
    // A fresh copy each time: the window keeps its own form.
    expect(startingSetup("cloud", null).form).not.toBe(EMPTY_CLOUD);
  });
});

describe("withProvider", () => {
  it("starts the chosen cloud fresh, in its default region", () => {
    const aws = form({ ...EMPTY_CLOUD, username: "AKIA1", password: "secret", awsProfile: "work", node: "org", monthlyLimit: 50 });
    const gcp = withProvider(aws, "gcp");
    expect(gcp).toMatchObject({
      provider: "gcp",
      name: "GCP",
      host: "europe-west1",
      username: "",
      password: null,
      useCliCreds: false,
      awsProfile: null,
      node: null,
    });
    // The budget is AWS-only.
    expect(gcp.monthlyLimit).toBeNull();
    expect(withProvider(aws, "aws").monthlyLimit).toBe(50);
    for (const id of Object.keys(DEFAULT_REGION) as (keyof typeof DEFAULT_REGION)[]) {
      expect(REGIONS[id].some(([code]) => code === DEFAULT_REGION[id])).toBe(true);
    }
  });

  it("keeps a name the user typed, replaces a default one", () => {
    expect(withProvider(form({ name: "Lab account" }), "azure").name).toBe("Lab account");
    expect(withProvider(form({ name: " AWS " }), "linode").name).toBe("Linode");
    expect(withProvider(form({ name: "" }), "oci").name).toBe("Oracle Cloud");
  });
});

describe("connectionReady", () => {
  it("needs an address, a user and a secret for a server (kept when editing)", () => {
    expect(connectionReady(form({ host: "pve", username: "root@pam", password: "x" }), false)).toBe(true);
    expect(connectionReady(form({ host: "pve", username: "root@pam" }), false)).toBe(false);
    expect(connectionReady(form({ host: "pve", username: "root@pam" }), true)).toBe(true);
    expect(connectionReady(form({ host: " ", username: "root@pam", password: "x" }), false)).toBe(false);
  });

  it("follows each cloud's way of signing in", () => {
    expect(connectionReady(form({ provider: "digitalocean", host: "fra1", password: "dop_v1" }), false)).toBe(true);
    expect(connectionReady(form({ provider: "linode", host: "eu-central" }), false)).toBe(false);
    expect(connectionReady(form({ provider: "azure", host: "swedencentral", username: "sub" }), false)).toBe(true);
    expect(connectionReady(form({ provider: "oci", host: "eu-frankfurt-1" }), false)).toBe(false);
    expect(connectionReady(form({ provider: "aws", host: "eu-west-3", useCliCreds: true }), false)).toBe(true);
    expect(connectionReady(form({ provider: "aws", host: "eu-west-3", useCliCreds: false, username: "AKIA" }), false)).toBe(false);
  });
});

describe("machineTools", () => {
  it("needs Vagrant, its ESXi plugin and OVF Tool for ESXi", () => {
    expect(machineTools("vmware_esxi", report()).toolsOk).toBe(true);
    expect(machineTools("vmware_esxi", report({ ovftool: tool(false) })).toolsOk).toBe(false);
    expect(machineTools("vmware_esxi", null).toolsOk).toBe(false);
  });

  it("needs Terraform, plus the cloud's CLI when it has one", () => {
    expect(machineTools("proxmox", report({ terraform: tool(false) })).toolsOk).toBe(false);
    expect(machineTools("aws", report()).toolsOk).toBe(true);
    expect(machineTools("azure", report())).toMatchObject({ cloudHasCli: true, cloudCliOk: false, cloudDep: "azurecli", toolsOk: false });
    expect(machineTools("linode", report())).toMatchObject({ cloudHasCli: false, toolsOk: true });
  });

  it("names each cloud's CLI", () => {
    expect(cloudCli("gcp")).toEqual({ has: true, dependency: "gcloud", key: "gcloud" });
    expect(cloudCli("oci").has).toBe(false);
  });
});

describe("saveName", () => {
  it("falls back to the cloud's name or the address", () => {
    expect(saveName(form({ name: "  Home lab ", host: "pve" }))).toBe("Home lab");
    expect(saveName(form({ name: " ", host: " 10.0.0.2 " }))).toBe("10.0.0.2");
    expect(saveName(form({ provider: "gcp", name: "", host: "europe-west1" }))).toBe("GCP");
    expect(isCloud("proxmox")).toBe(false);
  });
});

describe("setupTitle", () => {
  const base = { saved: null, test: null, editing: false, cloud: false, initialName: "pve" };
  it("names what is being set up, then what was saved and whether it connected", () => {
    expect(setupTitle(base)).toEqual({ key: "host", name: "" });
    expect(setupTitle({ ...base, cloud: true })).toEqual({ key: "cloud", name: "" });
    expect(setupTitle({ ...base, editing: true })).toEqual({ key: "edit", name: "pve" });
    const saved = { name: "Home lab" };
    expect(setupTitle({ ...base, saved, test: "testing" })).toEqual({ key: "connected", name: "Home lab" });
    expect(setupTitle({ ...base, saved, test: { ok: true } as ServerTest })).toEqual({ key: "connected", name: "Home lab" });
    expect(setupTitle({ ...base, editing: true, saved, test: { ok: false } as ServerTest })).toEqual({ key: "savedNotConnected", name: "Home lab" });
  });
});

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
    expect(setupSteps("proxmox", true)).toEqual(["server", "connection", "placement", "test"]);
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
