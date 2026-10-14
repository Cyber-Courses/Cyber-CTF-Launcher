// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppearance } from "@/lib/appearance";
import { ignore, openExternal, tell, warn } from "@/lib/failure";
import { getLocale, getLocalePreference, LOCALE_NAMES, LOCALES, setLocalePreference, translate, useFormat, useLocale, useT } from "@/lib/i18n";
import { notify } from "@/lib/notify";
import { useNow } from "@/lib/use-now";
import { usePoll, usePolled } from "@/lib/use-poll";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { Toaster } from "@/components/ui/toaster";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

describe("i18n", () => {
  afterEach(() => setLocalePreference("system"));

  it("translates, interpolates and falls back to the key", () => {
    setLocalePreference("en");
    expect(translate("labs.detail.startLab")).toBe("Start lab");
    expect(translate("common.units.seconds", { n: 5 })).toBe("5s");
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(translate("no.such.key" as never)).toBe("no.such.key");
    expect(warnSpy).toHaveBeenCalled();
    warnSpy.mockRestore();
  });

  it("picks the plural form for the language", () => {
    setLocalePreference("en");
    expect(translate("common.time.daysAgo", { count: 1 })).not.toBe(translate("common.time.daysAgo", { count: 2 }));
    expect(translate("common.time.daysAgo", { count: 2 })).toContain("2");
  });

  it("keeps a placeholder it has no value for", () => {
    setLocalePreference("en");
    expect(translate("common.units.seconds", { other: 1 })).toContain("{n}");
  });

  it("switches language and remembers the choice", () => {
    setLocalePreference("fr");
    expect(getLocale()).toBe("fr");
    expect(getLocalePreference()).toBe("fr");
    expect(document.documentElement.lang).toBe("fr");
    expect(translate("labs.detail.startLab")).not.toBe("Start lab");
    setLocalePreference("system");
    expect(getLocalePreference()).toBe("system");
  });

  it("maps the system language to a supported one", () => {
    const langs = vi.spyOn(navigator, "languages", "get").mockReturnValue(["xx-YY", "pt-PT", "en"]);
    setLocalePreference("system");
    expect(getLocale()).toBe("pt-BR");
    langs.mockReturnValue(["xx"]);
    setLocalePreference("system");
    expect(getLocale()).toBe("en");
    langs.mockRestore();
  });

  it("names every language in itself", () => {
    for (const l of LOCALES) expect(LOCALE_NAMES[l].length).toBeGreaterThan(0);
  });

  it("re-renders components on a change, here or from another window", () => {
    setLocalePreference("en");
    function Probe() {
      const t = useT();
      const f = useFormat();
      return (
        <p>
          {useLocale()} {t("labs.detail.startLab")} {f.number(1234.5)} {f.date(0, { timeZone: "UTC", year: "numeric" })} {f.relative(-1, "day")}
          {t.rich("machine.setup.title", { em: (s) => <em>{s}</em> })}
        </p>
      );
    }
    render(<Probe />);
    expect(screen.getByText(/en Start lab 1,234.5 1970 yesterday/)).toBeTruthy();
    expect(document.querySelector("em")).not.toBeNull();
    act(() => setLocalePreference("de"));
    expect(screen.getByText(/^de /)).toBeTruthy();
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.locale", newValue: "ja" }));
    });
    expect(screen.getByText(/^ja /)).toBeTruthy();
    // Unrelated keys are ignored.
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "other", newValue: "fr" }));
    });
    expect(screen.getByText(/^ja /)).toBeTruthy();
  });

  it("renders markup tags, and the text of unknown ones", () => {
    setLocalePreference("en");
    const { result } = renderHook(() => useT());
    const nodes = result.current.rich("machine.setup.title", {});
    expect(nodes.join("")).not.toContain("<");
  });
});

/** A browser Notification API whose permission is undecided and answers `answer` when asked;
 *  returns the notifications shown. */
function stubNotification(answer: NotificationPermission) {
  const sent: { title: string; body?: string }[] = [];
  vi.stubGlobal(
    "Notification",
    class {
      static permission = "default";
      static requestPermission = async () => answer;
      constructor(title: string, o?: { body?: string }) {
        sent.push({ title, body: o?.body });
      }
    },
  );
  return sent;
}

describe("failure helpers", () => {
  it("ignore and warn log at their levels", () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    const w = vi.spyOn(console, "warn").mockImplementation(() => {});
    ignore("why")(new Error("x"));
    warn("what")(new Error("y"));
    expect(debug).toHaveBeenCalledWith("ignored (why):", expect.any(Error));
    expect(w).toHaveBeenCalledWith("what:", expect.any(Error));
    w.mockRestore();
  });

  it("tell shows a toast while the window has focus", async () => {
    installTauri();
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
    render(<Toaster />);
    act(() => tell("Couldn't stop the lab")("docker is not running"));
    expect(await screen.findByText("Couldn't stop the lab")).toBeTruthy();
    focus.mockRestore();
  });

  it("tell sends an OS notification otherwise", async () => {
    const calls = installTauri({ "plugin:notification|is_permission_granted": () => true });
    const sent = stubNotification("granted");
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    tell("Lab failed")("boom");
    await flush();
    expect(commands(calls)).toContain("plugin:notification|is_permission_granted");
    expect(sent).toEqual([{ title: "Lab failed", body: "boom" }]);
    focus.mockRestore();
    vi.unstubAllGlobals();
  });

  it("openExternal tells the player when the browser won't open", async () => {
    installTauri({
      "plugin:opener|open_url": () => {
        throw new Error("no browser");
      },
    });
    const w = vi.spyOn(console, "warn").mockImplementation(() => {});
    openExternal("https://cyberctf.org");
    await flush();
    expect(w.mock.calls.some((c) => String(c[1]).includes("https://cyberctf.org"))).toBe(true);
  });

  it("notify asks for permission and never throws", async () => {
    installTauri({ "plugin:notification|is_permission_granted": () => false });
    const sent = stubNotification("denied");
    await expect(notify("t", "b")).resolves.toBeUndefined();
    expect(sent).toEqual([]);
    vi.unstubAllGlobals();
    installTauri({
      "plugin:notification|is_permission_granted": () => {
        throw new Error("unavailable");
      },
    });
    await expect(notify("t")).resolves.toBeUndefined();
  });
});

