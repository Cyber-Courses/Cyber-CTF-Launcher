"use client";

/**
 * What an install asks of the player (Rust's `install::Interaction`: a password prompt, the
 * agreement a vendor's app will show them, a setting to allow, a restart, a long build), said
 * before it starts, in the dialog that informs them, and again while the step that raises it runs,
 * as a live hint above its log. The core says which; the words are the webview's. Nothing accepts
 * an agreement for the player: the vendor's own window shows it, and they accept it there.
 */

import { ExternalLink, Info } from "lucide-react";
import { ConsentDialog } from "@/components/ui/consent-dialog";
import { openExternal } from "@/lib/failure";
import { translate, useT, type MessageKey, type T } from "@/lib/i18n";
import type { Dependency, InstallProgress, Interaction, SetupAction, SystemReport } from "@/lib/tauri";

/** An agreement a vendor's app shows the player, who accepts it there. */
export type VendorAgreement = Extract<Interaction, { kind: "vendorAgreement" }>;

/** The vendors' agreements among `asks`. */
export const agreementsOf = (asks: readonly Interaction[]): VendorAgreement[] => asks.filter((i): i is VendorAgreement => i.kind === "vendorAgreement");

/** The words of each interaction, by its name ([`name`]), said before the install. */
const BEFORE = {
  passwordMac: "machine.asks.before.passwordMac",
  passwordUac: "machine.asks.before.passwordUac",
  passwordPkexec: "machine.asks.before.passwordPkexec",
  vendorAgreement: "machine.asks.before.vendorAgreement",
  vendorWindow: "machine.asks.before.vendorWindow",
  systemExtension: "machine.asks.before.systemExtension",
  restart: "machine.asks.before.restart",
  relogin: "machine.asks.before.relogin",
  longBuild: "machine.asks.before.longBuild",
  downloadByHand: "machine.asks.before.downloadByHand",
} as const satisfies Record<string, MessageKey>;

/** The same, said now, as the step that raises it runs. */
const NOW = {
  passwordMac: "machine.asks.now.passwordMac",
  passwordUac: "machine.asks.now.passwordUac",
  passwordPkexec: "machine.asks.now.passwordPkexec",
  vendorAgreement: "machine.asks.now.vendorAgreement",
  vendorWindow: "machine.asks.now.vendorWindow",
  systemExtension: "machine.asks.now.systemExtension",
  restart: "machine.asks.now.restart",
  relogin: "machine.asks.now.relogin",
  longBuild: "machine.asks.now.longBuild",
  downloadByHand: "machine.asks.now.downloadByHand",
} as const satisfies Record<string, MessageKey>;

/** An interaction's name among [`BEFORE`]'s. */
function name(i: Interaction): keyof typeof BEFORE {
  switch (i.kind) {
    case "adminPassword":
      return i.via === "mac" ? "passwordMac" : i.via === "uac" ? "passwordUac" : "passwordPkexec";
    case "allowSystemExtension":
      return "systemExtension";
    default:
      return i.kind;
  }
}

/** The values an interaction's words name. */
function vars(i: Interaction): Record<string, string | number> {
  switch (i.kind) {
    case "vendorAgreement":
      return { app: i.app, agreement: i.name };
    case "vendorWindow":
      return { app: i.app };
    case "allowSystemExtension":
      return { vendor: i.vendor };
    case "longBuild":
      return { minutes: i.minutes };
    case "downloadByHand":
      return { vendor: i.vendor };
    default:
      return {};
  }
}

/** What `i` will ask, said before the install starts. */
const askedBefore = (t: T, i: Interaction) => t(BEFORE[name(i)], vars(i));

/** What `i` asks now, said as the step that raises it starts. */
const askedNow = (t: T, i: Interaction) => t(NOW[name(i)], vars(i));

/** What installing `dependency` asks, as the machine check's setup options say it (each install
 *  action carries its own); none when no option installs it. */
