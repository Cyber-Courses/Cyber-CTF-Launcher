"use client";

import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** A small indeterminate spinner that inherits the current text color. */
export function Spinner({ className }: { className?: string }) {
  const t = useT();
  return (
    <span
      role="status"
      aria-label={t("ui.spinner.loading")}
      className={cn("inline-block size-4 animate-spin rounded-full border-2 border-current border-t-transparent opacity-70", className)}
    />
  );
}