describe("appearance", () => {
  beforeEach(() => document.documentElement.classList.remove("black", "light"));

  it("defaults to dark, applies a choice and follows other windows", () => {
    const { result } = renderHook(() => useAppearance());
    expect(result.current[0]).toBe("dark");
    act(() => result.current[1]("light"));
    expect(result.current[0]).toBe("light");
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(localStorage.getItem("cyberctf.appearance")).toBe("light");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.appearance", newValue: "black" }));
    });
    expect(result.current[0]).toBe("black");
    expect(document.documentElement.classList.contains("black")).toBe(true);
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.appearance", newValue: "nonsense" }));
    });
    expect(result.current[0]).toBe("dark");
    expect(document.documentElement.classList.contains("black")).toBe(false);
  });

  it("reads the saved appearance and sets the window theme in the app", async () => {
    const calls = installTauri();
    localStorage.setItem("cyberctf.appearance", "black");
    const { result } = renderHook(() => useAppearance());
    expect(result.current[0]).toBe("black");
    act(() => result.current[1]("light"));
    await flush();
    expect(commands(calls).some((c) => c.includes("theme"))).toBe(true);
  });
});

describe("timers", () => {
  afterEach(() => vi.useRealTimers());

  it("usePoll reads now, then on every tick, and on demand", async () => {
    vi.useFakeTimers();
    const read = vi.fn(async () => 1);
    const onValue = vi.fn();
    const { result, unmount } = renderHook(() => usePoll(read, 1000, { onValue, onFocus: true }));
    await act(async () => {});
    expect(read).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(2000));
    expect(read).toHaveBeenCalledTimes(3);
    await act(async () => result.current());
    expect(read).toHaveBeenCalledTimes(4);
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(read).toHaveBeenCalledTimes(5);
    await act(async () => {});
    expect(onValue).toHaveBeenCalledWith(1);
    unmount();
    // Stopped: no more reads, and a late "read now" is a no-op.
    act(() => result.current());
    vi.advanceTimersByTime(5000);
    expect(read).toHaveBeenCalledTimes(5);
  });

  it("usePoll skips overlapping reads when serial, and reports errors", async () => {
    vi.useFakeTimers();
    let resolve: () => void = () => {};
    const read = vi.fn(() => new Promise<number>((_, reject) => (resolve = () => reject(new Error("down")))));
    const onError = vi.fn();
    renderHook(() => usePoll(read, 100, { onValue: vi.fn(), onError, serial: true }));
    await act(async () => {
      vi.advanceTimersByTime(350);
    });
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => resolve());
    expect(onError).toHaveBeenCalled();
  });

  it("usePoll does nothing while disabled", () => {
    const read = vi.fn(async () => 1);
    renderHook(() => usePoll(read, 100, { onValue: vi.fn(), enabled: false }));
    expect(read).not.toHaveBeenCalled();
  });

  it("usePolled keeps the latest answer", async () => {
    const { result } = renderHook(() => usePolled(async () => "fresh", 10_000, "initial"));
    expect(result.current[0]).toBe("initial");
    await flush();
    expect(result.current[0]).toBe("fresh");
  });

  it("useNow ticks and can be touched", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    const { result } = renderHook(() => useNow(500));
    expect(result.current[0]).toBe(1000);
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(result.current[0]).toBe(1500);
    vi.setSystemTime(9000);
    act(() => result.current[1]());
    expect(result.current[0]).toBe(9000);
  });

  it("useNow stays put while disabled", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1000);
    const { result } = renderHook(() => useNow(500, false));
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(result.current[0]).toBe(1000);
  });
});

describe("useTauriEvent", () => {
  it("calls the latest handler with each payload while enabled", async () => {
    installTauri();
    const first = vi.fn();
    const second = vi.fn();
    const { rerender, unmount } = renderHook(({ h, on }) => useTauriEvent<string>("navigate", h, on), { initialProps: { h: first, on: true } });
    await flush();
    await act(() => emit("navigate", "labs"));
    expect(first).toHaveBeenCalledWith("labs");
    rerender({ h: second, on: true });
    await act(() => emit("navigate", "cloud"));
    expect(second).toHaveBeenCalledWith("cloud");
    rerender({ h: second, on: false });
    await flush();
    await act(() => emit("navigate", "home"));
    expect(second).not.toHaveBeenCalledWith("home");
    unmount();
  });
});
