import { describe, expect, it } from "vitest";

import { ZONE_LABEL, frameLayout, zoneLabelWidth } from "@/features/labs/network-diagram/frame";
import { PORT_TAB_SLOT, layout } from "@/features/labs/network-diagram/layout";
import { topology } from "@/features/labs/network-diagram/topology";
import { BRIDGE, type Attacker, type Machine, type Pt } from "@/features/labs/network-diagram/model";
import { labCommands } from "@/lib/dev-mock-labs";
import { must } from "@/components/test-support";

const machine = (name: string, ip: string, published: number[]): Machine => ({
  name,
  state: "running",
  image: `${name}:latest`,
  ip,
  ports: published.map((p, i) => ({ published: p, target: 80 + i })),
  interfaces: [{ network: "default", ip }],
  services: [],
  infra: false,
});

describe("published port tabs", () => {
  it("never overlap, under one machine or across neighbours", async () => {
    // CI/CD Goat-like: several machines side by side, each publishing several ports.
    const machines = [
      machine("jenkins", "10.0.0.2", [51198, 51199, 51200]),
      machine("gitea", "10.0.0.3", [51201, 51202]),
      machine("ctfd", "10.0.0.4", [51203]),
    ];
    const topo = topology(machines, [{ name: "default", subnet: "10.0.0.0/24", internal: false }], null);
    const { boxes } = await layout(topo, (id) => (id.startsWith("hp-") ? { w: 2, h: 2 } : { w: 220, h: 140 }));
    const centres = [...boxes.entries()]
      .filter(([id]) => id.startsWith("hp-"))
      .map(([, b]) => b.x + b.w / 2)
      .sort((a, b) => a - b);
    expect(centres).toHaveLength(6);
    centres.slice(1).forEach((c, i) => {
      expect(c - centres[i]).toBeGreaterThanOrEqual(PORT_TAB_SLOT);
    });
  });
});

type Seg = [Pt, Pt];
const segments = (r: Pt[]): Seg[] => r.slice(1).map((p, i) => [r[i], p]);
// Orthogonal segments cross when one is vertical, the other horizontal, and each spans the other.
const cross = ([a, b]: Seg, [c, d]: Seg) => {
  const v1 = a.x === b.x,
    v2 = c.x === d.x;
  if (v1 === v2) return false;
  const [v, h] = v1
    ? [
        [a, b],
        [c, d],
      ]
    : [
        [c, d],
        [a, b],
      ];
  const within = (x: number, p: number, q: number) => x > Math.min(p, q) && x < Math.max(p, q);
  return within(v[0].x, h[0].x, h[1].x) && within(h[0].y, v[0].y, v[1].y);
};

describe("published port links", () => {
  it("leave from their service's box and don't cross", async () => {
    const m: Machine = {
      ...machine("web", "10.0.0.2", []),
      ports: [
        { published: 8080, target: 80 },
        { published: 3307, target: 3306 },
      ],
      services: [
        { name: "portal", kind: "http", ports: [80] },
        { name: "db", kind: "mysql", ports: [3306] },
      ],
    };
    const topo = topology([m], [{ name: "default", subnet: "10.0.0.0/24", internal: false }], null);
    // The portal row sits above the db row; each handle on its box's right edge.
    const handles: Record<string, Pt> = { "pub-8080": { x: 220, y: 70 }, "pub-3307": { x: 220, y: 115 } };
    const { boxes, routes } = await layout(
      topo,
      (id) => (id.startsWith("hp-") ? { w: 2, h: 2 } : { w: 220, h: 140 }),
      (_node, handle) => handles[handle],
    );
    const [, web] = must(
      [...boxes.entries()].find(([id, box]) => !id.startsWith("hp-") && !id.startsWith("zone") && !id.startsWith("bridge") && box.h === 140),
      "the web card",
    );
    const portal = must(routes.get("e-hp-web-8080"), "the portal's route");
    const db = must(routes.get("e-hp-web-3307"), "the db's route");
    expect(portal[0]).toEqual({ x: web.x + 220, y: web.y + 70 });
    expect(db[0]).toEqual({ x: web.x + 220, y: web.y + 115 });
    for (const s1 of segments(portal)) for (const s2 of segments(db)) expect(cross(s1, s2)).toBe(false);
  });
});

// ---- Nothing drawn over anything else: zone labels, bridges, cards and the zones' frames ----

type Rect = { id: string; x: number; y: number; w: number; h: number };
type Net = { name: string; subnet: string; internal: boolean };

const net = (name: string, internal = true): Net => ({ name, subnet: `10.${String(name.length)}.0.0/24`, internal });
const on = (name: string, nets: string[], published: number[] = []): Machine => ({
  ...machine(name, `10.0.0.${String(name.length)}`, published),
  interfaces: nets.map((n, i) => ({ network: n, ip: `10.${String(i)}.0.${String(name.length)}` })),
});

