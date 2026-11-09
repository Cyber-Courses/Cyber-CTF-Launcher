// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useLabTools } from "@/features/labs/lab-detail/use-lab-tools";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

describe("useLabTools", () => {
  it("asks in the lab's declared runtime, the backend reading the form it was started in", async () => {
    const tool = { name: "shell", image: null, addresses: [{ network: "lab", ip: "10.30.0.252" }], publish: null };
    const calls = installTauri({ lab_tools: () => [tool] });
    const { result } = renderHook(() => useLabTools("dual", true, "DOCKER"));
    await flush();
    expect(calls.find((c) => c.cmd === "lab_tools")?.args).toEqual({ id: "dual", runtime: "DOCKER" });
    expect(result.current).toEqual([tool]);
  });

  it("reads nothing while the lab is down", async () => {
    const calls = installTauri({ lab_tools: () => [] });
    renderHook(() => useLabTools("dual", false, "VM"));
    await flush();
    expect(calls.some((c) => c.cmd === "lab_tools")).toBe(false);
  });
});
