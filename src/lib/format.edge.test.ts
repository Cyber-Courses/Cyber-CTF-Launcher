// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { formatAgo, formatBytes, formatDuration, formatElapsed, formatUsd } from "@/lib/format";
import { setLocalePreference } from "@/lib/i18n";

// Boundary values of the shared formatters: the exact tick where each form changes, the far
// ends (a day-long elapsed, a clock that went backwards) and inputs that are not numbers.
describe("format: boundaries", () => {
  beforeEach(() => setLocalePreference("en"));
  afterEach(() => setLocalePreference("system"));

  it("switches from tenths to whole seconds at exactly 10 s, and to m:ss at exactly 60 s", () => {
    expect(formatDuration(0)).toBe("0.0s");
    // 9.999 s rounds to one decimal as 10.0, still in the tenths form.
    expect(formatDuration(9_999)).toBe("10.0s");
    expect(formatDuration(10_000)).toBe("10s");
    expect(formatDuration(59_999)).toBe("59s");
    expect(formatDuration(60_000)).toBe("1:00");
  });

  it("keeps counting in minutes past an hour and past a day: there is no h:mm:ss form", () => {
    expect(formatElapsed(3_599_999)).toBe("59:59");
    expect(formatElapsed(3_600_000)).toBe("60:00");
    expect(formatElapsed(25 * 3_600_000)).toBe("1500:00");
  });

  it("a non-finite duration reads as zero", () => {
    // `NaN < 10_000` is false, so formatDuration(NaN) goes through formatElapsed, where
    // Math.max(0, NaN) is NaN and the m:ss branch prints "NaN:NaN"; Infinity prints
    // "Infinity:NaN". A missing timestamp (a garbage last-run value, see last-run) reaches
    // these. Suggested fix: `if (!Number.isFinite(ms)) ms = 0;` at the top of both.
    expect(formatDuration(NaN)).not.toMatch(/NaN/);
    expect(formatElapsed(NaN)).not.toMatch(/NaN/);
    expect(formatDuration(Infinity)).not.toMatch(/Infinity|NaN/);
  });

  it("flips to minutes at exactly 60 s, to hours at 3600 s and to days at 86400 s", () => {
    const now = 1_700_000_000_000;
    expect(formatAgo(now - 59_999, now)).toBe("just now");
    expect(formatAgo(now - 60_000, now)).toBe("1 min ago");
    expect(formatAgo(now - 3_599_999, now)).toBe("59 min ago");
    expect(formatAgo(now - 3_600_000, now)).toBe("1 h ago");
    expect(formatAgo(now - 86_399_999, now)).toBe("23 h ago");
    expect(formatAgo(now - 86_400_000, now)).toBe("yesterday");
    expect(formatAgo(now - 172_799_999, now)).toBe("yesterday");
    expect(formatAgo(now - 172_800_000, now)).toBe("2 days ago");
    expect(formatAgo(now - 400 * 86_400_000, now)).toBe("400 days ago");
  });

  it("reads a clock that went backwards (a time in the future) as just now, however far", () => {
    const now = 1_700_000_000_000;
    expect(formatAgo(now, now)).toBe("just now");
    expect(formatAgo(now + 30 * 86_400_000, now)).toBe("just now");
  });

  it("rounds sizes at each unit boundary", () => {
    expect(formatBytes(999_999)).toBe("1000 kB");
    expect(formatBytes(1e6)).toBe("1 MB");
    expect(formatBytes(1e9 - 1)).toBe("1000 MB");
    expect(formatBytes(1e9)).toBe("1.0 GB");
    // One decimal under 10 GB: 9.99 rounds up to 10.0, still with the decimal.
    expect(formatBytes(9.99e9)).toBe("10.0 GB");
    expect(formatBytes(10e9)).toBe("10 GB");
    expect(formatBytes(1e12)).toBe("1000 GB");
  });

  it("never shows less than 1 kB, even for zero or negative sizes", () => {
    expect(formatBytes(0)).toBe("1 kB");
    expect(formatBytes(499)).toBe("1 kB");
    expect(formatBytes(-5e9)).toBe("1 kB");
  });

  it("a non-finite size reads as the smallest size", () => {
    // Math.max(1, NaN) is NaN, and Intl formats Infinity as "∞". A metric the backend could
    // not read (a server with no capacity figures) reaches these. Suggested fix: return the
    // kilobyte floor (or a dash) when `!Number.isFinite(b)`.
    expect(formatBytes(NaN)).not.toMatch(/NaN/);
    expect(formatBytes(Infinity)).not.toMatch(/∞|Infinity/);
  });

  it("uses each language's decimal separator for tenths and gigabytes", () => {
    setLocalePreference("de");
    expect(formatDuration(1_500)).toMatch(/^1,5/);
    expect(formatBytes(2.5e9)).toMatch(/^2,5/);
    setLocalePreference("pt-BR");
    expect(formatBytes(2.5e9)).toMatch(/^2,5/);
    setLocalePreference("ja");
    expect(formatDuration(1_500)).toMatch(/^1\.5/);
    expect(formatBytes(2.5e9)).toMatch(/^2\.5/);
  });
});

describe("formatUsd", () => {
  it("writes dollars in the language's notation", () => {
    expect(formatUsd(0.02, "en")).toBe("$0.02");
    expect(formatUsd(0.02, "fr")).toBe("0,02 $US");
    expect(formatUsd(6.4, "de")).toBe("6,40 $");
  });

  it("shows two significant digits under a cent, not $0.00", () => {
    expect(formatUsd(0.0042, "en")).toBe("$0.0042");
    expect(formatUsd(0.004, "en")).toBe("$0.004");
    expect(formatUsd(0, "en")).toBe("$0.00");
  });

  it("drops the cents of a whole budget", () => {
    expect(formatUsd(50, "en", { whole: true })).toBe("$50");
  });
});
