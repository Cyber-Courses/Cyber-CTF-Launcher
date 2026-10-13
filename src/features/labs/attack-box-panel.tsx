"use client";

import { Crosshair, Play, Square, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { LogConsole } from "@/components/ui/log-console";
import { CopyValue } from "@/components/ui/copy-value";
import { KeyValue, Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import type { AttackBox } from "@/features/labs/use-attack-box";
import { useT } from "@/lib/i18n";

/** The attack box card in the lab page's side rail. `host` is the server a remote lab runs on.
 *  `controller`: the lab runs on QEMU, whose network links only its two VMs, so the lab's own
 *  controller is the attacker (no attack VM to start or stop). */
export function AttackBoxPanel({
  box,
  running,
  host,
  onShell,
  shellReady,
  controller = false,
}: {
  box: AttackBox;
  running: boolean;
  host: string | null;
  onShell?: () => void;
  shellReady?: boolean;
  controller?: boolean;
}) {
  const t = useT();
  const remote = host !== null;
  const { status, busy, log } = box;
  const name = controller ? "isoloom-controller" : box.name;
  const vm = box.kind === "vm";
  const tone = (remote && running) || status?.running ? "ok" : busy ? "warn" : "muted";
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone={tone} pulse={busy} />
            {controller ? t("labs.attackBox.attacker") : vm ? t("labs.attackBox.attackVm") : t("labs.attackBox.attackBox")}
          </>
        }
        action={
          <span className="inline-block max-w-[9.5rem] truncate align-bottom font-mono text-[0.6875rem] text-faint" title={name}>
            {name}
          </span>
        }
      />
      <KeyValue k={t("labs.attackBox.status")}>
        <BoxState running={running} remote={remote} host={host} up={!!status?.running} busy={busy} />
      </KeyValue>
      {status?.running && status.ip && (
        <KeyValue k={t("labs.attackBox.address")}>
          <CopyValue text={status.ip} />
        </KeyValue>
      )}
      <div className="space-y-3 border-t border-border p-4">
        <p className="text-[0.75rem] leading-relaxed text-muted-foreground">
          {remote
            ? t("labs.attackBox.remoteHint")
            : controller
              ? t("labs.attackBox.controllerHint")
              : vm
                ? t("labs.attackBox.vmHint")
                : t("labs.attackBox.containerHint")}
        </p>
        {onShell && shellReady && (
          <Button variant="primary" size="sm" className="w-full" onClick={onShell}>
            <Terminal className="size-3.5" /> {t("labs.attackBox.openShell")}
          </Button>
        )}
        {!remote && !controller && status && !status.imagePresent && !status.running && (
          <p className="flex items-start gap-2 text-[0.75rem] text-muted-foreground">
            <StatusDot tone="warn" className="mt-1.5 shrink-0" />
            <span>{t.rich("labs.attackBox.firstDownload", { name: (s) => <span className="font-mono text-foreground">{s}</span> }, { name })}</span>
          </p>
        )}
        {running && !remote && !controller && <BoxControls box={box} />}
        {(busy || log.some((l) => l.startsWith("✗"))) && log.length > 0 && (
          <LogConsole lines={log} running={busy} title={vm ? t("labs.attackBox.attackVm") : t("labs.attackBox.attackBox")} />
        )}
      </div>
    </Panel>
  );
}

/** Where the box stands: on the lab's server, running, starting, not started (or it starts with
 *  the lab). */
function BoxState({ running, remote, host, up, busy }: { running: boolean; remote: boolean; host: string | null; up: boolean; busy: boolean }) {
  const t = useT();
  if (remote && running) return <>{t("labs.attackBox.runningOn", { host: host ?? "" })}</>;
  if (up)
    return (
      <span className="inline-flex items-center gap-1.5">
        <Crosshair className="size-3.5 text-you-text" /> {t("labs.attackBox.running")}
      </span>
    );
  if (busy)
    return (
      <span className="inline-flex items-center gap-1.5 text-muted-foreground">
        <Spinner className="size-3" /> {t("labs.attackBox.starting")}
      </span>
    );
  return <span className="text-muted-foreground">{running ? t("labs.attackBox.notStarted") : t("labs.attackBox.startsWithLab")}</span>;
}

/** Start or stop a box on this machine by hand. */
function BoxControls({ box }: { box: AttackBox }) {
  const t = useT();
  const { status, busy } = box;
  return (
    <div className="space-y-2">
      {status?.running ? (
        <Button variant="outline" size="sm" className="w-full" onClick={() => box.stop()} disabled={busy}>
          <Square className="size-3.5" /> {busy ? t("labs.attackBox.working") : t("labs.attackBox.stop")}
        </Button>
      ) : (
        <Button variant="outline" size="sm" className="w-full" onClick={() => box.start()} disabled={busy}>
          {busy ? <Spinner className="size-3.5" /> : <Play className="size-3.5" />} {t("labs.attackBox.start")}
        </Button>
      )}
    </div>
  );
}
