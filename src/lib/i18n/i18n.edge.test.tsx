// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LOCALES, getLocale, getLocalePreference, setLocalePreference, translate, useT, type Locale } from "@/lib/i18n";
import { must } from "@/components/test-support";
import { CATALOGS } from "@/messages";

/** Every { one, other } plural of a message tree, by path. */
function plurals(node: unknown, prefix = ""): [string, { one: string; other: string }][] {
  if (!node || typeof node !== "object") return [];
  const o = node as Record<string, unknown>;
  if (typeof o.one === "string" && typeof o.other === "string") return [[prefix, o as { one: string; other: string }]];
  return Object.entries(o).flatMap(([k, v]) => plurals(v, prefix ? `${prefix}.${k}` : k));
}

describe("i18n: plurals in every language", () => {
  afterEach(() => {
    setLocalePreference("system");
  });

  it("picks a form for 0, 1 and many in every language, never falling back to the key", () => {
    for (const locale of LOCALES) {
      setLocalePreference(locale);
      for (const count of [0, 1, 2, 21, 1000]) {
        const text = translate("labs.list.labCount", { count });
        expect(text, `${locale} ${String(count)}`).not.toBe("labs.list.labCount");
        expect(text, `${locale} ${String(count)}`).toContain(String(count));
      }
    }
  });

  it("gives zero the singular form in French and Brazilian Portuguese, the plural elsewhere", () => {
    // CLDR: fr and pt have `one` for 0 and 1; en, es and de keep `one` for exactly 1; ja has no `one`.
    const zero: Record<Locale, "one" | "other"> = { en: "other", fr: "one", es: "other", de: "other", "pt-BR": "one", ja: "other" };
    for (const locale of LOCALES) {
      setLocalePreference(locale);
      const [, forms] = must(
        plurals(CATALOGS[locale]).find(([p]) => p === "labs.list.labCount"),
        `${locale} labs.list.labCount`,
      );
      expect(translate("labs.list.labCount", { count: 0 }), locale).toBe(forms[zero[locale]].replace("{count}", "0"));
    }
  });

  it("Brazilian Portuguese singular forms carry the count, since zero takes them too", () => {
    // Intl.PluralRules("pt-BR").select(0) is "one", so with no lab running the Overview's
    // "Running now" header reads "1 lab" (home-screen.tsx passes `count: running.length`, which
    // is 0 there). The French forms carry {count} and read "0 lab". Suggested fix: write
    // "{count} lab" / "{count} contêiner" / "{count} lab rodando…" in pt-BR home.json, and keep
    // this invariant: where zero takes `one`, a `one` form must carry {count}.
    setLocalePreference("pt-BR");
    expect(translate("home.running.labs", { count: 0 })).toMatch(/^0/);
    expect(translate("home.running.containers", { count: 0 })).toMatch(/^0/);
    // The quit prompt only shows with at least one lab busy: its "one" may spell the number.
    const neverZero = new Set(["app.quitGuard.body"]);
    for (const locale of ["fr", "pt-BR"] as const) {
      for (const [path, forms] of plurals(CATALOGS[locale])) {
        if (forms.other.includes("{count}") && !neverZero.has(path)) expect(forms.one, `${locale} ${path}`).toContain("{count}");
      }
    }
  });

  it("warns and answers with the key when a plural message gets no numeric count", () => {
    setLocalePreference("en");
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(translate("labs.list.labCount")).toBe("labs.list.labCount");
    expect(translate("labs.list.labCount", { count: "3" })).toBe("labs.list.labCount");
    expect(warned).toHaveBeenCalledTimes(2);
    warned.mockRestore();
  });
});

describe("i18n: placeholders", () => {
  afterEach(() => {
    setLocalePreference("system");
  });

  it("writes out a zero, keeps $-patterns in values literal, and ignores extra variables", () => {
    setLocalePreference("en");
    expect(translate("common.units.seconds", { n: 0 })).toBe("0s");
    // String.replace's special patterns ($&, $1) must not expand inside a value.
    expect(translate("labs.readiness.or", { rest: "$&", last: "$1" })).toBe("$& or $1");
    expect(translate("common.units.seconds", { n: 1, extra: "x" })).toBe("1s");
  });

  it("the translator is one stable function per language", () => {
    setLocalePreference("fr");
    const a = renderHook(() => useT()).result.current;
    const b = renderHook(() => useT()).result.current;
    expect(a).toBe(b);
    expect(a.locale).toBe("fr");
  });
});

describe("i18n: the system language", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setLocalePreference("system");
  });

  const system = (languages: string[]) => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(languages);
    setLocalePreference("system");
    return getLocale();
  };

  it("matches the language whatever the case of the tag, and skips unsupported ones", () => {
    expect(system(["PT-br"])).toBe("pt-BR");
    expect(system(["zh-CN", "DE-at", "fr"])).toBe("de");
    expect(system(["ja-JP"])).toBe("ja");
  });

  it("is English when the list is empty or holds nothing supported", () => {
    expect(system([])).toBe("en");
    expect(system(["zh-CN", "ko"])).toBe("en");
  });

  it("ignores a saved language that differs only in case", () => {
    localStorage.setItem("cyberctf.locale", "EN");
    expect(getLocalePreference()).toBe("system");
  });
});

describe("i18n: rich text markup", () => {
  afterEach(() => {
    setLocalePreference("system");
  });

  // A key that is not in the catalogue comes back as itself, which lets the markup splitter be
  // fed arbitrary text without a message to hold it (the missing-key warning is silenced).
  const richOf = (text: string, tags: Record<string, (s: string) => ReactNode>) => {
    const warned = vi.spyOn(console, "warn").mockImplementation(() => {});
    const t = renderHook(() => useT()).result.current;
    const out = t.rich(text as never, tags);
    warned.mockRestore();
    return out;
  };

  it("hands a nested tag's raw text to the outer renderer, and leaves an unclosed tag as text", () => {
    const a = vi.fn((s: string) => s);
    const b = vi.fn((s: string) => s);
    expect(richOf("<a><b>x</b></a>", { a, b })).toHaveLength(1);
    expect(a).toHaveBeenCalledWith("<b>x</b>");
    expect(b).not.toHaveBeenCalled();
    expect(richOf("<a>x", { a })).toEqual(["<a>x"]);
  });

  it("does not pair a tag across a line break, and renders the same tag twice", () => {
    const a = vi.fn((s: string) => s);
    expect(richOf("<a>x\ny</a>", { a })).toEqual(["<a>x\ny</a>"]);
    expect(a).not.toHaveBeenCalled();
    const out = richOf("<a>1</a> and <a>2</a>", { a });
    expect(a.mock.calls.map((c) => c[0])).toEqual(["1", "2"]);
    expect(out).toHaveLength(3);
  });
});
