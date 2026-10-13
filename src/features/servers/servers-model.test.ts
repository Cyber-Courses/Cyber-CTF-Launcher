import { describe, expect, it } from "vitest";
import type { ServerHost, ServerTest, SystemReport } from "@/lib/tauri";
import { canRunVmHere, capacityHost, hostStatus, hostsSummary, isPrivateHost, onPremHosts } from "@/features/servers/servers-model";
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

  it("reads a host's last test for its status pill", () => {
    const failed = (message: string) => ({ ok: false, message }) as ServerTest;
    expect(hostStatus(undefined)).toEqual({ result: null, needsPassword: false, tone: "muted", status: "notTested", online: false });
    expect(hostStatus("testing")).toMatchObject({ result: null, tone: "muted", status: "testing", online: false });
    expect(hostStatus(result(true))).toMatchObject({ tone: "ok", status: "online", online: true });
    expect(hostStatus(failed("No password stored for root"))).toMatchObject({ needsPassword: true, tone: "warn", status: "needsPassword", online: false });
    expect(hostStatus(failed("connection refused"))).toMatchObject({ needsPassword: false, tone: "fail", status: "unreachable", online: false });
  });

  it("knows the private LAN addresses macOS's Local Network permission governs", () => {
    for (const h of ["10.0.0.2", "192.168.1.10", "172.16.0.1", "172.31.255.1", "pve.local"]) expect(isPrivateHost(h)).toBe(true);
    for (const h of ["172.15.0.1", "172.32.0.1", "8.8.8.8", "pve.example.com"]) expect(isPrivateHost(h)).toBe(false);
  });
});
