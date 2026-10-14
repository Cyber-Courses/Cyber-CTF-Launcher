"use client";

import { useState } from "react";
import { ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { LogConsole } from "@/components/ui/log-console";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { Spinner } from "@/components/ui/spinner";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { providerLabel } from "@/features/machine/hypervisors";
import { Row } from "@/features/settings/settings-layout";
import { getVmProvider, setVmProvider } from "@/lib/settings";
import { hypervJoinAdmins, installVagrantPlugin, type Provider, type ProviderStatus, type SystemReport } from "@/lib/tauri";
import { translate, useT } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

/**
 * Which local hypervisor VM labs run on. Every installed one is listed: the ready ones can be
 * picked (or Automatic, the first ready one); one that's installed but missing its Vagrant
 * add-on says so and offers to install it, then becomes pickable.
 */
export function HypervisorRow({
  report,
  onSaved,
  onNavigate,
  onRefresh,
}: {
  report: SystemReport | null;
  onSaved: () => void;
  onNavigate: (tab: "machine") => void;
  onRefresh: () => void;
}) {
  const t = useT();
  const [provider, setProvider] = useState<Provider | null>(() => getVmProvider());
  const { installing, log, installAddon, allowHyperv } = useAddonInstall(onRefresh);

  const local = report ? report.vmProviders.filter((p) => !p.remote && p.hypervisor === true) : null;
  // Ready = hypervisor + Vagrant + its plugin: VM labs can start on it right now.
  const ready: ProviderStatus[] = (local ?? []).filter((p) => p.available);
  const notReady: ProviderStatus[] = (local ?? []).filter((p) => !p.available);
  // A saved choice that's no longer ready falls back to automatic.
  const effective = ready.some((h) => h.provider === provider) ? provider : null;

  function choose(p: Provider | null) {
    setVmProvider(p);
    setProvider(p);
    onSaved();
  }

  const setupLink = (
    <button onClick={() => onNavigate("machine")} className="inline-flex items-center gap-1 text-link underline-offset-4 hover:underline">
      {t("settings.hypervisor.setupLink")} <ArrowRight className="size-3" />
    </button>
  );

  if (local === null) {
    return (
      <Row
        title={t("settings.hypervisor.title")}
        description={
          <span className="flex items-center gap-2">
            <Spinner className="size-3" /> {t("settings.hypervisor.checking")}
          </span>
        }
      />
    );
  }
  if (local.length === 0) {
    return (
      <Row
        title={t("settings.hypervisor.title")}
        description={
          <>
            {t("settings.hypervisor.noneInstalled")} {setupLink}
          </>
        }
      />
    );
  }

  const current = ready.find((h) => h.provider === (effective ?? ready[0]?.provider));
  return (
    <Row
      stacked
      title={t("settings.hypervisor.title")}
      description={current ? t("settings.hypervisor.current", { name: providerLabel(current, report?.os) }) : t("settings.hypervisor.noneReady")}
    >
      {ready.length > 0 && <ReadyList ready={ready} effective={effective} os={report?.os} onChoose={choose} />}
      {notReady.length > 0 && (
        <NotReadyList
          notReady={notReady}
          os={report?.os}
          installing={installing}
          canInstall={!!report?.vagrant.installed}
          onInstall={installAddon}
          onAllowHyperv={allowHyperv}
        />
      )}
      {log && <LogConsole className="mt-3" lines={log} running={installing !== null} title={t("settings.hypervisor.logTitle")} />}
    </Row>
  );
}

/** The ready hypervisors to pick from, with Automatic first when there are several. */
function ReadyList({
  ready,
  effective,
  os,
  onChoose,
}: {
  ready: ProviderStatus[];
  effective: Provider | null;
  os: string | undefined;
  onChoose: (p: Provider | null) => void;
}) {
  const t = useT();
  return (
    <RadioList label={t("settings.hypervisor.title")} className="mt-3">
      {ready.length > 1 && (
        <RadioRow
          compact
          selected={effective === null}
          onSelect={() => onChoose(null)}
          title={t("settings.hypervisor.automatic")}
          subtitle={t("settings.hypervisor.automaticNote", { name: providerLabel(ready[0], os) })}
        />
      )}
      {ready.map((h) => (
        <RadioRow
          key={h.provider}
          compact
          selected={effective === h.provider || (ready.length === 1 && effective === null)}
          onSelect={() => onChoose(h.provider)}
          title={
            <span className="flex items-center gap-2">
              <HypervisorLogo provider={h.provider} size="sm" />
              {providerLabel(h, os)}
            </span>
          }
          subtitle={h.plugin ? t("settings.hypervisor.readyPlugin", { plugin: h.plugin }) : t("settings.hypervisor.readyBuiltIn")}
        />
      ))}
    </RadioList>
  );
}

/** The installed hypervisors that can't run VMs yet, each with why, and the add-on install. */
function NotReadyList({
  notReady,
  os,
  installing,
  canInstall,
  onInstall,
  onAllowHyperv,
}: {
  notReady: ProviderStatus[];
  os: string | undefined;
  installing: string | null;
  canInstall: boolean;
  onInstall: (plugin: string) => void;
  onAllowHyperv: () => void;
}) {
  const t = useT();
  return (
    <div className="mt-3 overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      {notReady.map((h) => (
        <div key={h.provider} className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-3.5 py-2 first:border-t-0">
          <HypervisorLogo provider={h.provider} size="sm" />
          <div className="min-w-0 flex-1">
            <p className="text-[0.8125rem] font-medium text-foreground">{providerLabel(h, os)}</p>
            <p className="truncate text-[0.75rem] text-muted-foreground">
              {h.plugin && !h.pluginInstalled ? t("settings.hypervisor.needsPlugin", { plugin: h.plugin }) : (h.reason ?? t("settings.hypervisor.notReady"))}
            </p>
          </div>
          {h.plugin && !h.pluginInstalled && (
            <Button variant="outline" size="xs" disabled={installing !== null || !canInstall} onClick={() => onInstall(h.plugin!)}>
              {installing === h.plugin ? (
                <>
                  <Spinner className="size-3.5" /> {t("settings.hypervisor.installing")}
                </>
              ) : (
                t("settings.hypervisor.installAddon")
              )}
            </Button>
          )}
          {canInstall && needsHypervRights(h) && (
            <Button variant="outline" size="xs" disabled={installing !== null} onClick={onAllowHyperv}>
              {installing === HYPERV_RIGHTS ? (
                <>
                  <Spinner className="size-3.5" /> {t("settings.hypervisor.allowingHyperv")}
                </>
              ) : (
                t("settings.hypervisor.allowHyperv")
              )}
            </Button>
          )}
        </div>
      ))}
    </div>
  );
}

