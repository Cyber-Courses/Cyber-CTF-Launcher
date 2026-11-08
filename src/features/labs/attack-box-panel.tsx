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

/** The attack box card in the lab page's side rail. `host` is the server or cloud account a
 *  remote lab runs on: there the box is a container in the VM the lab's containers run in, shown
 *  as the runner reports it, started and stopped as here. Beside a VM lab here the box is the
 *  lab's on-demand attack VM, on every local hypervisor (QEMU included). */
export function AttackBoxPanel({
  box,
  running,
  host,
  onShell,
  shellReady,
  noBox = false,
}: {
  box: AttackBox;
  running: boolean;
  host: string | null;
  onShell?: () => void;
  shellReady?: boolean;
  /** This run has no attack box yet (a cloud-vm lab: its machines are cloud instances, with no
   *  container-host VM to add a box to): the panel says so, with no status, controls or shell. */
  noBox?: boolean;
}) {
  const t = useT();
  const remote = host !== null;
  const { status, busy, log, name, vm } = box;
  const tone = status?.running ? "ok" : busy ? "warn" : "muted";
  if (noBox) return <NoBoxPanel />;
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone={tone} pulse={busy} />
            {vm ? t("labs.attackBox.attackVm") : t("labs.attackBox.attackBox")}
          </>
        }
        action={
          <span className="inline-block max-w-[9.5rem] truncate align-bottom font-mono text-[0.6875rem] text-faint" title={name}>
            {name}
          </span>
        }
      />
      <KeyValue k={t("labs.attackBox.status")}>
        <BoxState running={running} host={host} up={!!status?.running} busy={busy} />
      </KeyValue>
      {status?.running && status.ip && (
        <KeyValue k={t("labs.attackBox.address")}>
          <CopyValue text={status.ip} />
        </KeyValue>
      )}
      <div className="space-y-3 border-t border-border p-4">
        <p className="text-[0.75rem] leading-relaxed text-muted-foreground">{t(boxHint({ remote, vm }))}</p>
        {onShell && shellReady && (
          <Button variant="primary" size="sm" className="w-full" onClick={onShell}>
            <Terminal className="size-3.5" /> {t("labs.attackBox.openShell")}
          </Button>
        )}
        {running && <BoxControls box={box} />}
        {(busy || log.some((l) => l.startsWith("✗"))) && log.length > 0 && (
          <LogConsole lines={log} running={busy} title={vm ? t("labs.attackBox.attackVm") : t("labs.attackBox.attackBox")} />
        )}
      </div>
    </Panel>
  );
}

/** A run with no attack box yet (cloud-vm): the card, with a plain note in place of the status
 *  and controls, so the player isn't offered a Start that can't work. */
function NoBoxPanel() {
  const t = useT();
  return (
    <Panel>
      <PanelHeader
        title={
          <>
            <StatusDot tone="muted" />
            {t("labs.attackBox.attackBox")}
          </>
        }
      />
      <p className="p-4 text-[0.75rem] leading-relaxed text-muted-foreground">{t("labs.attackBox.cloudVmNone")}</p>
    </Panel>
  );
}

/** What the box is: a container in the lab's VM on its server or cloud account, a VM or a
 *  container here. */
function boxHint({ remote, vm }: { remote: boolean; vm: boolean }) {
  if (remote) return "labs.attackBox.remoteHint";
  return vm ? "labs.attackBox.vmHint" : "labs.attackBox.containerHint";
}

/** Where the box stands, as the runner reports it: running (on the lab's server or cloud
 *  account, for a remote lab), starting, not started (or it starts with the lab). */
function BoxState({ running, host, up, busy }: { running: boolean; host: string | null; up: boolean; busy: boolean }) {
  const t = useT();
  if (up)
    return (
      <span className="inline-flex items-center gap-1.5">
        <Crosshair className="size-3.5 text-you-text" /> {host ? t("labs.attackBox.runningOn", { host }) : t("labs.attackBox.running")}
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

/** Start or stop the box by hand, picking which lab network it joins when the lab
 *  has more than one. */
function BoxControls({ box }: { box: AttackBox }) {
  const t = useT();
  const { status, busy, networks, network, setNetwork } = box;
  const choosing = !status?.running && networks.length > 1;
  return (
    <div className="space-y-2">
      {choosing && (
        <label className="block space-y-1">
          <span className="text-[0.6875rem] text-faint">{t("labs.attackBox.network")}</span>
          <select
            className="w-full rounded-md border border-border bg-background px-2 py-1 text-[0.75rem]"
            value={network ?? networks[0]}
            onChange={(e) => {
              setNetwork(e.target.value);
            }}
            disabled={busy}
          >
            {networks.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
      )}
      {status?.running ? (
        <Button
          variant="outline"
          size="sm"
          className="w-full"
          onClick={() => {
            void box.stop();
          }}
          disabled={busy}
        >
          <Square className="size-3.5" /> {busy ? t("labs.attackBox.working") : t("labs.attackBox.stop")}
        </Button>
      ) : (
        <Button
          variant="outline"
          size="sm"
          className="w-full"
          onClick={() => {
            void box.start();
          }}
          disabled={busy}
        >
          {busy ? <Spinner className="size-3.5" /> : <Play className="size-3.5" />} {t("labs.attackBox.start")}
        </Button>
      )}
    </div>
  );
}
