/**
 * State kept at module level (shared polls, the deploy store, self-test runs) lives for the
 * app's lifetime. Each such module registers here how to put it back to its first value, so the
 * test setup can start every test from a clean window with one call, without importing (and so
 * loading) each module itself.
 */
const resets = new Set<() => void>();

/** Registers how to put this module's state back to its first value. */
export function registerReset(reset: () => void) {
  resets.add(reset);
}

/** Puts every loaded module's state back to its first value (between tests). */
export function resetModuleState() {
  for (const reset of resets) reset();
}
