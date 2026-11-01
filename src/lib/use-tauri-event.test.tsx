// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { describe, expect, it, vi } from "vitest";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

// An event's payload reaches the handler while the hook is mounted and enabled, and no longer
// once it unmounts (the listener is taken down).

/** Emits `name` with `payload` and lets the handlers it reaches run. */
async function emitted(name: string, payload: unknown) {
  await act(() => emit(name, payload));
}

describe("useTauriEvent", () => {
  it("delivers an event emitted once the listener is set up, with the latest handler", async () => {
    installTauri();
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(
      (p: { handler: (payload: unknown) => void }) => {
        useTauriEvent("lab-ready", p.handler);
      },
      { initialProps: { handler: first } },
    );
    // `listen` answers asynchronously: an event before that has nobody to reach.
    await flush();
    await emitted("lab-ready", { id: "lab-sqli" });
    expect(first).toHaveBeenCalledWith({ id: "lab-sqli" });
    // The handler changes without listening again: the new one gets the next event.
    rerender({ handler: second });
    await emitted("lab-ready", { id: "lab-goad" });
    expect(second).toHaveBeenCalledWith({ id: "lab-goad" });
    expect(first).toHaveBeenCalledTimes(1);
  });

  it("stops delivering once unmounted", async () => {
    installTauri();
    const handler = vi.fn();
    const { unmount } = renderHook(() => {
      useTauriEvent("lab-ready", handler);
    });
    await flush();
    await emitted("lab-ready", 1);
    expect(handler).toHaveBeenCalledTimes(1);
    unmount();
    await flush();
    await emitted("lab-ready", 2);
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("listens to nothing while disabled, and starts once enabled", async () => {
    installTauri();
    const handler = vi.fn();
    const { rerender } = renderHook(
      (p: { enabled: boolean }) => {
        useTauriEvent("lab-ready", handler, p.enabled);
      },
      { initialProps: { enabled: false } },
    );
    await flush();
    await emitted("lab-ready", "early");
    expect(handler).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await flush();
    await emitted("lab-ready", "late");
    expect(handler).toHaveBeenCalledWith("late");
  });
});
