// @vitest-environment jsdom
import { act, render, renderHook, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { closeIfSettings, showInMainWindow } from "@/lib/app-events";
import { useAppearance, useAppearanceSync } from "@/lib/appearance";
import {
  appendDeployLog,
  getDeploySnapshot,
  operationLabel,
  OPERATION_STATUS,
  SIGNED_OUT_EVENT,
  useActiveOperations,
  useDeployingLabs,
  useWorkerLog,
} from "@/lib/deploy-store";
import { LOCALES, getLocale, getLocalePreference, setLocalePreference, translate, useFormat, useLocale, useT, type Locale } from "@/lib/i18n";
import { isMac, useIsMac } from "@/lib/platform";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { usePoll } from "@/lib/use-poll";
import type { ActiveOperation } from "@/lib/tauri";
import { commands, installTauri } from "@/test/tauri";
import { flush } from "@/test/ui";

afterEach(() => {
  act(() => setLocalePreference("en"));
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const languages = (list: string[]) => vi.spyOn(navigator, "languages", "get").mockReturnValue(list);

describe("i18n", () => {
  it("follows the system language, any region, else English", () => {
    languages(["pt-PT", "en-US"]);
    setLocalePreference("system");
    expect(getLocale()).toBe("pt-BR");
    expect(getLocalePreference()).toBe("system");
    expect(document.documentElement.lang).toBe("pt-BR");
    languages(["de-CH"]);
    setLocalePreference("system");
    expect(getLocale()).toBe("de");
    languages(["ko-KR", "zh"]);
    setLocalePreference("system");
    expect(getLocale()).toBe("en");
  });

  it("keeps a chosen language, and ignores an unknown saved one", () => {
    setLocalePreference("ja");
    expect(getLocalePreference()).toBe("ja");
    localStorage.setItem("cyberctf.locale", "klingon");
    expect(getLocalePreference()).toBe("system");
  });

  it("falls back to English, then the key itself", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    setLocalePreference("fr");
    expect(translate("shell.tabs.labs")).toBe("Labs");
    // @ts-expect-error: a key no catalog has
    expect(translate("nowhere.at.all")).toBe("nowhere.at.all");
    expect(warn).toHaveBeenCalledWith('i18n: missing message "nowhere.at.all"');
    // A plural without a count can't pick a form: the key, too.
    expect(translate("home.running.labs")).toBe("home.running.labs");
  });

  it("keeps a placeholder it has no value for", () => {
    expect(translate("hosted.stopFailed", {})).toBe("Couldn't stop the hosted lab: {error}");
  });

  it.each([
    ["en", 1, "1 lab"],
    ["en", 3, "3 labs"],
    ["fr", 0, "0 lab"],
    ["fr", 2, "2 labs"],
    ["ja", 1, "1 件のラボ"],
  ] as [Locale, number, string][])("%s plural of %d labs: %s", (locale, count, text) => {
    setLocalePreference(locale);
    // Japanese has a single plural form: "other", whatever the count.
    expect(translate("home.running.labs", { count })).toBe(locale === "ja" && count === 1 ? "1 件のラボ" : text);
  });

  it("re-renders when another window changes the language", () => {
    const { result } = renderHook(() => useLocale());
    expect(result.current).toBe("en");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.locale", newValue: "es" }));
    });
    expect(result.current).toBe("es");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "other", newValue: "de" }));
    });
    expect(result.current).toBe("es");
    // Cleared there: back to the system's.
    languages(["fr-CA"]);
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.locale", newValue: null }));
    });
    expect(result.current).toBe("fr");
  });

  it("renders <tag> markup, leaving unknown tags as text", () => {
    function Rich() {
      const t = useT();
      return <p>{t.rich("shell.update.available", { version: (v) => <b>{v}</b> }, { version: "2.0.0" })}</p>;
    }
    render(<Rich />);
    expect(screen.getByText("2.0.0").tagName).toBe("B");
    expect(screen.getByText(/is available/)).toBeTruthy();
    function Plain() {
      return <p>{useT().rich("shell.update.available", {}, { version: "3.0.0" })}</p>;
    }
    render(<Plain />);
    expect(screen.getByText(/3\.0\.0/).tagName).toBe("P");
  });

  // The same number, date and relative time in each language.
  const expected: Record<Locale, { number: string; relative: string }> = {
    en: { number: "1,234.5", relative: "yesterday" },
    fr: { number: "1 234,5", relative: "hier" },
    es: { number: "1234,5", relative: "ayer" },
    de: { number: "1.234,5", relative: "gestern" },
    "pt-BR": { number: "1.234,5", relative: "ontem" },
    ja: { number: "1,234.5", relative: "昨日" },
  };
  it.each(LOCALES)("formats numbers, dates and relative times in %s", (locale) => {
    act(() => setLocalePreference(locale));
    const { result } = renderHook(() => useFormat());
    expect(result.current.number(1234.5).replace(/\s/g, " ")).toBe(expected[locale].number);
    expect(result.current.relative(-1, "day")).toBe(expected[locale].relative);
    expect(result.current.date(new Date(Date.UTC(2026, 9, 11, 12)), { year: "numeric", timeZone: "UTC" })).toMatch(/2026/);
  });
});

