"use client";

import { cn } from "@/lib/utils";
import { HypervisorStep } from "@/features/servers/host-setup/hypervisor-step";
import { ToolsStep } from "@/features/servers/host-setup/tools-step";
import { ConnectionStep } from "@/features/servers/host-setup/connection-step";
import { PlacementStep } from "@/features/servers/host-setup/placement-step";
import { AccountStep } from "@/features/servers/host-setup/account-step";
import { CredentialsStep } from "@/features/servers/host-setup/credentials-step";
import { OptionsStep } from "@/features/servers/host-setup/options-step";
import { ProviderStep } from "@/features/servers/host-setup/provider-step";
import { TestStep } from "@/features/servers/host-setup/test-step";
import { useHostSetup } from "@/features/servers/host-setup/use-host-setup";
import type { ServerHost, ServerHostInput, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

export function HostSetupPage({
  initial,
  report,
  onRefresh,
  onSaved,
  onDone,
}: {
  initial: ServerHostInput;
  report: SystemReport | null;
  onRefresh: () => void;
  /** After every save, so the main window's host list can refresh. */
  onSaved: (h: ServerHost) => void;
  /** Setup finished or cancelled: the window closes. */
  onDone: () => void;
}) {
  const t = useT();
  const s = useHostSetup({ initial, report, onRefresh, onSaved, onDone });
  const { i, cloud, steps, key, title } = s;
  return (
    <>
      <div>
        <p className="font-mono text-[0.6875rem] tabular-nums text-faint">{t("servers.setup.stepOf", { n: i + 1, total: steps.length })}</p>
        <h1 className="serif-title mt-1.5 text-[1.75rem] text-foreground">{title}</h1>
        <p className="mt-1.5 text-[0.8125rem] text-muted-foreground">{cloud ? t("servers.setup.leadCloud") : t("servers.setup.leadServer")}</p>
      </div>

      {/* Thin step indicator, one segment per step. */}
      <div className="mt-5 flex gap-1" aria-hidden>
        {steps.map((s, n) => (
          <div key={s} className={cn("h-0.5 flex-1 rounded-full transition-colors", n <= i ? "bg-jewel-solid" : "bg-border")} />
        ))}
      </div>

      <div key={key} className="mt-6 flex flex-1 animate-rise-in flex-col">
        {key === "hypervisor" && <HypervisorStep s={s} />}

        {key === "tools" && <ToolsStep s={s} />}

        {key === "connection" && <ConnectionStep s={s} />}

        {key === "placement" && <PlacementStep s={s} />}

        {key === "account" && <AccountStep s={s} />}

        {key === "credentials" && <CredentialsStep s={s} />}

        {key === "options" && <OptionsStep s={s} />}

        {key === "provider" && <ProviderStep s={s} />}

        {key === "test" && <TestStep s={s} />}
      </div>
    </>
  );
}

// Used by the server-setup window and the Servers page.
export { EMPTY_CLOUD, EMPTY_HOST, KIND } from "@/features/servers/host-setup/constants";
export { SetupTrademarks } from "@/features/servers/host-setup/form";
