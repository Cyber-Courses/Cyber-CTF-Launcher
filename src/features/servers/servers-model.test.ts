import { describe, expect, it } from "vitest";
import type { ServerHost, ServerTest, SystemReport } from "@/lib/tauri";
import { canRunVmHere, capacityHost, hostsSummary, onPremHosts } from "@/features/servers/servers-model";
import { failedTest } from "@/features/servers/use-server-list";

const host = (id: string, provider: ServerHost["provider"] = "proxmox") => ({ id, name: id, provider }) as ServerHost;
const result = (ok: boolean) => ({ ok }) as ServerTest;

describe("servers model", () => {
  it("leaves the AWS, Azure and GCP accounts to the Cloud page", () => {
    const hosts = [host("p"), host("e", "vmware_esxi"), host("a", "aws"), host("z", "azure"), host("g", "gcp")];
    expect(onPremHosts(hosts).map((h) => h.id)).toEqual(["p", "e"]);
  });

  it("leaves the token and key clouds to the Cloud page too", () => {
    const hosts = [host("p"), host("d", "digitalocean"), host("l", "linode"), host("o", "oci")];
    expect(onPremHosts(hosts).map((h) => h.id)).toEqual(["p"]);
  });

  it("summarises the hosts' last tests", () => {
    const hosts = [host("a"), host("b")];
    expect(hostsSummary([], {}).tone).toBe("warn");
    expect(hostsSummary(hosts, { a: "testing", b: result(true) })).toEqual({ online: 1, anyTesting: true, tone: "muted" });
    expect(hostsSummary(hosts, { a: result(true), b: result(true) }).tone).toBe("ok");
    expect(hostsSummary(hosts, { a: result(true), b: result(false) }).tone).toBe("warn");
    expect(hostsSummary(hosts, { a: result(false) })).toEqual({ online: 0, anyTesting: false, tone: "fail" });
  });

  it("shows the default host's capacity, else the first that answered", () => {
    const hosts = [host("a"), host("b"), host("c")];
    const cap = { cores: 8, memTotal: 64, memFree: 32 };
    expect(capacityHost(hosts, "b", { b: cap, c: cap })?.host.id).toBe("b");
    expect(capacityHost(hosts, "a", { a: null, c: cap })?.host.id).toBe("c");
    expect(capacityHost(hosts, null, {})).toBeNull();
  });

  it("suggests a server only when this machine can't run VM labs", () => {
    const report = (vmProviders: unknown[]) => ({ vmProviders }) as SystemReport;
    expect(canRunVmHere(report([{ remote: false, available: true, hypervisor: true }]))).toBe(true);
    expect(canRunVmHere(report([{ remote: true, available: true, hypervisor: null }]))).toBe(false);
    expect(canRunVmHere(report([{ remote: false, available: true, hypervisor: false }]))).toBe(false);
  });

  it("turns a test that couldn't run into a failed one", () => {
    expect(failedTest(new Error("timed out"))).toEqual({
      ok: false,
      reachable: false,
      authenticated: null,
      latencyMs: null,
      message: "Error: timed out",
      checks: [],
    });
  });
});