describe("deploy-store", () => {
  const op = (o: ActiveOperation["op"], machine: string | null = null): ActiveOperation => ({ labId: "l", op: o, machine, step: null });

  it("names every operation", () => {
    expect((["launch", "resume", "pause", "shutdown", "provision", "attack_vm", "stop"] as const).map((o) => operationLabel(op(o)))).toEqual([
      "Starting…",
      "Resuming…",
      "Pausing…",
      "Shutting down…",
      "Provisioning…",
      "Starting the attack VM…",
      "Stopping…",
    ]);
    expect(operationLabel(op("provision", "isoloom-DC01"))).toContain("DC01");
    expect(operationLabel(op("provision", "isoloom-DC01"))).not.toContain("isoloom");
  });

  it("names the status shown while an operation runs, in the current language", () => {
    expect([OPERATION_STATUS.launch, OPERATION_STATUS.resume, OPERATION_STATUS.pause, OPERATION_STATUS.shutdown, OPERATION_STATUS.stop].every(Boolean)).toBe(
      true,
    );
    const en = OPERATION_STATUS.launch;
    setLocalePreference("de");
    expect(OPERATION_STATUS.launch).not.toBe(en);
  });

  it("keeps log lines, and reads no sign-out from them", () => {
    const heard = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, heard);
    appendDeployLog("lab-x", "Downloading");
    appendDeployLog("lab-x", "✗ You're signed out. Sign in again.");
    expect(heard).not.toHaveBeenCalled();
    window.removeEventListener(SIGNED_OUT_EVENT, heard);
    // The lines are kept; whether the lab is busy stays begin/end's to say.
    expect(getDeploySnapshot().runs["lab-x"].logs).toHaveLength(2);
    expect(getDeploySnapshot().runs["lab-x"].busy).toBe(false);
  });

  it("polls the labs deploying and the operations in flight", async () => {
    installTauri();
    const deploying = renderHook(() => useDeployingLabs());
    const ops = renderHook(() => useActiveOperations());
    await flush();
    expect([...deploying.result.current]).toEqual(["lab-goad"]);
    expect(ops.result.current.get("lab-goad")?.step).toContain("SRV02");
  });

  it("follows a worker's log only while active, skipping blank lines", async () => {
    const calls = installTauri({ lab_deploy_log: () => "line 1\n\nline 2\n" });
    const { result, rerender } = renderHook(({ active }) => useWorkerLog("lab-goad", active, 1000), { initialProps: { active: false } });
    await flush();
    expect(result.current.lines).toEqual([]);
    expect(commands(calls)).not.toContain("lab_deploy_log");
    rerender({ active: true });
    await flush();
    expect(result.current.lines).toEqual(["line 1", "line 2"]);
    expect(result.current.times).toHaveLength(2);
    expect(calls.find((c) => c.cmd === "lab_deploy_log")?.args).toMatchObject({ id: "lab-goad" });
  });
});

describe("usePoll", () => {
  it("reads now and on every tick, skips a tick while a slow read is in flight, and reads on focus", async () => {
    vi.useFakeTimers();
    let resolve: (v: number) => void = () => {};
    const read = vi.fn(() => new Promise<number>((r) => (resolve = r)));
    const onValue = vi.fn();
    const { result, unmount } = renderHook(() => usePoll(read, 1000, { onValue, serial: true, onFocus: true }));
    expect(read).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(1000));
    // Still reading: the tick is skipped.
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => resolve(7));
    expect(onValue).toHaveBeenCalledWith(7);
    window.dispatchEvent(new Event("focus"));
    expect(read).toHaveBeenCalledTimes(2);
    await act(async () => resolve(8));
    act(() => result.current());
    expect(read).toHaveBeenCalledTimes(3);
    unmount();
    // A read answering after unmount is dropped.
    await act(async () => resolve(9));
    expect(onValue).not.toHaveBeenCalledWith(9);
    await act(() => vi.advanceTimersByTimeAsync(5000));
    expect(read).toHaveBeenCalledTimes(3);
  });

  it("hands failures to onError, and does nothing while disabled", async () => {
    const onError = vi.fn();
    const read = vi.fn(async () => {
      throw new Error("down");
    });
    const { rerender } = renderHook(({ enabled }) => usePoll(read, 1000, { onValue: vi.fn(), onError, enabled }), { initialProps: { enabled: false } });
    await flush();
    expect(read).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await flush();
    expect(onError).toHaveBeenCalledWith(expect.any(Error));
  });
});

