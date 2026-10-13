import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * The top of every screen: a serif title (Newsreader 350; wrap at most one word in <em> for the
 * jewel italic), an optional lead line or status strip below it, and actions on the right.
 */
export function PageHeader({
  title,
  lead,
  actions,
  className,
  focusTitle,
}: {
  title: ReactNode;
  lead?: ReactNode;
  actions?: ReactNode;
  className?: string;
  /** Moves the keyboard focus to the title on open (a page reached from a list that is gone). */
  focusTitle?: boolean;
}) {
  return (
    <div className={cn("flex flex-wrap items-end justify-between gap-4", className)}>
      <div className="min-w-0">
        <h1 className="page-title outline-none" tabIndex={focusTitle ? -1 : undefined} ref={focusTitle ? (el) => el?.focus() : undefined}>
          {title}
        </h1>
        {lead && <div className="mt-2 text-[0.875rem] text-muted-foreground">{lead}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

/** A mono status strip (lab state · runtime · commit · elapsed), with <b> for the key fact. */
export function StatusStrip({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={cn("flex flex-wrap items-center gap-x-2.5 gap-y-1 font-mono text-[0.6875rem] text-faint [&_b]:font-medium [&_b]:text-foreground", className)}
    >
      {children}
    </div>
  );
}
