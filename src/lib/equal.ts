/**
 * Whether two polled answers say the same thing: primitives by value, arrays, plain objects,
 * Sets and Maps by content (Set members by identity, Map values compared the same way). A poll
 * that read the same answer again keeps the previous object, so nothing reading it re-renders.
 */
export function structurallyEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (a instanceof Set || b instanceof Set) {
    if (!(a instanceof Set && b instanceof Set) || a.size !== b.size) return false;
    for (const v of a) if (!b.has(v)) return false;
    return true;
  }
  if (a instanceof Map || b instanceof Map) {
    if (!(a instanceof Map && b instanceof Map) || a.size !== b.size) return false;
    for (const [k, v] of a) if (!b.has(k) || !structurallyEqual(v, b.get(k))) return false;
    return true;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!(Array.isArray(a) && Array.isArray(b)) || a.length !== b.length) return false;
    return a.every((v, i) => structurallyEqual(v, b[i]));
  }
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  return ka.every((k) => Object.prototype.hasOwnProperty.call(b, k) && structurallyEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}