/** Measured sizes as the cards come out: taller with each interface and service row. */
const sized = (topo: ReturnType<typeof topology>) => (id: string) => {
  if (id.startsWith("hp-") || id.startsWith("up-")) return { w: 2, h: 2 };
  if (id.startsWith("bridge-")) return { w: BRIDGE.w, h: BRIDGE.h };
  if (id === "__attacker") return { w: 120, h: 64 };
  const data = must(
    topo.nodes.find((n) => n.id === id),
    `the node ${id}`,
  ).data as { ifaces: unknown[]; rows: unknown[] };
  return { w: 188, h: 70 + 14 * data.ifaces.length + 34 * data.rows.length };
};

const overlap = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const inside = (a: Rect, b: Rect) => a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h;

/** Lays the lab out and frames it, as the diagram draws it; returns what must stay apart. */
async function drawn(machines: Machine[], networks: Net[], attacker: Attacker = null) {
  const topo = topology(machines, networks, attacker);
  const { boxes, routes, bounds } = await layout(topo, sized(topo));
  const { zones } = frameLayout(topo, boxes, routes, bounds);
  const rect = (id: string): Rect => ({ id, ...must(boxes.get(id), `the box of ${id}`) });
  const frames = zones.map((z) => ({ id: z.id, x: z.position.x, y: z.position.y, w: Number(z.style?.width), h: Number(z.style?.height) }));
  const labels = topo.zones.map((z, i) => ({
    id: `label-${z.id}`,
    x: frames[i].x + ZONE_LABEL.left,
    y: frames[i].y + ZONE_LABEL.top,
    w: zoneLabelWidth(z.data),
    h: ZONE_LABEL.h,
  }));
  const cards = topo.nodes.filter((n) => n.type === "computer" || n.type === "attacker").map((n) => rect(n.id));
  const bridges = topo.nodes.filter((n) => n.type === "bridge").map((n) => rect(n.id));
  const memberOf = new Map(topo.zones.flatMap((z) => z.members.map((m) => [m, z.id] as const)));
  return { topo, frames, labels, cards, bridges, memberOf };
}

async function expectNoOverlap(machines: Machine[], networks: Net[], attacker: Attacker = null) {
  const { frames, labels, cards, bridges, memberOf } = await drawn(machines, networks, attacker);
  const shapes = [...labels, ...bridges, ...cards];
  for (const [i, a] of shapes.entries()) for (const b of shapes.slice(i + 1)) expect(overlap(a, b), `${a.id} over ${b.id}`).toBe(false);
  // A frame with its label on top (the label rises above the frame's top edge).
  const zoneArea = frames.map((f) => ({ ...f, y: f.y + ZONE_LABEL.top, h: f.h - ZONE_LABEL.top }));
  for (const [i, a] of zoneArea.entries()) for (const b of zoneArea.slice(i + 1)) expect(overlap(a, b), `${a.id} over ${b.id}`).toBe(false);
  // A card sits inside its own network's frame and clear of the others; a machine between
  // networks (multi-homed) is clear of every frame.
  for (const card of cards) {
    const home = memberOf.get(card.id);
    for (const z of zoneArea) {
      if (z.id === home) expect(inside(card, z), `${card.id} inside ${z.id}`).toBe(true);
      else expect(overlap(card, z), `${card.id} over ${z.id}`).toBe(false);
    }
  }
  return { frames, cards };
}

describe("the drawn diagram", () => {
  it("keeps the dev mock's lab (three networks, two pivots) apart", async () => {
    const status = labCommands.lab_status({ id: "lab-invoice" }) as { machines: Machine[]; networks: Net[] };
    const { frames } = await expectNoOverlap(status.machines, status.networks, { running: true, ip: "10.42.0.9", labNetwork: "default" });
    expect(frames).toHaveLength(3);
  });

  it("keeps a machine on three networks apart from all three", async () => {
    await expectNoOverlap(
      [on("hub", ["default", "billing", "finance"], [8080]), on("web", ["default"]), on("ledger", ["billing"]), on("payroll", ["finance"])],
      [net("default", false), net("billing"), net("finance")],
      { running: true, ip: "10.0.0.9", labNetwork: "default" },
    );
  });

  it("keeps a single network apart", async () => {
    const { frames } = await expectNoOverlap([on("web", ["default"], [80, 443]), on("db", ["default"])], [net("default", false)], {
      running: false,
      ip: "",
    });
    expect(frames).toHaveLength(1);
  });

  it("keeps a lone machine on a long-named network apart", async () => {
    await expectNoOverlap([on("a", ["a-network-with-a-rather-long-name"])], [net("a-network-with-a-rather-long-name")]);
  });

  it("keeps a chain of networks, each behind a pivot, apart", async () => {
    const names = ["default", "dmz", "app", "data", "vault"];
    const pivots = names.slice(1).map((n, i) => on(`gw-${n}`, [names[i], n]));
    const leaves = names.flatMap((n) => [on(`${n}-1`, [n]), on(`${n}-2`, [n], [9000 + n.length])]);
    await expectNoOverlap(
      [...pivots, ...leaves],
      names.map((n) => net(n, n !== "default")),
    );
  });

  it("keeps several pivots between the same networks, and an empty network, apart", async () => {
    await expectNoOverlap(
      [on("p1", ["default", "back"]), on("p2", ["default", "back"]), on("p3", ["back", "default", "side"]), on("x", ["default"])],
      [net("default", false), net("back"), net("side"), net("empty")],
    );
  });
});
