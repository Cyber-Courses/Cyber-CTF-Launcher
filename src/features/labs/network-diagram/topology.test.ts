import { describe, expect, it } from "vitest";
import { topology } from "@/features/labs/network-diagram/topology";
import { isIp, netLabel, serviceRows, serviceType, uniquePorts, type ComputerData, type Machine } from "@/features/labs/network-diagram/model";
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
    const atk = t.nodes.find((n) => n.id === "__attacker")!;
    expect(atk.data).toMatchObject({ subtitle: "10.5.0.9", running: true });
    expect(t.zones.find((z) => z.id === "zone-dmz")!.members).toContain("__attacker");
    // The entry network comes first.
    expect(t.zones[0].id).toBe("zone-dmz");
    expect(t.edges.find((e) => e.id === "e-attach")).toMatchObject({ source: "bridge-dmz", animated: true });
  });

  it("shows a running attack box without an address as attached, and a stopped one as not", () => {
    const on = topology([machine("web")], [], { running: true, ip: "" });
    expect(on.nodes.find((n) => n.id === "__attacker")!.data.subtitle).toBe("attached");
    const off = topology([machine("web")], [], { running: false, ip: "10.0.0.9", labNetwork: "nowhere" });
    const atk = off.nodes.find((n) => n.id === "__attacker")!;
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
