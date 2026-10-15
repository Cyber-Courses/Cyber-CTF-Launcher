/**
 * Whether two polled answers say the same thing: primitives by value, arrays, plain objects,
 * Sets and Maps by content (Set members by identity, Map values compared the same way). A poll
 * that read the same answer again keeps the previous object, so nothing reading it re-renders.
 */
export function structurallyEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (a instanceof Set || b instanceof Set) return a instanceof Set && b instanceof Set && setsEqual(a, b);
  if (a instanceof Map || b instanceof Map) return a instanceof Map && b instanceof Map && mapsEqual(a, b);
  if (Array.isArray(a) || Array.isArray(b)) return Array.isArray(a) && Array.isArray(b) && arraysEqual(a, b);
  return recordsEqual(a as Record<string, unknown>, b as Record<string, unknown>);
}

function setsEqual(a: ReadonlySet<unknown>, b: ReadonlySet<unknown>): boolean {
  return a.size === b.size && [...a].every((v) => b.has(v));
}

function mapsEqual(a: ReadonlyMap<unknown, unknown>, b: ReadonlyMap<unknown, unknown>): boolean {
  return a.size === b.size && [...a].every(([k, v]) => b.has(k) && structurallyEqual(v, b.get(k)));
}

function arraysEqual(a: readonly unknown[], b: readonly unknown[]): boolean {
  return a.length === b.length && a.every((v, i) => structurallyEqual(v, b[i]));
}

function recordsEqual(a: Record<string, unknown>, b: Record<string, unknown>): boolean {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((k) => Object.prototype.hasOwnProperty.call(b, k) && structurallyEqual(a[k], b[k]));
}
