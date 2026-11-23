import { logFrontendWarning } from "@/lib/error-log";

// The quiet `.catch` handlers of `@/lib/failure` (see there): `ignore` and `warn`. Here, apart
// from `tell` and its toasts, so a module the language itself depends on (its saved choice, read
// through `@/lib/local-store`) can use them without a cycle back to the translations.

export const ignore =
  <T = undefined>(why: string, instead: () => T = () => undefined as T) =>
  (e: unknown): T => {
    console.debug(`ignored (${why}):`, e);
    return instead();
  };

export const warn =
  <T = undefined>(what: string, instead: () => T = () => undefined as T) =>
  (e: unknown): T => {
    console.warn(`${what}:`, e);
    logFrontendWarning(what, e);
    return instead();
  };
