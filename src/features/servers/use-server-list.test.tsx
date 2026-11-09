// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useSavedHosts, useServerList } from "@/features/servers/use-server-list";
import { answer } from "@/lib/dev-mock";
import type { ServerList } from "@/lib/tauri";
import { installTauri, rejects } from "@/test/tauri";
import { flush } from "@/test/ui";

const told = vi.hoisted(() => vi.fn<(title: string, e: unknown) => void>());
vi.mock("@/lib/failure", async (actual) => ({
  ...(await actual<typeof import("@/lib/failure")>()),
  tell: (title: string) => (e: unknown) => {
    told(title, e);
  },
}));

const sample = () => structuredClone(answer("server_list", undefined) as ServerList);

afterEach(() => {
  told.mockReset();
});

describe("the saved servers", () => {
  it("are read again when the setup window saves one, wherever they are read", async () => {
    // Regression: the labs list and home read them once, so a server added in its setup window
    // left a lab "needing setup" until the page was opened again.
    let list: ServerList = { default: null, hosts: [] };
    installTauri({ server_list: () => list });
    const { result } = renderHook(() => useSavedHosts());
    await act(() => flush());
    expect(result.current.hosts).toEqual([]);
    list = sample();
    await act(async () => {
      await emit("server:changed");
      await flush();
    });
    expect(result.current.hosts.map((h) => h.id)).toEqual(list.hosts.map((h) => h.id));
    expect(result.current.defaultId).toBe(list.default);
  });

  it("are never 'none' when they can't be read: the failure is said, the last list kept", async () => {
    // Regression: a failed read turned into an empty list, so every saved server vanished from
    // the picker and the labs without a word.
    let fail = false;
    const list = sample();
    installTauri({ server_list: (a) => (fail ? rejects({ code: "other", message: "server.json: expected value" })(a) : list) });
    const saved = renderHook(() => useSavedHosts());
    const page = renderHook(() => useServerList());
    await act(() => flush());
    fail = true;
    await act(async () => {
      await emit("server:changed");
      await flush();
    });
    expect(saved.result.current.hosts.map((h) => h.id)).toEqual(list.hosts.map((h) => h.id));
    expect(told).toHaveBeenCalledWith("Couldn't read your saved servers and cloud accounts", expect.anything());
    // A page that shows the error itself has it, the list kept, and no toast of its own.
    expect(page.result.current.error).toContain("server.json: expected value");
    expect(page.result.current.list?.hosts.length).toBe(list.hosts.length);
    expect(told).toHaveBeenCalledTimes(1);
  });
});
