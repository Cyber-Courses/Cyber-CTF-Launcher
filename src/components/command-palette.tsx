"use client";

import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Search, type LucideIcon } from "lucide-react";
import { PaletteResults, optionId } from "@/components/palette-results";
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

/** The commands matching `query` (label, hint and keywords), all of them for an empty one. */
function matching(commands: Command[], query: string): Command[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  return commands.filter((c) => `${c.label} ${c.hint ?? ""} ${c.keywords ?? ""}`.toLowerCase().includes(q));
}

/** An Enter that confirms an IME composition (Japanese): the input method's, not a choice. */
function composing(e: React.KeyboardEvent): boolean {
  // eslint-disable-next-line @typescript-eslint/no-deprecated -- WebKit (the macOS webview) sends the Enter that ends a composition with isComposing false; keyCode 229 is its only mark
  return e.nativeEvent.isComposing || e.keyCode === 229;
}

/** The palette's keys: Esc closes, the arrows move the selection (wrapping), Enter runs it. */
function paletteKeys({
  count,
  selected,
  setActive,
  onClose,
  onEnter,
}: {
  count: number;
  selected: number;
  setActive: (i: number) => void;
  onClose: () => void;
  onEnter: () => void;
}) {
  return (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive(count ? (selected + 1) % count : 0);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive(count ? (selected - 1 + count) % count : 0);
    } else if (e.key === "Enter" && e.target instanceof HTMLInputElement && !composing(e)) {
      // From the search field only: a command reached with Tab runs itself (its own button).
      e.preventDefault();
      onEnter();
    }
  };
}

/** Focuses the field when it appears, and keeps the selected row visible: DOM side effects only. */
function usePaletteFocus(selected: number) {
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);
  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>("[data-active=true]")?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  return { inputRef, listRef };
}

/**
 * ⌘K / Ctrl+K command palette: fuzzy-filter a flat list of actions (navigate, open settings,
 * find a lab…), arrow keys to move, Enter to run, Esc to close. Rendered by the app shell, which
 * owns open/close; this component owns the query, selection and keyboard handling while open.
 */
export function CommandPalette({ open, onClose, commands }: { open: boolean; onClose: () => void; commands: Command[] }) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const dialogRef = useRef<HTMLDivElement>(null);
  // Ids for the combobox wiring: the field controls the list and points at the active option.
  const listId = useId();
  // A modal: Tab stays in the palette instead of reaching the page behind it, and focus goes
  // back where it was when it closes.
  useFocusTrap(dialogRef, open);

  const results = useMemo(() => matching(commands, query), [query, commands]);

  // The selection, kept in range as results shrink (clamped during render, not in an effect).
  const selected = Math.min(active, Math.max(0, results.length - 1));

  const { inputRef, listRef } = usePaletteFocus(selected);

  const run = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };
  const onKey = paletteKeys({
    count: results.length,
    selected,
    setActive,
    onClose,
    onEnter: () => {
      run(results[selected]);
    },
  });

  if (!open) return null;
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
        <PaletteField
          inputRef={inputRef}
          listId={results.length ? listId : undefined}
          activeId={results.length ? optionId(listId, selected) : undefined}
          query={query}
          onQuery={setQuery}
        />
        <div ref={listRef} className="max-h-[24rem] overflow-y-auto pb-2">
          <PaletteResults listId={listId} results={results} selected={selected} onHover={setActive} onRun={run} />
        </div>
      </div>
    </div>
  );
}

/** The search field: a combobox over the results list (`listId`, none while nothing matches). */
function PaletteField({
  inputRef,
  listId,
  activeId,
  query,
  onQuery,
}: {
  inputRef: React.RefObject<HTMLInputElement | null>;
  listId: string | undefined;
  activeId: string | undefined;
  query: string;
  onQuery: (query: string) => void;
}) {
  const t = useT();
  return (
    <div className="flex items-center gap-3 border-b border-border px-4">
      <Search className="size-4 shrink-0 text-faint" />
      <input
        ref={inputRef}
        role="combobox"
        aria-expanded={listId !== undefined}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={activeId}
        aria-label={t("shell.palette.label")}
        value={query}
        onChange={(e) => {
          onQuery(e.target.value);
        }}
        placeholder={t("shell.palette.placeholder")}
        className="h-13 w-full bg-transparent text-[0.9375rem] outline-none placeholder:text-faint"
      />
      <kbd className="kbd">esc</kbd>
    </div>
  );
}
