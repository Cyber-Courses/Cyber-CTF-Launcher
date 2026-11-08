"use client";

import { Cloud, Container, Server, type LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TypeIcon } from "@/components/ui/type-icon";
import { PATHS, type SetupPath } from "@/features/machine/setup-steps/steps";
import { pathProgress, type LastTests } from "@/features/machine/setup-steps/step-status";
import type { Provider, SystemReport } from "@/lib/tauri";
import { useT, type MessageKey } from "@/lib/i18n";

const ICONS: Record<SetupPath, LucideIcon> = { docker: Container, vm: Server, cloud: Cloud };
const NAMES: Record<SetupPath, MessageKey> = { docker: "machine.setup.paths.docker", vm: "machine.setup.paths.vm", cloud: "machine.setup.paths.cloud" };
const NOTES: Record<SetupPath, MessageKey> = {
  docker: "machine.setup.paths.dockerNote",
  vm: "machine.setup.paths.vmNote",
  cloud: "machine.setup.paths.cloudNote",
};

/** The setup window's first screen: one row per lab type, where this machine stands on it, and
 *  the way into its path. The first path with something left to do carries the jewel. */
export function PathChooser({
  report,
  last,
  preferred,
  onStart,
}: {
  report: SystemReport;
  last: LastTests;
  preferred: Provider | null;
  onStart: (path: SetupPath) => void;
}) {
  const t = useT();
  const rows = PATHS.map((path) => ({ path, ...pathProgress(path, report, last, preferred) }));
  const jewel = rows.find((r) => r.done < r.total)?.path;
  return (
    <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      {rows.map(({ path, done, total }) => {
        const Icon = ICONS[path];
        const ready = done === total;
        return (
          <div key={path} className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2.5 text-left first:border-t-0">
            <TypeIcon>
              <Icon className="size-4" />
            </TypeIcon>
            <div className="min-w-0 flex-1">
              <p className="text-[0.8125rem] font-medium text-foreground">{t(NAMES[path])}</p>
              <p className="text-[0.75rem] text-muted-foreground">{t(NOTES[path])}</p>
            </div>
            <span className="shrink-0 font-mono text-[0.6875rem] tabular-nums text-faint">
              {ready ? t("machine.setup.paths.ready") : t("machine.checklist.done", { done, total })}
            </span>
            <Button
              variant={path === jewel ? "primary" : "outline"}
              size="xs"
              onClick={() => {
                onStart(path);
              }}
            >
              {ready ? t("machine.setup.paths.review") : t("machine.checklist.setUp")}
            </Button>
          </div>
        );
      })}
    </div>
  );
}
