import { describe, expect, it } from "vitest";
import { topology } from "@/features/labs/network-diagram/topology";
import { isIp, netLabel, serviceRows, serviceType, uniquePorts, type ComputerData, type Machine } from "@/features/labs/network-diagram/model";
import type { LabNetwork } from "@/lib/tauri";
import { must } from "@/components/test-support";

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

describe("topology", () => {
  it("draws one default network for a runtime that reports none, with its subnet guessed", () => {
    const t = topology([machine("web", { interfaces: [], ip: "172.18.0.4" })], [], null);
    expect(ids(t)).toEqual(["bridge-default", "up-default", "svc-web"]);
    expect(t.zones).toHaveLength(1);
    expect(t.zones[0].data.detail).toBe("172.18.0.0/16");
    // No attack box (a VM lab): no attacker node or link.
    expect(t.edges.some((e) => e.id === "e-attach")).toBe(false);
  });

  it("draws a network even with no machines at all", () => {
    const t = topology([], [], null);
    expect(ids(t)).toEqual(["bridge-default", "up-default"]);
    expect(t.zones[0].data.detail).toBe("");
  });

  it("does not write the guessed subnet into the caller's networks", () => {
    // The networks come from a status poll: a guess written into them changed the diagram's
    // layout signature between renders, re-laying the diagram out on every poll.
    const networks: LabNetwork[] = [{ name: "lan", subnet: "", internal: false }];
    const t = topology([machine("web", { interfaces: [{ network: "lan", ip: "10.9.0.3" }] })], networks, null);
    expect(t.zones[0].data.detail).toBe("10.9.0.0/16");
    expect(networks[0].subnet).toBe("");
  });

  it("gives an isolated network no uplink and marks its zone", () => {
    const t = topology([machine("db", { interfaces: [{ network: "back", ip: "10.1.0.2" }] })], [{ name: "back", subnet: "10.1.0.0/24", internal: true }], null);
    expect(ids(t)).not.toContain("up-back");
    expect(t.zones[0].data).toMatchObject({ isolated: true, detail: "10.1.0.0/24" });
  });

  it("puts a running attack box on the network it joined, with a live link", () => {
    const nets: LabNetwork[] = [
      { name: "default", subnet: "10.0.0.0/24", internal: false },
      { name: "dmz", subnet: "10.5.0.0/24", internal: false },
    ];
    const t = topology([machine("web", { interfaces: [{ network: "dmz", ip: "10.5.0.2" }] })], nets, { running: true, ip: "10.5.0.9", labNetwork: "dmz" });
    const atk = must(
      t.nodes.find((n) => n.id === "__attacker"),
      "the attack box",
    );
    expect(atk.data).toMatchObject({ subtitle: "10.5.0.9", running: true });
    expect(
      must(
        t.zones.find((z) => z.id === "zone-dmz"),
        "the dmz zone",
      ).members,
    ).toContain("__attacker");
    // The entry network comes first.
    expect(t.zones[0].id).toBe("zone-dmz");
    expect(t.edges.find((e) => e.id === "e-attach")).toMatchObject({ source: "bridge-dmz", animated: true });
  });

  it("shows a running attack box without an address as attached, and a stopped one as not", () => {
    const on = topology([machine("web")], [], { running: true, ip: "" });
    expect(
      must(
        on.nodes.find((n) => n.id === "__attacker"),
        "the attack box",
      ).data.subtitle,
    ).toBe("attached");
    const off = topology([machine("web")], [], { running: false, ip: "10.0.0.9", labNetwork: "nowhere" });
    const atk = must(
      off.nodes.find((n) => n.id === "__attacker"),
      "the attack box",
    );
    expect(atk.data).toMatchObject({ running: false });
    expect(String(atk.data.subtitle)).toMatch(/not attached/i);
    // An unknown lab network falls back to the default one, and the link is idle.
    expect(off.edges.find((e) => e.id === "e-attach")).toMatchObject({ source: "bridge-default", animated: false });
  });

  it("links a pivot sideways: from its nearest network, on into the deeper one", () => {
    const nets: LabNetwork[] = [
      { name: "default", subnet: "10.0.0.0/24", internal: false },
      { name: "inner", subnet: "10.7.0.0/24", internal: true },
    ];
    const t = topology(
      [
        machine("jump", {
          interfaces: [
            { network: "default", ip: "10.0.0.5" },
            { network: "inner", ip: "10.7.0.5" },
          ],
        }),
        machine("dc", { interfaces: [{ network: "inner", ip: "10.7.0.10" }] }),
      ],
      nets,
      null,
    );
    expect(t.edges.find((e) => e.id === "e-default-jump")).toMatchObject({ source: "zone-default", target: "svc-jump", sourceHandle: "e", targetHandle: "w" });
    expect(t.edges.find((e) => e.id === "e-jump-inner")).toMatchObject({ source: "svc-jump", target: "zone-inner", data: { label: "10.7.0.5" } });
    // A pivot belongs to no zone.
    expect(t.zones.flatMap((z) => z.members)).not.toContain("svc-jump");
    expect(t.zones.map((z) => z.id)).toEqual(["zone-default", "zone-inner"]);
  });

  it("leaves the provisioning controller off the map", () => {
    const t = topology([machine("web"), machine("isoloom-controller", { infra: true })], [], null);
    expect(ids(t)).not.toContain("svc-isoloom-controller");
  });

  it("publishes each port once, from its service's box when a service shows it", () => {
    const m = machine("api", {
      image: "node:20",
      ports: [
        { published: 8080, target: 80 },
        { published: 8080, target: 80 },
        { published: 9000, target: 9000 },
        { published: 0, target: 22 },
      ],
      services: [{ name: "http", kind: "web", ports: [80] }],
    });
    const t = topology([m], [], null);
    expect(ids(t).filter((id) => id.startsWith("hp-"))).toEqual(["hp-api-8080", "hp-api-9000"]);
    expect(t.edges.find((e) => e.id === "e-hp-api-8080")?.sourceHandle).toBe("pub-8080");
    // 9000 belongs to no declared service: it keeps the card's bottom handle.
    expect(t.edges.find((e) => e.id === "e-hp-api-9000")?.sourceHandle).toBeUndefined();
    expect(data(t, "svc-api").rows).toEqual([{ title: "http", type: "web", label: "web", ports: [{ published: 8080, target: 80 }] }]);
  });

  it("guesses a missing network for a machine whose interfaces name an unreported one", () => {
    const t = topology([machine("web", { interfaces: [{ network: "extra", ip: "pending" }] })], [{ name: "default", subnet: "", internal: false }], null);
    expect(t.zones.map((z) => z.id)).toEqual(["zone-default", "zone-extra"]);
    // No IP to guess from: the subnet stays blank.
    expect(t.zones[1].data.detail).toBe("");
  });

  it("names a stopped machine's image by its look when it has none", () => {
    const t = topology([machine("redis-cache", { image: "", state: "exited" })], [], null);
    expect(data(t, "svc-redis-cache")).toMatchObject({ image: "cache", running: false, state: "exited", type: "cache" });
  });
});

