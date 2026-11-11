"use client";

import { AppWindow, Crosshair, Monitor, Play, Square, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { LogConsole } from "@/components/ui/log-console";
import { CopyValue } from "@/components/ui/copy-value";
import { KeyValue, Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import { PasswordValue } from "@/features/labs/password-value";
import type { AttackBox } from "@/features/labs/use-attack-box";
import { useT } from "@/lib/i18n";
import type { SignIn } from "@/lib/tauri";
import { isFailedLine } from "@/lib/log-marks";

/** The attack box card in the lab page's side rail. The box is one more machine of the lab,
 *  wherever it runs (`host`: the server or cloud account of a remote lab): a Kali VM among a VM
 *  lab's, a container among a container lab's; shown as the runner reports it. */
export function AttackBoxPanel({
  box,
  running,
  host,
  onShell,
  shellReady,
  onScreen,
  nativeApp,
  onNative,
}: {
  box: AttackBox;
  running: boolean;
  host: string | null;
  onShell?: () => void;
  shellReady?: boolean;
  /** Opens the box's screen (its graphical console), when it has one that opens now. */
  onScreen?: () => void;
  /** The box's hypervisor's own app this computer has ("VirtualBox"), once the box runs. */
  nativeApp?: string | null;
  /** Opens the box in that app. */
  onNative?: () => void;
}) {
  const t = useT();
  const { status, busy, log, name, vm, signIn } = box;
  const tone = status?.running ? "ok" : busy ? "warn" : "muted";
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
      {status?.running && signIn && <Credentials signIn={signIn} />}
      <div className="space-y-3 border-t border-border p-4">
        <p className="text-[0.75rem] leading-relaxed text-muted-foreground">{t(vm ? "labs.attackBox.vmHint" : "labs.attackBox.containerHint")}</p>
        {status?.running && signIn && (
          <p className="text-[0.75rem] leading-relaxed text-muted-foreground">{t("labs.attackBox.credentialsHint", { user: signIn.user })}</p>
        )}
        <WaysIn onShell={shellReady ? onShell : undefined} onScreen={onScreen} nativeApp={nativeApp ?? null} onNative={onNative} />
        {running && <BoxControls box={box} />}
        {(busy || log.some(isFailedLine)) && log.length > 0 && (
          <LogConsole lines={log} running={busy} title={vm ? t("labs.attackBox.attackVm") : t("labs.attackBox.attackBox")} />
        )}
      </div>
    </Panel>
  );
}

/** How the player signs in where the box asks for a password: its account, and its password
 *  masked until Show. */
function Credentials({ signIn }: { signIn: SignIn }) {
  const t = useT();
  return (
    <KeyValue k={t("labs.attackBox.credentials")}>
      <span className="inline-flex flex-wrap items-center justify-end gap-1.5">
        <CopyValue text={signIn.user} />
        <PasswordValue password={signIn.password} />
      </span>
    </KeyValue>
  );
}

/** The box's ways in: its shell, its screen when it has one, and its hypervisor's own app. */
function WaysIn({ onShell, onScreen, nativeApp, onNative }: { onShell?: () => void; onScreen?: () => void; nativeApp: string | null; onNative?: () => void }) {
  const t = useT();
  return (
    <>
      {onShell && (
        <Button variant="primary" size="sm" className="w-full" onClick={onShell}>
          <Terminal className="size-3.5" /> {t("labs.attackBox.openShell")}
        </Button>
      )}
      {onScreen && (
        <Button variant="outline" size="sm" className="w-full" onClick={onScreen}>
          <Monitor className="size-3.5" /> {t("labs.attackBox.openScreen")}
        </Button>
      )}
      {nativeApp && onNative && (
        <Button variant="ghost" size="sm" className="w-full" onClick={onNative}>
          <AppWindow className="size-3.5" /> {t("labs.attackBox.openNative", { app: nativeApp })}
        </Button>
      )}
    </>
  );
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

/** Start or stop the box by hand, picking which lab network it joins when the backend offers
 *  more than one (none where the lab declares the player's seat), its default until picked. */
function BoxControls({ box }: { box: AttackBox }) {
  const t = useT();
  const { status, busy, networks, network, defaultNetwork, setNetwork } = box;
  const choosing = !status?.running && networks.length > 1;
  return (
    <div className="space-y-2">
      {choosing && (
        <label className="block space-y-1">
          <span className="text-[0.6875rem] text-faint">{t("labs.attackBox.network")}</span>
          <select
            className="w-full rounded-md border border-border bg-background px-2 py-1 text-[0.75rem]"
            value={network ?? defaultNetwork}
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
