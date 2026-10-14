"use client";

import { cn } from "@/lib/utils";

/** A row of mutually exclusive options in one glass track (filters, view switches, small settings). */
export function Segmented<T extends string | number | null>({
  label,
  value,
  onChange,
  options,
  className,
}: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string }[];
  className?: string;
}) {
  // A radio group as WAI-ARIA has it: one Tab stop (the checked option), arrows move the choice.
  const checked = Math.max(
    0,
    options.findIndex((o) => o.value === value),
  );
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    const to = e.key === "Home" ? 0 : e.key === "End" ? options.length - 1 : step !== undefined ? (checked + step + options.length) % options.length : null;
    if (to === null || options.length === 0) return;
    e.preventDefault();
    onChange(options[to].value);
    e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="radio"]')[to]?.focus();
  };
  return (
    <div
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cn("inline-flex items-center gap-0.5 rounded-control bg-glass p-[0.1875rem] shadow-[inset_0_0_0_1px_var(--border)]", className)}
    >
      {options.map((o, i) => (
        <button
          key={String(o.value)}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          tabIndex={i === checked ? 0 : -1}
          onClick={() => onChange(o.value)}
          className={cn(
            "h-7 rounded-[0.4375rem] px-3 text-[0.75rem] transition-colors",
            value === o.value ? "bg-glass-2 text-foreground shadow-[inset_0_0_0_1px_var(--input)]" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
