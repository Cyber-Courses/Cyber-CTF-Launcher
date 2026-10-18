import { describe, expect, it } from "vitest";
import type { LabStatus, ServerCheck, ServerHost, ServerTest } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import { budgetedAccounts, cloudAccounts, credentialOf, isOverBudget, labsOnAccounts, outcomeOf } from "@/features/cloud/cloud-model";

const host = (id: string, provider: ServerHost["provider"], patch: Partial<ServerHost> = {}) => ({ id, name: id, provider, ...patch }) as ServerHost;

describe("cloud model", () => {
  it("keeps the cloud accounts among the saved hosts", () => {
    const hosts = ["aws", "azure", "gcp", "digitalocean", "linode", "oci", "proxmox", "vmware_esxi"].map((p) => host(p, p as ServerHost["provider"]));
    expect(cloudAccounts(hosts).map((h) => h.id)).toEqual(["aws", "azure", "gcp", "digitalocean", "linode", "oci"]);
  });

  it("is over budget once the spend reaches the limit", () => {
    const capped = host("a", "aws", { monthlyLimit: 50 });
    expect(isOverBudget(capped, 50)).toBe(true);
    expect(isOverBudget(capped, 49.99)).toBe(false);
    expect(isOverBudget(capped, null)).toBe(false);
    expect(isOverBudget(host("b", "aws"), 1000)).toBe(false);
  });

  it("treats a budget of 0 as no limit", () => {
    const uncapped = host("a", "aws", { monthlyLimit: 0 });
    expect(isOverBudget(uncapped, 0)).toBe(false);
    expect(isOverBudget(uncapped, 12)).toBe(false);
    expect(budgetedAccounts([uncapped])).toEqual([]);
  });

  it("guards the budget of AWS accounts that set one", () => {
    const hosts = [host("a", "aws", { monthlyLimit: 50 }), host("b", "aws"), host("c", "azure", { monthlyLimit: 50 })];
    expect(budgetedAccounts(hosts).map((b) => [b.host.id, b.limit])).toEqual([["a", 50]]);
  });

  it("finds the labs running on these accounts", () => {
    const lab = (id: string, runtime = true) => ({ id, runtime: runtime ? { runtime: "VM" } : null }) as Lab;
    const status = (running: boolean, host: string | null) => ({ running, host }) as LabStatus;
    const labs = [lab("on-aws"), lab("on-server"), lab("stopped"), lab("local"), lab("no-runtime", false)];
    const statuses = {
      "on-aws": status(true, "aws"),
      "on-server": status(true, "pve"),
      stopped: status(false, "aws"),
      local: status(true, null),
      "no-runtime": status(true, "aws"),
    };
    expect(labsOnAccounts(labs, statuses, [host("aws", "aws")]).map((l) => l.id)).toEqual(["on-aws"]);
  });

  it("names the AWS CLI profile an account signs in with", () => {
    expect(credentialOf(host("a", "aws", { useCliCreds: true, awsProfile: "lab" }))).toEqual({ kind: "cliProfile", profile: "lab" });
    expect(credentialOf(host("a", "aws", { useCliCreds: true, awsProfile: null }))).toEqual({ kind: "cliCredentials" });
  });

  it("reads a test's outcome from its checks", () => {
    const test = (ok: boolean, states: ServerCheck["state"][]) => ({ ok, checks: states.map((state) => ({ name: state, state, detail: "" })) }) as ServerTest;
    expect(outcomeOf(test(false, ["ok"]))).toBe("fail");
    expect(outcomeOf(test(true, ["ok", "warn"]))).toBe("warn");
    expect(outcomeOf(test(true, []))).toBe("ok");
  });
});
