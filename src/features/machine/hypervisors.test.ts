import { describe, expect, it } from "vitest";

import { labHypervisor, preferredFirst } from "@/features/machine/hypervisors";
import type { Provider } from "@/lib/tauri";

const ready = ["virtualbox", "qemu"] as Provider[];

describe("preferredFirst", () => {
  it("puts the preferred one first when it is ready", () => {
    expect(preferredFirst(ready, "qemu")).toEqual(["qemu", "virtualbox"]);
    expect(preferredFirst(ready, "parallels")).toEqual(["virtualbox", "qemu"]);
    expect(preferredFirst(ready, null)).toEqual(["virtualbox", "qemu"]);
  });
});

describe("labHypervisor", () => {
  it("is Settings' choice when it is ready, else the first ready one, else none", () => {
    expect(labHypervisor(ready, "qemu")).toBe("qemu");
    expect(labHypervisor(ready, "parallels")).toBe("virtualbox");
    expect(labHypervisor(ready, null)).toBe("virtualbox");
    expect(labHypervisor([], "qemu")).toBeNull();
  });
});
