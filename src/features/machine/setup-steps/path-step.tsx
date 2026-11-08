"use client";

import { Container, Server, type LucideIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TypeIcon } from "@/components/ui/type-icon";
import { PATHS, type SetupPath } from "@/features/machine/setup-steps/steps";
import { pathProgress, type LastTests } from "@/features/machine/setup-steps/step-status";
import type { Provider, SystemReport } from "@/lib/tauri";
import { useT, type MessageKey } from "@/lib/i18n";

const ICONS: Record<SetupPath, LucideIcon> = { docker: Container, vm: Server };
const NAMES: Record<SetupPath, MessageKey> = { docker: "machine.setup.paths.docker", vm: "machine.setup.paths.vm" };
const NOTES: Record<SetupPath, MessageKey> = { docker: "machine.setup.paths.dockerNote", vm: "machine.setup.paths.vmNote" };

/** The setup window's first screen: the providers this machine can have, one row each (a
 *  container engine, a hypervisor for VMs) with where this machine stands and the way into its
 *  path. This machine only: cloud accounts and servers have their own pages. The first path
 *  with something left to do carries the jewel. */
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
        const ready = done === total;
        return (
          <Row key={path} icon={ICONS[path]} title={t(NAMES[path])} note={t(NOTES[path])}>
            <span className="shrink-0 font-mono text-[0.6875rem] tabular-nums text-faint">
              {ready ? t("machine.setup.paths.ready") : t("machine.setup.paths.progress", { done, total })}
            </span>
            <Button
              variant={path === jewel ? "primary" : "outline"}
              size="xs"
              onClick={() => {
                onStart(path);
              }}
            >
              {ready ? t("machine.setup.paths.review") : t("machine.setup.paths.setUp")}
            </Button>
          </Row>
        );
      })}
    </div>
  );
}

/** One row: the provider kind's icon, its name and note, then its standing and action. */
function Row({ icon: Icon, title, note, children }: { icon: LucideIcon; title: string; note: string; children: React.ReactNode }) {
  return (
    <div className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2.5 text-left first:border-t-0">
      <TypeIcon>
        <Icon className="size-4" />
      </TypeIcon>
      <div className="min-w-0 flex-1">
        <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
        <p className="text-[0.75rem] text-muted-foreground">{note}</p>
      </div>
      {children}
    </div>
  );
}
