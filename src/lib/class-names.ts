/** Minimal className joiner (clsx-style) with no runtime deps, so the launcher's design
 * primitives stay dependency-free. Later we can swap in clsx + tailwind-merge if needed. */
export type ClassValue = string | number | null | false | undefined | ClassValue[];

export function cn(...inputs: ClassValue[]): string {
  const out: string[] = [];
  const walk = (v: ClassValue) => {
    if (v === null || v === undefined || v === false || v === "") return;
    if (Array.isArray(v)) v.forEach(walk);
    else out.push(String(v));
  };
  inputs.forEach(walk);
  return out.join(" ");
}