export function asksOf(report: SystemReport | null, dependency: Dependency): Interaction[] {
  const setup = report?.setup;
  if (!setup) return [];
  const actions: SetupAction[] = [...setup.wsl, ...setup.engines.flatMap((e) => e.actions), ...setup.hypervisors.flatMap((h) => [...h.get, ...h.vagrant])];
  const install = actions.find((a): a is Extract<SetupAction, { kind: "install" }> => a.kind === "install" && a.dependency === dependency);
  return install?.interactions ?? [];
}

/** An install's progress into a log alone (a flow without a live hint): what a step asks said as
 *  a line of it. */
export const progressAsLog =
  (onLog: (line: string) => void) =>
  (p: InstallProgress): void => {
    if (p.kind === "line") onLog(p.line);
    else for (const i of p.interactions) onLog(askedNow(translate as T, i));
  };

/** An install the player is asked about before it starts. */
export type PendingInstall = { name: string; asks: Interaction[] };

/**
 * Tells the player, before an install that asks something of them, what it will ask (a password
 * prompt, the agreement a vendor's app will show them, a setting to allow, a restart, a long
 * build), each in a line, with a link to read each agreement beforehand. It informs: nothing is
 * accepted here, and the player accepts a vendor's agreement in the vendor's own window.
 */
export function InstallConsent({ pending, onInstall, onCancel }: { pending: PendingInstall; onInstall: () => void; onCancel: () => void }) {
  const t = useT();
  return (
    <ConsentDialog
      title={t("machine.asks.title", { name: pending.name })}
      confirmLabel={t("machine.asks.install")}
      cancelLabel={t("machine.asks.cancel")}
      onConfirm={onInstall}
      onCancel={onCancel}
    >
      <p>{t("machine.asks.lead")}</p>
      <ul className="mt-2 list-disc space-y-1 pl-5" data-testid="install-asks">
        {pending.asks.map((i) => (
          <li key={JSON.stringify(i)}>{askedBefore(t, i)}</li>
        ))}
      </ul>
      {agreementsOf(pending.asks).map((a) => (
        <button
          key={a.url}
          type="button"
          className="mt-3 inline-flex items-center gap-1 text-foreground underline underline-offset-2"
          onClick={() => {
            openExternal(a.url);
          }}
        >
          {t("machine.asks.readAgreement", { agreement: a.name })} <ExternalLink className="size-3" aria-hidden="true" />
        </button>
      ))}
    </ConsentDialog>
  );
}

/** The dialog of the install waiting for the player in a setup flow (`installs.pending`), if any:
 *  one per flow, whichever of its views asked it. */
export function PendingInstallConsent({ installs }: { installs: { pending: (PendingInstall & { start: () => void }) | null; cancelPending: () => void } }) {
  const { pending, cancelPending } = installs;
  if (!pending) return null;
  return <InstallConsent key={pending.name} pending={pending} onInstall={pending.start} onCancel={cancelPending} />;
}

/** What the step running now asks of the player, above its log: "macOS is asking for your
 *  password in a window that just opened". Nothing when it asks nothing. */
export function InstallHint({ asks }: { asks: readonly Interaction[] | null }) {
  const t = useT();
  if (!asks?.length) return null;
  return <LiveHint said={asks.map((i) => askedNow(t, i))} />;
}

/** The live hint's box: what is asked of the player right now, a line each. */
export function LiveHint({ said }: { said: readonly string[] }) {
  const t = useT();
  return (
    <div
      role="status"
      aria-live="polite"
      className="mb-2 flex gap-2 rounded-control bg-primary/10 px-3 py-2 text-[0.8125rem] text-foreground"
      data-testid="install-hint"
    >
      <Info className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden="true" />
      <div>
        <p className="font-medium">{t("machine.asks.nowTitle")}</p>
        <ul className="mt-0.5 space-y-0.5">
          {said.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      </div>
    </div>
  );
}