describe("model helpers", () => {
  it("tells an address from a placeholder", () => {
    expect(isIp("10.0.0.1")).toBe(true);
    expect(isIp("pending")).toBe(false);
  });

  it("calls compose's default network the lab network", () => {
    expect(netLabel("default")).toBe("lab");
    expect(netLabel("dmz")).toBe("dmz");
  });

  it("drops Docker's duplicate port per address family", () => {
    expect(
      uniquePorts([
        { published: 1, target: 2 },
        { published: 1, target: 2 },
        { published: 1, target: 3 },
      ]),
    ).toHaveLength(2);
  });

  it.each([
    ["mariadb:11", "x", "database"],
    ["x", "app-db", "database"],
    ["valkey", "x", "cache"],
    ["nginx", "x", "web"],
    ["x", "rabbitmq", "worker"],
    ["busybox", "thing", "service"],
  ])("types %s / %s as %s", (image, name, type) => {
    expect(serviceType(image, name)).toBe(type);
  });

  it("maps declared kinds onto known looks, else a plain service", () => {
    const m = machine("x", {
      services: [
        { name: "a", kind: "SSH", ports: [22] },
        { name: "b", kind: "ftp", ports: [21] },
        { name: "c", kind: "", ports: [] },
      ],
    });
    const rows = serviceRows(m, [{ published: 2222, target: 22 }], "service");
    expect(rows.map((r) => [r.type, r.label])).toEqual([
      ["ssh", "SSH"],
      ["service", "ftp"],
      ["service", "service"],
    ]);
    // A declared port nobody publishes is still listed, unpublished.
    expect(rows[1].ports).toEqual([{ published: 0, target: 21 }]);
    expect(rows[0].ports).toEqual([{ published: 2222, target: 22 }]);
  });
});

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
