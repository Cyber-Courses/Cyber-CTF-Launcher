"use client";

import { useState } from "react";
import { ArrowRight } from "lucide-react";
import { LogConsole } from "@/components/ui/log-console";
import { Spinner } from "@/components/ui/spinner";
import { providerLabel } from "@/features/machine/hypervisors";
import { Row } from "@/features/settings/settings-layout";
import { getVmProvider, setVmProvider } from "@/lib/settings";
import type { Provider, ProviderStatus, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { useAddonInstall } from "@/features/settings/hypervisor-addons";
import { NotReadyList, ReadyList } from "@/features/settings/hypervisor-lists";

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
  if (!report) {
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
  const local = report.vmProviders.filter((p) => !p.remote && p.hypervisor === true);
  if (local.length === 0) {
    return (
      <Row
        title={t("settings.hypervisor.title")}
        description={
          <>
            {t("settings.hypervisor.noneInstalled")}{" "}
            <button
              onClick={() => {
                onNavigate("machine");
              }}
              className="inline-flex items-center gap-1 text-link underline-offset-4 hover:underline"
            >
              {t("settings.hypervisor.setupLink")} <ArrowRight className="size-3" />
            </button>
          </>
        }
      />
    );
  }
  return <HypervisorChoice report={report} local={local} onSaved={onSaved} onRefresh={onRefresh} />;
}

/** The installed hypervisors: the ready ones to pick from, the others with what they need. */
function HypervisorChoice({
  report,
  local,
  onSaved,
  onRefresh,
}: {
  report: SystemReport;
  local: ProviderStatus[];
  onSaved: () => void;
  onRefresh: () => void;
}) {
  const t = useT();
  const [provider, setProvider] = useState<Provider | null>(() => getVmProvider());
  const { installing, log, installAddon, allowHyperv } = useAddonInstall(onRefresh);
  // Ready = hypervisor + Vagrant + its plugin: VM labs can start on it right now.
  const ready = local.filter((p) => p.available);
  const notReady = local.filter((p) => !p.available);
  // A saved choice that's no longer ready falls back to automatic.
  const effective = ready.some((h) => h.provider === provider) ? provider : null;
  const current = ready.find((h) => h.provider === (effective ?? ready[0]?.provider));

  function choose(p: Provider | null) {
    setVmProvider(p);
    setProvider(p);
    onSaved();
  }

  return (
    <Row
      stacked
      title={t("settings.hypervisor.title")}
      description={current ? t("settings.hypervisor.current", { name: providerLabel(current, report.os) }) : t("settings.hypervisor.noneReady")}
    >
      {ready.length > 0 && <ReadyList ready={ready} effective={effective} os={report.os} onChoose={choose} />}
      {notReady.length > 0 && (
        <NotReadyList
          notReady={notReady}
          os={report.os}
          installing={installing}
          canInstall={report.vagrant.installed}
          onInstall={installAddon}
          onAllowHyperv={allowHyperv}
        />
      )}
      {log && <LogConsole className="mt-3" lines={log} running={installing !== null} title={t("settings.hypervisor.logTitle")} />}
    </Row>
  );
}
