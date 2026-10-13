"use client";

import { useLayoutEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";

/**
 * One option in a pick-one list (rows inside a bordered box). `compact` is the dense form
 * used in popovers; a disabled row stays visible and says why through `subtitle`/`hint`.
 * Inside a RadioList the group is one Tab stop (the selected row); arrow keys move the choice.
 */
export function RadioRow({
  selected,
  onSelect,
  title,
  subtitle,
  disabled,
  hint,
  compact,
  leading,
  trailing,
  children,
}: {
  selected: boolean;
  onSelect: () => void;
  title: ReactNode;
  subtitle: ReactNode;
  disabled?: boolean;
  /** Tooltip, e.g. why the option can't be picked. */
  hint?: string;
  compact?: boolean;
  /** A mark before the text (e.g. a provider logo). */
  leading?: ReactNode;
  /** Right-aligned extra (e.g. a status pill). */
  trailing?: ReactNode;
  children?: ReactNode;
}) {
  const pick = () => !disabled && onSelect();
  return (
    <div
      role="radio"
      aria-checked={selected}
      aria-disabled={disabled || undefined}
      // The selected row is the group's Tab stop; RadioList gives it to the first enabled row
      // when none is selected, and moves it with the arrow keys.
      tabIndex={selected && !disabled ? 0 : -1}
      title={hint}
      onClick={pick}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          pick();
        }
      }}
      className={cn(
        "flex gap-3 border-b border-border outline-none transition-colors last:border-0 focus-visible:bg-accent",
        compact ? "px-3 py-2" : "px-3.5 py-3",
        disabled ? "cursor-not-allowed opacity-45" : "cursor-pointer",
        selected ? "bg-jewel/[0.06]" : !disabled && "hover:bg-glass",
      )}
    >
      <span
        className={cn(
          "mt-0.5 flex shrink-0 items-center justify-center rounded-full border-[1.5px] transition-colors",
          compact ? "size-3.5" : "size-4",
          selected ? "border-jewel" : "border-input",
        )}
      >
        {selected && <span className={cn("rounded-full bg-jewel", compact ? "size-1.5" : "size-2")} />}
      </span>
      {leading && <span className="-my-0.5 shrink-0">{leading}</span>}
      <div className="min-w-0 flex-1">
        <div className={cn("flex flex-wrap items-center gap-1.5 font-medium text-foreground", compact ? "text-[0.78125rem]" : "text-[0.8125rem]")}>{title}</div>
        {/* Two lines before it cuts (a translated description runs longer); the whole text in the tooltip. */}
        <div
          title={typeof subtitle === "string" ? subtitle : undefined}
          className={cn("mt-0.5 line-clamp-2 break-words", compact ? "font-mono text-[0.6875rem] text-faint" : "text-[0.75rem] text-muted-foreground")}
        >
          {subtitle}
        </div>
        {children}
      </div>
      {trailing && <span className="shrink-0 self-center">{trailing}</span>}
    </div>
  );
}

const RADIOS = '[role="radio"]';
const enabledRadios = (group: HTMLElement) =>
  [...group.querySelectorAll<HTMLElement>(RADIOS)].filter((r) => r.closest('[role="radiogroup"]') === group && r.getAttribute("aria-disabled") !== "true");

/** The bordered box that holds a list of RadioRows: one Tab stop for the group (the selected
 *  row, else the first enabled one), and the arrow keys (Home, End) move and pick, as a native
 *  radio group does. */
export function RadioList({ label, className, children }: { label: string; className?: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  // Every render, exactly one Tab stop: the selected enabled row, else the first enabled one
  // (with nothing selected the rows all say -1, and the group must stay reachable).
  useLayoutEffect(() => {
    const group = ref.current;
    if (!group) return;
    const radios = enabledRadios(group);
    const stop = radios.find((r) => r.getAttribute("aria-checked") === "true") ?? radios[0];
    radios.forEach((r) => (r.tabIndex = r === stop ? 0 : -1));
  });
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const group = ref.current;
    const from = e.target as HTMLElement;
    if (!group || from.getAttribute("role") !== "radio") return;
    const radios = enabledRadios(group);
    const at = radios.indexOf(from);
    if (at < 0 || radios.length < 2) return;
    const step: Record<string, number> = { ArrowDown: at + 1, ArrowRight: at + 1, ArrowUp: at - 1, ArrowLeft: at - 1, Home: 0, End: radios.length - 1 };
    if (!(e.key in step)) return;
    e.preventDefault();
    const next = radios[(step[e.key] + radios.length) % radios.length];
    from.tabIndex = -1;
    next.tabIndex = 0;
    next.focus();
    // Moving picks, as a native radio group: the row's own click handler selects it.
    next.click();
  };
  return (
    <div ref={ref} role="radiogroup" aria-label={label} onKeyDown={onKeyDown} className={cn("surface-panel overflow-hidden rounded-control", className)}>
      {children}
    </div>
  );
}
