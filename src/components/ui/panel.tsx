import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * The density unit of the launcher: a compact panel (gradient, lit top edge, hairline ring).
 * Rows inside are separated by `border-t border-border` hairlines.
 */
export function Panel({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("surface-panel min-w-0 overflow-hidden rounded-panel", className)}>{children}</div>;
}

/**
 * The panel's 2.75rem header: a title (optionally led by a status dot or icon), then mono meta
 * on the right (`meta`) and/or an action.
 */
export function PanelHeader({ title, meta, action, className }: { title: ReactNode; meta?: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <div className={cn("flex h-11 items-center gap-2.5 border-b border-border px-4", className)}>
      <h3 className="flex min-w-0 items-center gap-2.5 truncate text-[0.8125rem] font-medium text-foreground">{title}</h3>
      {(meta || action) && (
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {meta && <span className="font-mono text-[0.6875rem] text-faint">{meta}</span>}
          {action}
        </div>
      )}
    </div>
  );
}

/** A key/value line inside a panel (Details, Attack box). Value in mono, right-aligned. */
export function KeyValue({ k, children, className }: { k: ReactNode; children: ReactNode; className?: string }) {
  return (
    <div className={cn("flex items-baseline justify-between gap-4 border-t border-border px-4 py-2.5 text-[0.8125rem] first:border-t-0", className)}>
      <span className="shrink-0 text-muted-foreground">{k}</span>
      <span className="min-w-0 text-right font-mono text-[0.75rem] break-words text-foreground">{children}</span>
    </div>
  );
}
