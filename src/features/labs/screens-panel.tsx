"use client";

import { Monitor } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import type { LabMachine } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The machines whose screen the lab page offers: the lab's own VMs whose run says anything of
 *  their screen (a container says nothing), the attack box apart (its card has its own). */
export function screenMachines(machines: LabMachine[]): LabMachine[] {
  return machines.filter((m) => !m.infra && m.console !== null);
}

/** Whether `m`'s screen can open now: it has one, and it runs. */
export function screenOpens(m: LabMachine): boolean {
  return m.console !== null && m.console.protocol !== "none" && m.state === "running";
}

/** The lab's machines with a screen (their graphical console), each with a Screen button that
 *  opens it in its own window, or why it has none (the run's words) or waits (not running). */
export function ScreensPanel({ machines, onOpen, error }: { machines: LabMachine[]; onOpen: (machine: string) => void; error?: string | null }) {
  const t = useT();
  const shown = screenMachines(machines);
  if (shown.length === 0) return null;
  return (
    <Panel>
      <PanelHeader title={t("labs.screens.title")} />
      <ul className="divide-y divide-border">
        {shown.map((m) => (
          <li key={m.name} className="flex items-center justify-between gap-3 px-4 py-2.5">
            <div className="min-w-0">
              <p className="truncate font-mono text-[0.75rem]">{m.name}</p>
              <ScreenNote machine={m} />
            </div>
            {screenOpens(m) && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  onOpen(m.name);
                }}
                aria-label={t("labs.screens.openOf", { machine: m.name })}
              >
                <Monitor className="size-3.5" /> {t("labs.screens.open")}
              </Button>
            )}
          </li>
        ))}
      </ul>
      {error && <p className="border-t border-border px-4 py-2 text-[0.75rem] text-destructive">{error}</p>}
    </Panel>
  );
}

/** Under a machine's name: its screen's protocol, why it has none, or that it waits for the machine. */
function ScreenNote({ machine: m }: { machine: LabMachine }) {
  const t = useT();
  const c = m.console;
  if (!c) return null;
  const text =
    c.protocol === "none"
      ? t("labs.screens.none", { why: c.note ?? "" })
      : m.state !== "running"
        ? t("labs.screens.notRunning")
        : t(c.protocol === "rdp" ? "labs.screens.rdp" : "labs.screens.vnc");
  return <p className="text-[0.6875rem] leading-snug text-muted-foreground">{text}</p>;
}
