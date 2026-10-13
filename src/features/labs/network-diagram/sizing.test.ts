// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { fittedView } from "@/features/labs/network-diagram/flow";
import { shellSize, topologySig } from "@/features/labs/network-diagram/index";
import type { Machine } from "@/features/labs/network-diagram/model";

const api: Machine = {
  name: "api",
  state: "running",
  image: "node:20",
  ip: "10.0.0.2",
  ports: [{ published: 8080, target: 80 }],
  interfaces: [{ network: "default", ip: "10.0.0.2" }],
  services: [],
  infra: false,
};

describe("shellSize", () => {
  it("keeps the default height until the graph is measured", () => {
    expect(shellSize(null, 800)).toEqual({ canvas: 800, height: 430 });
  });
  it("fits a small graph in the shell's width, at least 220px tall", () => {
    expect(shellSize({ w: 300, h: 100 }, 800)).toEqual({ canvas: 800, height: 220 });
  });
  it("grows the canvas past a narrow shell instead of shrinking below the readable zoom", () => {
    expect(shellSize({ w: 2000, h: 2000 }, 600)).toEqual({ canvas: 1248, height: 720 });
  });
});

describe("fittedView", () => {
  it("centres an undocked drawing, zoomed to fit", () => {
    expect(fittedView({ x: 0, y: 0, w: 100, h: 100, docked: false }, 248, 400)).toEqual({ x: 71.5, y: 147.5, zoom: 1.05 });
  });
  it("sits a docked drawing on the shell's bottom edge", () => {
    const v = fittedView({ x: 0, y: 0, w: 100, h: 100, docked: true }, 248, 400);
    expect(v.y + 100 * v.zoom).toBeCloseTo(400);
  });
});

describe("topologySig", () => {
  it("changes with what is drawn, the attack box and the language, not with the same facts again", () => {
    const base = topologySig("en", [api], [], null);
    expect(topologySig("en", [{ ...api }], [], null)).toBe(base);
    expect(topologySig("fr", [api], [], null)).not.toBe(base);
    expect(topologySig("en", [{ ...api, state: "exited" }], [], null)).not.toBe(base);
    expect(topologySig("en", [api], [{ name: "default", subnet: "10.0.0.0/24", internal: true }], null)).not.toBe(base);
    expect(topologySig("en", [api], [], { running: true, ip: "10.0.0.9" })).not.toBe(base);
    expect(topologySig("en", [api], [], { running: false, ip: "10.0.0.9" })).toBe(base);
  });
});