describe("platform", () => {
  it("tells macOS from the rest", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)");
    expect(isMac()).toBe(true);
    expect(renderHook(() => useIsMac()).result.current).toBe(true);
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64)");
    expect(isMac()).toBe(false);
    expect(renderHook(() => useIsMac()).result.current).toBe(false);
  });
});

describe("useFocusTrap", () => {
  function Dialog({ active, empty = false }: { active: boolean; empty?: boolean }) {
    const ref = useRef<HTMLDivElement>(null);
    useFocusTrap(ref, active);
    return (
      <div ref={ref} tabIndex={-1} data-testid="dialog">
        {!empty && (
          <>
            <button>first</button>
            <button>last</button>
          </>
        )}
      </div>
    );
  }
  const visible = () =>
    document.querySelectorAll("button").forEach((b) => Object.defineProperty(b, "offsetParent", { get: () => document.body, configurable: true }));

  it("focuses the dialog itself when it has nothing focusable, and holds Tab there", () => {
    render(<Dialog active empty />);
    const dialog = screen.getByTestId("dialog");
    expect(document.activeElement).toBe(dialog);
    const e = new KeyboardEvent("keydown", { key: "Tab", cancelable: true });
    dialog.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(true);
  });

  it("wraps Tab and Shift+Tab, pulls focus back in, and restores it on close", () => {
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    const { rerender } = render(<Dialog active={false} />);
    // Inactive: focus stays where it was.
    expect(document.activeElement).toBe(outside);
    visible();
    rerender(<Dialog active />);
    const dialog = screen.getByTestId("dialog");
    const [first, last] = screen.getAllByRole("button", { name: /first|last/ });
    expect(document.activeElement).toBe(first);
    const tab = (shift: boolean) => {
      const e = new KeyboardEvent("keydown", { key: "Tab", shiftKey: shift, cancelable: true });
      dialog.dispatchEvent(e);
      return e.defaultPrevented;
    };
    expect(tab(true)).toBe(true);
    expect(document.activeElement).toBe(last);
    expect(tab(false)).toBe(true);
    expect(document.activeElement).toBe(first);
    // In the middle of the cycle, the browser moves focus itself.
    last.focus();
    expect(tab(true)).toBe(false);
    // Other keys are left alone.
    const enter = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
    dialog.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(false);
    rerender(<Dialog active={false} />);
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });
});

describe("appearance", () => {
  it("applies and saves the chosen look, and follows another window", () => {
    const { result } = renderHook(() => useAppearance());
    expect(result.current[0]).toBe("dark");
    act(() => result.current[1]("light"));
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(localStorage.getItem("cyberctf.appearance")).toBe("light");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "cyberctf.appearance", newValue: "black" }));
    });
    expect(result.current[0]).toBe("black");
    expect(document.documentElement.classList.contains("light")).toBe(false);
    expect(document.documentElement.classList.contains("black")).toBe(true);
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "unrelated", newValue: "light" }));
    });
    expect(result.current[0]).toBe("black");
    act(() => result.current[1]("dark"));
    expect(document.documentElement.classList.contains("black")).toBe(false);
  });

  it("sets the window frame's theme inside the app", async () => {
    const calls = installTauri();
    localStorage.setItem("cyberctf.appearance", "light");
    renderHook(() => useAppearanceSync());
    renderHook(() => useAppearance());
    await flush(50);
    expect(calls.some((c) => c.cmd === "plugin:window|set_theme" && c.args?.value === "light")).toBe(true);
  });
});

describe("app events", () => {
  it("asks the main window to switch screens, and closes only the Settings window", async () => {
    const calls = installTauri();
    const heard = vi.fn();
    await listen("cyberctf:navigate", (e) => heard(e.payload));
    showInMainWindow("machine");
    await flush();
    expect(heard).toHaveBeenCalledWith("machine");
    // The main window ("main") stays open.
    expect(commands(calls)).not.toContain("plugin:window|close");
    closeIfSettings();
    await flush();
    expect(commands(calls)).not.toContain("plugin:window|close");
  });
});
