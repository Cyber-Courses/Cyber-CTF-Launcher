"use client";

import { AppWindow, Monitor } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import type { LabConsole, LabMachine } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The machines whose screen the lab page offers: the lab's own VMs whose run says anything of
 *  their screen (a container says nothing), the attack box apart (its card has its own). */
export function screenMachines(machines: LabMachine[]): LabMachine[] {
  return machines.filter((m) => !m.infra && m.console !== null);
}

/** Whether `m`'s screen can open now: it has one, it runs, and the run found it there and
 *  answering (a VM started before screens were given one has none). */
export function screenOpens(m: LabMachine): boolean {
  return m.console !== null && m.console.protocol !== "none" && m.state === "running" && !m.console.unavailable;
}

/** The app of `m`'s hypervisor this computer has to open it in, once it runs (none for
 *  vagrant-qemu, or an app this computer lacks). */
export function nativeApp(m: LabMachine): string | null {
  return m.state === "running" ? (m.console?.nativeApp ?? null) : null;
}

/** The lab's machines with a screen (their graphical console), each with a Screen button that
 *  opens it in its own window, or why it has none (the run's words) or waits (not running, or
 *  being set up), and an "Open in …" button where this computer has the hypervisor's own app. */
export function ScreensPanel({
  machines,
  onOpen,
  onOpenNative,
  error,
}: {
  machines: LabMachine[];
  onOpen: (machine: string) => void;
  onOpenNative?: (machine: string) => void;
  error?: string | null;
}) {
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
            <div className="flex shrink-0 gap-2">
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
              <NativeButton machine={m} onOpen={onOpenNative} />
            </div>
          </li>
        ))}
      </ul>
      {error && <p className="border-t border-border px-4 py-2 text-[0.75rem] text-destructive">{error}</p>}
    </Panel>
  );
}

/** "Open in VirtualBox": `machine` in its hypervisor's own app, where this computer has it. */
function NativeButton({ machine: m, onOpen }: { machine: LabMachine; onOpen?: (machine: string) => void }) {
  const t = useT();
  const app = nativeApp(m);
  if (!app || !onOpen) return null;
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={() => {
        onOpen(m.name);
      }}
      aria-label={t("labs.screens.openNativeOf", { machine: m.name, app })}
    >
      <AppWindow className="size-3.5" /> {t("labs.screens.openNative", { app })}
    </Button>
  );
}

/** What to say of `m`'s screen `c`, a running machine's: why it can't show now, or how it shows. */
function runningNote(c: LabConsole, t: ReturnType<typeof useT>): string {
  if (c.unavailable) return t("labs.screens.unavailable", { why: c.unavailable });
  return t(c.protocol === "rdp" ? "labs.screens.rdp" : "labs.screens.vnc");
}

/** Under a machine's name: its screen's protocol, why it has none or can't show now, or that it
 *  waits for the machine (being set up, or not running). */
function ScreenNote({ machine: m }: { machine: LabMachine }) {
  const t = useT();
  const c = m.console;
  if (!c) return null;
  let text: string;
  if (c.protocol === "none") text = t("labs.screens.none", { why: c.note ?? "" });
  else if (m.state === "busy") text = t("labs.screens.busy");
  else if (m.state !== "running") text = t("labs.screens.notRunning");
  else text = runningNote(c, t);
  return <p className="text-[0.6875rem] leading-snug text-muted-foreground">{text}</p>;
}
