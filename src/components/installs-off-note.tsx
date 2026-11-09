"use client";

import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Why the install buttons beside it are off: this computer's policy (`DisableToolInstalls`). */
export function InstallsOffNote({ className }: { className?: string }) {
  const t = useT();
  return <p className={cn("text-[0.75rem] text-muted-foreground", className)}>{t("settings.managed.installsOff")}</p>;
}
