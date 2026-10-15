"use client";

import { useSyncExternalStore } from "react";
import { CATALOGS } from "@/messages";

/**
 * The launcher's language: English (the source of truth), French, Spanish, German, Brazilian
 * Portuguese or Japanese. Chosen in Settings, else the
 * system's; saved on this computer and followed by every window through the `storage` event, like
 * the appearance. No library: typed JSON messages per area (src/messages/<lang>/<area>.json).
 *
 *   const t = useT();
 *   t("labs.detail.stop")                      // plain
 *   t("labs.list.count", { count: 3 })         // {count} interpolation; { one, other } plurals
 *   t.rich("settings.help", { link: (s) => <a>{s}</a> })   // <link>…</link> markup in the text
 *
 * Outside React (error helpers, notifications): `translate(key, vars)` in the current language.
 */
export const LOCALES = ["en", "fr", "es", "de", "pt-BR", "ja"] as const;
export type Locale = (typeof LOCALES)[number];
/** Each language in itself, as the language picker shows it. */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: "English",
  fr: "Français",
  es: "Español",
  de: "Deutsch",
  "pt-BR": "Português (Brasil)",
  ja: "日本語",
};

type Messages = (typeof CATALOGS)["en"];
// "a.b.c" for every leaf (string, or a { one, other } plural) of the English messages.
type Leaf = string | { one: string; other: string };
type Paths<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends Leaf ? `${P}${K}` : Paths<T[K], `${P}${K}.`>;
}[keyof T & string];
export type MessageKey = Paths<Messages>;
export type Vars = Record<string, string | number>;

const KEY = "cyberctf.locale";
const listeners = new Set<() => void>();
let current: Locale | null = null;

function parse(v: string | null | undefined): Locale | null {
  return (LOCALES as readonly string[]).includes(v ?? "") ? (v as Locale) : null;
}

/** A system language tag ("pt-PT", "de-CH", "ja-JP") to ours: same language, any region. */
function fromSystem(tag: string | undefined): Locale | null {
  const lang = tag?.toLowerCase().split("-")[0];
  return LOCALES.find((l) => l.toLowerCase().split("-")[0] === lang) ?? null;
}

/** The system's language, when we have it; English otherwise. */
function systemLocale(): Locale {
  if (typeof navigator === "undefined") return "en";
  for (const l of navigator.languages.length > 0 ? navigator.languages : [navigator.language]) {
    const match = fromSystem(l);
    if (match) return match;
  }
  return "en";
}

/** The chosen language: "system" when none was chosen. */
export function getLocalePreference(): Locale | "system" {
  try {
    return parse(localStorage.getItem(KEY)) ?? "system";
  } catch {
    return "system";
  }
}

export function getLocale(): Locale {
  if (current) return current;
  const pref = getLocalePreference();
  current = pref === "system" ? systemLocale() : pref;
  // Screen readers and spell-check read the page's language from <html lang>.
  if (typeof document !== "undefined") document.documentElement.lang = current;
  return current;
}

function apply(next: Locale) {
  current = next;
  if (typeof document !== "undefined") document.documentElement.lang = next;
  listeners.forEach((l) => {
    l();
  });
}

export function setLocalePreference(pref: Locale | "system") {
  try {
    if (pref === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, pref);
  } catch {
    /* kept for this session only */
  }
  apply(pref === "system" ? systemLocale() : pref);
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  // Another window (Settings) changed it.
  const onStorage = (e: StorageEvent) => {
    if (e.key === KEY) apply(parse(e.newValue) ?? systemLocale());
  };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(cb);
    window.removeEventListener("storage", onStorage);
  };
}

function lookup(catalog: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>((node, part) => (node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined), catalog);
}

/** The message for `key` in `locale`, English when French lacks it, the key itself as a last resort. */
function resolve(locale: Locale, key: string, vars?: Vars): string {
  let msg = lookup(CATALOGS[locale], key) ?? lookup(CATALOGS.en, key);
  if (msg && typeof msg === "object" && vars && typeof vars.count === "number") {
    const forms = msg as Record<string, string>;
    msg = forms[new Intl.PluralRules(locale).select(vars.count)] ?? forms.other;
  }
  if (typeof msg !== "string") {
    if (process.env.NODE_ENV !== "production") console.warn(`i18n: missing message "${key}"`);
    return key;
  }
  return vars ? msg.replace(/\{(\w+)\}/g, (m, name: string) => (name in vars ? String(vars[name]) : m)) : msg;
}

/** `translate` in the current language, for code outside React components. */
export function translate(key: MessageKey, vars?: Vars): string {
  return resolve(getLocale(), key, vars);
}

/** How each tag of a message's markup renders; a tag without one shows as its text. */
type RichTags = Partial<Record<string, (chunks: string) => React.ReactNode>>;

/** Splits `<tag>text</tag>` markup into React nodes. */
function rich(text: string, tags: RichTags): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  const re = /<(\w+)>(.*?)<\/\1>/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const render = tags[m[1]];
    out.push(render ? <span key={m.index}>{render(m[2])}</span> : m[2]);
    last = re.lastIndex;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export type T = ((key: MessageKey, vars?: Vars) => string) & {
  rich: (key: MessageKey, tags: RichTags, vars?: Vars) => React.ReactNode[];
  locale: Locale;
};

function makeT(locale: Locale): T {
  const t = ((key: MessageKey, vars?: Vars) => resolve(locale, key, vars)) as T;
  t.rich = (key, tags, vars) => rich(resolve(locale, key, vars), tags);
  t.locale = locale;
  return t;
}

const tCache = new Map<Locale, T>();
function tFor(locale: Locale): T {
  let t = tCache.get(locale);
  if (!t) tCache.set(locale, (t = makeT(locale)));
  return t;
}

/** The current language, re-rendering when it changes (here or in another window). */
export function useLocale(): Locale {
  // The server render (static export) is English; the client takes the real one on hydration.
  return useSyncExternalStore(subscribe, getLocale, () => "en");
}

/** The translator for the current language. */
export function useT(): T {
  return tFor(useLocale());
}

/** Numbers, dates and relative times in the current language. */
export function useFormat() {
  const locale = useLocale();
  return {
    number: (n: number, o?: Intl.NumberFormatOptions) => new Intl.NumberFormat(locale, o).format(n),
    date: (d: Date | number, o?: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat(locale, o).format(d),
    relative: (value: number, unit: Intl.RelativeTimeFormatUnit) => new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(value, unit),
  };
}
