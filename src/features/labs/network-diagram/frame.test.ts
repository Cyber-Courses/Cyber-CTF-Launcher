import { describe, expect, it } from "vitest";

import { ZONE_LABEL, frameLayout, zoneLabelWidth } from "@/features/labs/network-diagram/frame";
import { layout } from "@/features/labs/network-diagram/layout";
import { topology } from "@/features/labs/network-diagram/topology";
import type { Machine } from "@/features/labs/network-diagram/model";
import { must } from "@/components/test-support";

const machine = (name: string, ip: string, network = "default"): Machine => ({
  name,
  state: "running",
  image: `${name}:latest`,
  ip,
  ports: [],
  interfaces: [{ network, ip }],
  services: [],
  infra: false,
});

async function framed(machines: Machine[], networks: { name: string; subnet: string; internal: boolean }[]) {
  const topo = topology(machines, networks, null);
  const laid = await layout(topo, () => ({ w: 220, h: 140 }));
  return { topo, laid, ...frameLayout(topo, laid.boxes, laid.routes, laid.bounds) };
}

describe("frameLayout", () => {
  it("frames each network as a zone with its bridge centred on the top edge", async () => {
    const { topo, laid, zones } = await framed(
      [machine("web", "10.0.0.2"), machine("db", "10.0.0.3")],
      [{ name: "default", subnet: "10.0.0.0/24", internal: false }],
    );
    expect(zones.length).toBeGreaterThan(0);
    expect(zones).toHaveLength(topo.zones.length);
    for (const zone of zones) {
      const members = must(
        topo.zones.find((z) => z.id === zone.id),
        `the ${zone.id} zone`,
      ).members;
      const bridge = must(laid.boxes.get(members[0]), "the zone's bridge");
      const width = Number(zone.style?.width);
      expect(bridge.x + bridge.w / 2).toBeCloseTo(zone.position.x + width / 2);
      expect(zone.position.y).toBeCloseTo(bridge.y + bridge.h / 2);
    }
  });

  it("moves the bridge right of a label that would reach it, inside the frame", async () => {
    const { topo, laid, zones } = await framed(
      [machine("web", "10.0.0.2"), machine("db", "10.0.0.3")],
      [{ name: "default", subnet: "10.0.0.0/24", internal: true }],
    );
    const [zone] = zones;
    const z = topo.zones[0];
    const bridge = must(laid.boxes.get(z.members[0]), "the zone's bridge");
    const labelEnd = zone.position.x + ZONE_LABEL.left + zoneLabelWidth(z.data);
    expect(bridge.x + bridge.w / 2).toBeGreaterThan(zone.position.x + Number(zone.style?.width) / 2);
    expect(bridge.x).toBeGreaterThan(labelEnd);
    expect(bridge.x + bridge.w).toBeLessThanOrEqual(zone.position.x + Number(zone.style?.width));
    // Its links still leave from its centre.
    const bus = topo.edges.filter((e) => e.source === z.members[0]).map((e) => must(laid.routes.get(e.id), `the route of ${e.id}`)[0]);
    bus.forEach((p) => {
      expect(p.x).toBeCloseTo(bridge.x + bridge.w / 2);
    });
  });

  it("widens the bounds to the widest zone", async () => {
    const { zones, bounds } = await framed([machine("a", "10.0.0.2")], [{ name: "a-network-with-a-long-name", subnet: "10.0.0.0/24", internal: true }]);
    for (const zone of zones) {
      expect(zone.position.x).toBeGreaterThanOrEqual(bounds.x);
      expect(zone.position.x + Number(zone.style?.width)).toBeLessThanOrEqual(bounds.x + bounds.w + 1e-9);
    }
  });

  it("runs uplinks straight up from their bridge", async () => {
    const { topo, laid } = await framed([machine("web", "10.0.0.2")], [{ name: "default", subnet: "10.0.0.0/24", internal: false }]);
    const uplinks = topo.edges.filter((e) => e.source.startsWith("up-"));
    expect(uplinks.length).toBeGreaterThan(0);
    for (const e of uplinks) {
      const [from, to] = must(laid.routes.get(e.id), `the route of ${e.id}`);
      expect(from.x).toBe(to.x);
      expect(from.y).toBeLessThan(to.y);
    }
  });
});
