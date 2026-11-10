import { describe, expect, it } from "vitest";
import type { LabStatus, ServerCheck, ServerHost, ServerTest } from "@/lib/tauri";
import type { Lab } from "@/features/labs/use-labs";
import { budgetedAccounts, cloudAccounts, credentialOf, labsOnAccounts } from "@/features/cloud/cloud-model";
import { outcomeOf } from "@/features/servers/test-outcome";

const host = (id: string, provider: ServerHost["provider"], patch: Partial<ServerHost> = {}) => ({ id, name: id, provider, ...patch }) as ServerHost;

describe("cloud model", () => {
  it("keeps the cloud accounts among the saved hosts", () => {
    const hosts = ["aws", "azure", "gcp", "digitalocean", "linode", "oci", "proxmox", "vmware_esxi"].map((p) => host(p, p as ServerHost["provider"]));
    expect(cloudAccounts(hosts).map((h) => h.id)).toEqual(["aws", "azure", "gcp", "digitalocean", "linode", "oci"]);
  });

  it("guards the budgets the backend lists, whatever a host's own fields say", () => {
    // Which account has a budget is the backend's rule (`ServerList.budgets`): an Azure limit, a
    // limit of 0 or none is left out there, not here.
    const hosts = [host("a", "aws", { monthlyLimit: 50 }), host("b", "aws", { monthlyLimit: 0 }), host("c", "azure", { monthlyLimit: 50 })];
    expect(budgetedAccounts(hosts, { a: 50 }).map((b) => [b.host.id, b.limit])).toEqual([["a", 50]]);
    expect(budgetedAccounts(hosts, {})).toEqual([]);
  });

  it("finds the labs running on these accounts", () => {
    const lab = (id: string, runtime = true) => ({ id, runtime: runtime ? { runtime: "VM" } : null }) as Lab;
    const status = (running: boolean, hostId: string | null, name = hostId) => ({ running, host: name, hostId }) as LabStatus;
    const labs = [lab("on-aws"), lab("on-server"), lab("same-name"), lab("stopped"), lab("local"), lab("no-runtime", false)];
    const statuses = {
      "on-aws": status(true, "aws"),
      "on-server": status(true, "pve"),
      // A server saved under the account's name is still no cloud account: matched by id.
      "same-name": status(true, "pve", "aws"),
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
