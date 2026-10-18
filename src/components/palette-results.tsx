"use client";

import { CornerDownLeft } from "lucide-react";
import type { Command } from "@/components/command-palette";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** The id of the `i`th option of the list `listId`. */
export const optionId = (listId: string, i: number) => `${listId}-opt-${String(i)}`;

/** The matching commands under their group headings, or a "no results" line. */
export function PaletteResults({
  listId,
  results,
  selected,
  onHover,
  onRun,
}: {
  listId: string;
  results: Command[];
  selected: number;
  onHover: (i: number) => void;
  onRun: (c: Command) => void;
}) {
  const t = useT();
  if (results.length === 0)
    return (
      <p role="status" className="px-4 py-8 text-center text-[0.8125rem] text-muted-foreground">
        {t("shell.palette.noResults")}
      </p>
    );
  return (
    <div id={listId} role="listbox" aria-label={t("shell.palette.label")}>
      {results.map((c, i) => {
        const heading = c.group && c.group !== results[i - 1]?.group ? c.group : null;
        return (
          <div key={c.id} role="presentation">
            {heading && (
              <div role="presentation" aria-hidden className="section-label px-4 pt-3 pb-1.5">
                {heading}
              </div>
            )}
            <PaletteOption
              id={optionId(listId, i)}
              command={c}
              active={i === selected}
              onHover={() => {
                onHover(i);
              }}
              onRun={() => {
                onRun(c);
              }}
            />
          </div>
        );
      })}
    </div>
  );
}

/** One command: an option, not a tab stop (focus stays in the field, aria-activedescendant). */
function PaletteOption({ id, command: c, active, onHover, onRun }: { id: string; command: Command; active: boolean; onHover: () => void; onRun: () => void }) {
  const Icon = c.icon;
  return (
    <div
      id={id}
      role="option"
      aria-selected={active}
      data-active={active}
      onMouseMove={onHover}
      // Focus stays in the field (a click would otherwise blur it).
      onMouseDown={(e) => {
        e.preventDefault();
      }}
      onClick={onRun}
      className={cn(
        "flex h-9 w-full cursor-pointer items-center gap-3 px-4 text-left text-[0.8125rem] transition-colors",
        active ? "bg-glass-2 text-foreground" : "text-muted-foreground",
      )}
    >
      <Icon className="size-3.5 shrink-0 text-faint" />
      <span className="flex-1 truncate">{c.label}</span>
      {c.hint && <span className="truncate font-mono text-[0.625rem] text-faint">{c.hint}</span>}
      {active && <CornerDownLeft className="size-3.5 shrink-0 text-faint" />}
    </div>
  );
}
