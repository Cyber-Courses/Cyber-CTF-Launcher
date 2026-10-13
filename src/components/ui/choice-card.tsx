"use client";

import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

// Pick one of several equivalent options, as a grid of cards (e.g. container engines).

const BADGE = { "in use": "inUse", running: "running", installed: "installed", recommended: "recommended" } as const;

export function ChoiceGrid({ children }: { children: React.ReactNode }) {
  return (
    <div role="radiogroup" className="grid gap-2 sm:grid-cols-2">
      {children}
    </div>
  );
}

/** One option: selecting it shows its install / status below, the others stay alternatives. */
export function Choice({
  selected,
  onSelect,
  mark,
  title,
  note,
  badge,
}: {
  selected: boolean;
  onSelect: () => void;
  mark: React.ReactNode;
  title: string;
  note: string;
  badge?: keyof typeof BADGE;
}) {
  const t = useT();
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={cn(
        "flex items-start gap-3 rounded-control p-3 text-left transition-[background-color,box-shadow]",
        selected
          ? "bg-jewel/[0.06] shadow-[inset_0_0_0_1px_var(--jewel),0_0_0_3px_rgb(var(--jewel-rgb)/0.12)]"
          : "bg-glass shadow-[inset_0_0_0_1px_var(--border)] hover:shadow-[inset_0_0_0_1px_var(--input)]",
      )}
    >
      {mark}
      <span className="min-w-0 flex-1">
        {/* The badge drops under the name when both don't fit; neither wraps inside itself. */}
        <span className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.8125rem] font-medium text-foreground">
          <span className="min-w-0 break-words">{title}</span>
          {badge && (
            <span
              className={cn(
                "rounded-full px-2 py-px font-mono text-[0.625rem] font-normal whitespace-nowrap",
                badge === "recommended"
                  ? "text-jewel-text shadow-[inset_0_0_0_1px_rgb(var(--jewel-rgb)/0.4)]"
                  : "text-success shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--success)_35%,transparent)]",
              )}
            >
              {t(`ui.choice.${BADGE[badge]}`)}
            </span>
          )}
        </span>
        <span className="mt-0.5 block text-[0.75rem] text-muted-foreground">{note}</span>
      </span>
      <span className={cn("mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-full border-[1.5px]", selected ? "border-jewel" : "border-input")}>
        {selected && <span className="size-2 rounded-full bg-jewel" />}
      </span>
    </button>
  );
}

/** What to do for the selected option: install it, get it, or nothing (it's ready). */
export function ChoiceAction({ children }: { children: React.ReactNode }) {
  return <div className="surface-panel flex min-h-12 items-center justify-between gap-3 rounded-control px-3.5 py-2.5 text-left">{children}</div>;
}
