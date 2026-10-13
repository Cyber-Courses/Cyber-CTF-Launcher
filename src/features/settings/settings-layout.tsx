"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check } from "lucide-react";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/* ------------------------------------------------------------------ layout */

export function Section({ title, description, saved, children }: { title: string; description?: string; saved?: boolean; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <SectionTitle title={title} description={description} saved={saved} />
      <Card className="overflow-hidden">{children}</Card>
    </section>
  );
}

/** A section's serif title and muted description, with the "Saved" mark on the right. */
export function SectionTitle({ title, description, saved }: { title: string; description?: ReactNode; saved?: boolean }) {
  const t = useT();
  return (
    <div className="flex items-end justify-between gap-4 px-0.5">
      <div className="min-w-0">
        <h2 className="serif-title text-[1.5rem] text-foreground">{title}</h2>
        {description && <p className="mt-1.5 text-[0.8125rem] text-muted-foreground">{description}</p>}
      </div>
      <span
        aria-live="polite"
        className={cn(
          "flex shrink-0 items-center gap-1 font-mono text-[0.6875rem] text-success transition-opacity duration-300",
          saved ? "opacity-100" : "opacity-0",
        )}
      >
        <Check className="size-3.5" /> {t("settings.saved")}
      </span>
    </div>
  );
}

/** One setting: label + description on the left, its control on the right (or below, when `stacked`).
 *  The row is a size container: narrower than 26.25rem (420px, a small window or a long German
 *  label beside a wide control), the label goes above its control instead of folding into a
 *  column a word wide. */
export function Row({
  title,
  description,
  control,
  stacked,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  control?: ReactNode;
  stacked?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className="@container border-t border-border px-4 py-3.5 first:border-t-0">
      <div
        className={cn(
          "flex flex-col",
          stacked ? "gap-0" : "gap-2.5 @min-[26.25rem]:flex-row @min-[26.25rem]:items-center @min-[26.25rem]:justify-between @min-[26.25rem]:gap-6",
        )}
      >
        <div className="min-w-0">
          <div className="text-[0.8125rem] font-medium text-foreground">{title}</div>
          {description && <div className="mt-0.5 text-[0.75rem] leading-relaxed text-muted-foreground">{description}</div>}
        </div>
        {control && <div className="max-w-full shrink-0 self-start @min-[26.25rem]:self-auto">{control}</div>}
      </div>
      {children}
    </div>
  );
}

/** Flashes the section's "Saved" mark for a moment after a change. */
export function useSavedFlash(): [boolean, () => void] {
  const [saved, setSaved] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );
  return [
    saved,
    () => {
      setSaved(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setSaved(false), 1600);
    },
  ];
}
