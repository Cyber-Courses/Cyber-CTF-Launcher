import { describe, expect, it } from "vitest";
import { topology } from "@/features/labs/network-diagram/topology";
import { serviceRows, serviceType, uniquePorts, type ComputerData, type Machine } from "@/features/labs/network-diagram/model";
import type { LabNetwork } from "@/lib/tauri";

const machine = (name: string, over: Partial<Machine> = {}): Machine => ({
  name,
  state: "running",
  image: `${name}:latest`,
  ip: "10.0.0.2",
  ports: [],
  interfaces: [{ network: "default", ip: "10.0.0.2" }],
  services: [],
  infra: false,
  ...over,
});
const net = (name: string, over: Partial<LabNetwork> = {}): LabNetwork => ({ name, subnet: "", internal: false, ...over });

const ids = (t: ReturnType<typeof topology>) => t.nodes.map((n) => n.id);
const data = (t: ReturnType<typeof topology>, id: string) => t.nodes.find((n) => n.id === id)?.data as ComputerData;

describe("topology: where the attack box enters", () => {
  it("an attack box naming a network that was not reported joins the default one, else the first reported", () => {
    const onDefault = topology([machine("web")], [], { running: true, ip: "10.0.0.9", labNetwork: "ghost" });
    expect(onDefault.edges.find((e) => e.id === "e-attach")?.source).toBe("bridge-default");
    const noDefault = topology([machine("web", { interfaces: [{ network: "lan", ip: "10.1.0.2" }] })], [net("lan"), net("dmz")], {
      running: true,
      ip: "",
      labNetwork: "ghost",
    });
    expect(noDefault.edges.find((e) => e.id === "e-attach")?.source).toBe("bridge-lan");
    expect(noDefault.zones[0].members).toContain("__attacker");
  });

  it("a network no machine bridges to from the entry is drawn last, whatever its name", () => {
    const t = topology([machine("web")], [net("aaa"), net("default")], null);
    expect(t.zones.map((z) => z.id)).toEqual(["zone-default", "zone-aaa"]);
    const pivot = machine("fw", {
      interfaces: [
        { network: "default", ip: "10.0.0.3" },
        { network: "aaa", ip: "10.9.0.1" },
      ],
    });
    expect(topology([machine("web"), pivot], [net("aaa"), net("default")], null).zones.map((z) => z.id)).toEqual(["zone-default", "zone-aaa"]);
  });
});

describe("topology: addresses and ports", () => {
  it("an IPv6-only machine leaves the subnet unguessed", () => {
    const t = topology([machine("web", { ip: "fd00::2", interfaces: [{ network: "default", ip: "fd00::2" }] })], [], null);
    expect(t.zones[0].data.detail).toBe("");
  });

  it("a machine with no interfaces and no address sits on the first reported network", () => {
    const t = topology([machine("web", { interfaces: [], ip: "" })], [net("lan"), net("dmz")], null);
    expect(t.zones.find((z) => z.id === "zone-lan")?.members).toContain("svc-web");
    expect(data(t, "svc-web").ifaces).toEqual([{ network: "lan", ip: "" }]);
  });

  it("a port published on both address families gets one host-port node and one link", () => {
    const web = machine("web", {
      ports: [
        { published: 8080, target: 80 },
        { published: 8080, target: 80 },
      ],
    });
    const t = topology([web], [], null);
    expect(ids(t).filter((id) => id === "hp-web-8080")).toHaveLength(1);
    expect(t.edges.filter((e) => e.id === "e-hp-web-8080")).toHaveLength(1);
  });

  it("a declared service port the container does not expose is drawn unpublished, with no host port", () => {
    const api = machine("api", { services: [{ name: "api", kind: "web", ports: [9999] }], ports: [] });
    const t = topology([api], [], null);
    expect(data(t, "svc-api").rows[0].ports).toEqual([{ published: 0, target: 9999 }]);
    expect(ids(t).some((id) => id.startsWith("hp-"))).toBe(false);
  });

  it("two interfaces on the same network get distinct edge ids", () => {
    // A machine with two addresses on one segment (two NICs of a VM on the same network) goes
    // down the pivot path, and both links get the id `e-<network>-<machine>`: React Flow keeps
    // one and React warns about the duplicate key. Not reachable from Docker (one address per
    // network). Suggested fix: suffix the id with the interface index.
    const twin = machine("fw", {
      interfaces: [
        { network: "default", ip: "10.0.0.3" },
        { network: "default", ip: "10.0.0.4" },
      ],
    });
    const edgeIds = topology([twin], [], null).edges.map((e) => e.id);
    expect(new Set(edgeIds).size).toBe(edgeIds.length);
  });
});

describe("model helpers: edges", () => {
  it("reads the image and the name together, the database look winning over the cache one", () => {
    expect(serviceType("redis:7", "users-db")).toBe("database");
    expect(serviceType("", "")).toBe("service");
    expect(serviceType("node:20", "")).toBe("web");
    expect(serviceType("busybox", "seed-data")).toBe("worker");
  });

  it("keeps the same host port forwarded to two targets", () => {
    expect(
      uniquePorts([
        { published: 8080, target: 80 },
        { published: 8080, target: 81 },
      ]),
    ).toHaveLength(2);
  });

  it("a declared kind outside the known looks keeps its own word on the row", () => {
    const m = machine("x", { services: [{ name: "ldap", kind: "Directory", ports: [389] }] });
    const [row] = serviceRows(m, [{ published: 0, target: 389 }], "service");
    expect(row.type).toBe("service");
    expect(row.label).toBe("Directory");
  });
});
