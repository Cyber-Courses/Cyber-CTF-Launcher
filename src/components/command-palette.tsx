"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { CornerDownLeft, Search, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useT } from "@/lib/i18n";

export type Command = {
  id: string;
  label: string;
  hint?: string;
  icon: LucideIcon;
  keywords?: string;
  /** Heading the command is listed under (Screens, Labs...). */
  group?: string;
  run: () => void;
};

/**
 * ⌘K / Ctrl+K command palette: fuzzy-filter a flat list of actions (navigate, open settings,
 * find a lab…), arrow keys to move, Enter to run, Esc to close. Rendered by the app shell, which
 * owns open/close; this component owns the query, selection and keyboard handling while open.
 */
export function CommandPalette({ open, onClose, commands }: { open: boolean; onClose: () => void; commands: Command[] }) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  // Ids for the combobox wiring: the field controls the list and points at the active option.
  const listId = useId();
  const optionId = (i: number) => `${listId}-opt-${i}`;
  // A modal: Tab stays in the palette instead of reaching the page behind it, and focus goes
  // back where it was when it closes.
  useFocusTrap(dialogRef, open);

  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter((c) => `${c.label} ${c.hint ?? ""} ${c.keywords ?? ""}`.toLowerCase().includes(q));
  }, [query, commands]);

  // The selection, kept in range as results shrink (clamped during render, not in an effect).
  const selected = Math.min(active, Math.max(0, results.length - 1));

  // Focus the field when it appears, and keep the selected row visible: DOM side effects only.
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>("[data-active=true]")?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  if (!open) return null;

  const run = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive(results.length ? (selected + 1) % results.length : 0);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive(results.length ? (selected - 1 + results.length) % results.length : 0);
    } else if (e.key === "Enter" && e.target instanceof HTMLInputElement && !e.nativeEvent.isComposing && e.keyCode !== 229) {
      // From the search field only: a command reached with Tab runs itself (its own button), and
      // an Enter that confirms an IME composition (Japanese) is the input method's, not a choice.
      e.preventDefault();
      run(results[selected]);
    }
  };

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[14vh] backdrop-blur-[3px]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        data-command-palette
        aria-modal="true"
        aria-label={t("shell.palette.label")}
        className="surface-glass w-full max-w-[36rem] overflow-hidden rounded-[1rem]"
        onKeyDown={onKey}
      >
        <div className="flex items-center gap-3 border-b border-border px-4">
          <Search className="size-4 shrink-0 text-faint" />
          <input
            ref={inputRef}
            role="combobox"
            aria-expanded={results.length > 0}
            aria-controls={results.length ? listId : undefined}
            aria-autocomplete="list"
            aria-activedescendant={results.length ? optionId(selected) : undefined}
            aria-label={t("shell.palette.label")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("shell.palette.placeholder")}
            className="h-13 w-full bg-transparent text-[0.9375rem] outline-none placeholder:text-faint"
          />
          <kbd className="kbd">esc</kbd>
        </div>
        <div ref={listRef} className="max-h-[24rem] overflow-y-auto pb-2">
          {results.length === 0 ? (
            <p role="status" className="px-4 py-8 text-center text-[0.8125rem] text-muted-foreground">
              {t("shell.palette.noResults")}
            </p>
          ) : (
            <div id={listId} role="listbox" aria-label={t("shell.palette.label")}>
              {results.map((c, i) => {
                const Icon = c.icon;
                const heading = c.group && c.group !== results[i - 1]?.group ? c.group : null;
                return (
                  <div key={c.id} role="presentation">
                    {heading && (
                      <div role="presentation" aria-hidden className="section-label px-4 pt-3 pb-1.5">
                        {heading}
                      </div>
                    )}
                    {/* An option, not a tab stop: focus stays in the field (aria-activedescendant). */}
                    <div
                      id={optionId(i)}
                      role="option"
                      aria-selected={i === selected}
                      data-active={i === selected}
                      onMouseMove={() => setActive(i)}
                      // Focus stays in the field (a click would otherwise blur it).
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => run(c)}
                      className={cn(
                        "flex h-9 w-full cursor-pointer items-center gap-3 px-4 text-left text-[0.8125rem] transition-colors",
                        i === selected ? "bg-glass-2 text-foreground" : "text-muted-foreground",
                      )}
                    >
                      <Icon className="size-3.5 shrink-0 text-faint" />
                      <span className="flex-1 truncate">{c.label}</span>
                      {c.hint && <span className="truncate font-mono text-[0.625rem] text-faint">{c.hint}</span>}
                      {i === selected && <CornerDownLeft className="size-3.5 shrink-0 text-faint" />}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
