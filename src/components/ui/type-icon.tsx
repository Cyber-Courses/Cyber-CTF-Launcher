import type { ReactNode } from "react";

/** The square icon tile at the start of a list row (provider, runtime). */
export function TypeIcon({ children }: { children: ReactNode }) {
  return (
    <span className="flex size-8 shrink-0 items-center justify-center rounded-control bg-glass-2 text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]">
      {children}
    </span>
  );
}
