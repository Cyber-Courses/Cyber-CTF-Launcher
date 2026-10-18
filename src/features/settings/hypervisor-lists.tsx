"use client";

import { Button } from "@/components/ui/button";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { Spinner } from "@/components/ui/spinner";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { providerLabel } from "@/features/machine/hypervisors";
import { HYPERV_RIGHTS, needsHypervRights } from "@/features/settings/hypervisor-addons";
import type { Provider, ProviderStatus } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

// The hypervisor setting's lists: the ready hypervisors to pick from, and the installed ones that
// can't run VM labs yet, with what each needs.

/** The ready hypervisors to pick from, with Automatic first when there are several. */
export function ReadyList({
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
          onSelect={() => {
            onChoose(null);
          }}
          title={t("settings.hypervisor.automatic")}
          subtitle={t("settings.hypervisor.automaticNote", { name: providerLabel(ready[0], os) })}
        />
      )}
      {ready.map((h) => (
        <RadioRow
          key={h.provider}
          compact
          selected={effective === h.provider || (ready.length === 1 && effective === null)}
          onSelect={() => {
            onChoose(h.provider);
          }}
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
export function NotReadyList({ notReady, os, ...actions }: { notReady: ProviderStatus[]; os: string | undefined } & AddonActions) {
  return (
    <div className="mt-3 overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
      {notReady.map((h) => (
        <NotReadyRow key={h.provider} hypervisor={h} os={os} {...actions} />
      ))}
    </div>
  );
}

/** What the not-ready rows can do: install an add-on, allow Hyper-V (one at a time). */
type AddonActions = {
  installing: string | null;
  canInstall: boolean;
  onInstall: (plugin: string) => void;
  onAllowHyperv: () => void;
};

/** One hypervisor that can't run VMs yet: why, and what fixes it. */
function NotReadyRow({
  hypervisor: h,
  os,
  installing,
  canInstall,
  onInstall,
  onAllowHyperv,
}: { hypervisor: ProviderStatus; os: string | undefined } & AddonActions) {
  const t = useT();
  const missing = h.plugin && !h.pluginInstalled ? h.plugin : null;
  return (
    <div className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-3.5 py-2 first:border-t-0">
      <HypervisorLogo provider={h.provider} size="sm" />
      <div className="min-w-0 flex-1">
        <p className="text-[0.8125rem] font-medium text-foreground">{providerLabel(h, os)}</p>
        <p className="truncate text-[0.75rem] text-muted-foreground">
          {missing ? t("settings.hypervisor.needsPlugin", { plugin: missing }) : (h.reason ?? t("settings.hypervisor.notReady"))}
        </p>
      </div>
      {missing && (
        <Button
          variant="outline"
          size="xs"
          disabled={installing !== null || !canInstall}
          onClick={() => {
            onInstall(missing);
          }}
        >
          {installing === missing ? (
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
  );
}
