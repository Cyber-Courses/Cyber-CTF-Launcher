"use client";

import { runPlaces } from "@/features/labs/lab-runtime";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import type { LabStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/** Every place a lab could run, as small icons: where it runs (green), where it can (jewel), and
 *  where it can't (greyed), so a row reads as the full set of options at a glance. */
export function RunPlaceIcons({
  runtime,
  hostArch,
  emulates,
  running,
  status,
}: {
  runtime: LabRuntimeInfo;
  hostArch: string;
  emulates: boolean;
  running: boolean;
  status?: LabStatus;
}) {
  const t = useT();
  return (
    // Above a lab row's stretched title (see LabRow), so each icon's hint shows on hover.
    <span className="relative z-[1] inline-flex items-center gap-1">
      {runPlaces(runtime, hostArch, emulates).map(({ key, icon: Icon, available }) => {
        const label = t(`labs.places.${key}`);
        const inUse = running && status?.place === key;
        const hint = inUse
          ? t("labs.row.runningOn", { where: status.host ?? label })
          : available
            ? t("labs.row.canRunOn", { where: label })
            : t("labs.row.notAvailable", { where: label });
        return (
          <span key={key} role="img" title={hint} aria-label={hint} className="inline-flex">
            <Icon className={cn("size-3", inUse ? "text-success" : available ? "text-jewel-text" : "text-faint opacity-50")} />
          </span>
        );
      })}
    </span>
  );
}