/** What `installing` holds while this account is being allowed to manage Hyper-V. */
const HYPERV_RIGHTS = "hyperv-rights";

/** Hyper-V is on and Vagrant installed, yet not ready: the one thing left is this account's
 *  right to manage Hyper-V (Vagrant needs an administrator or a Hyper-V Administrators member). */
const needsHypervRights = (h: ProviderStatus) => h.provider === "hyperv" && h.hypervisor === true && !h.available;

/** Installs a hypervisor's Vagrant add-on (or allows this account to manage Hyper-V), with its
 *  log, then re-checks the machine. */
function useAddonInstall(onRefresh: () => void) {
  const [installing, setInstalling] = useState<string | null>(null);
  const [log, setLog] = useState<string[] | null>(null);

  async function installAddon(plugin: string) {
    setInstalling(plugin);
    setLog([translate("settings.hypervisor.logInstalling", { plugin })]);
    try {
      await installVagrantPlugin(plugin, (l) => setLog((x) => [...(x ?? []), l]));
      setLog((x) => [...(x ?? []), translate("settings.hypervisor.logInstalled")]);
    } catch (e) {
      setLog((x) => [...(x ?? []), `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  async function allowHyperv() {
    setInstalling(HYPERV_RIGHTS);
    setLog([translate("settings.hypervisor.logAllowingHyperv")]);
    try {
      await hypervJoinAdmins();
      setLog((x) => [...(x ?? []), translate("settings.hypervisor.logHypervAllowed")]);
    } catch (e) {
      setLog((x) => [...(x ?? []), `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  return { installing, log, installAddon, allowHyperv };
}
