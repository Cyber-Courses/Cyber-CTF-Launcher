"use client";

import { ExternalLink, HardDrive } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { LogConsole } from "@/components/ui/log-console";
import { Requirement } from "@/features/machine/setup-steps";
import { installDependency, installVagrantPlugin } from "@/lib/tauri";
import { CLOUD_META, KIND } from "@/features/servers/host-setup/constants";
import { Step } from "@/features/servers/host-setup/form";
import { BackNav, ContinueButton } from "@/features/servers/host-setup/step-nav";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

const OVFTOOL_URL = "https://developer.broadcom.com/tools/open-virtualization-format-ovf-tool/latest";

export function ToolsStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { report, onRefresh, v, pluginLog, toolLabel, cloudProvider, cloud, next, back, toolBusy, cloudHasCli, toolsOk } = s;
  return (
    <Step
      icon={HardDrive}
      title={cloud ? t("servers.setup.tools.titleCloud") : t("servers.setup.tools.titleServer")}
      description={
        cloud
          ? cloudHasCli
            ? t("servers.setup.tools.descriptionCli", { cloud: CLOUD_META[cloudProvider].label })
            : t("servers.setup.tools.descriptionToken", { cloud: CLOUD_META[cloudProvider].label })
          : t("servers.setup.tools.descriptionServer", { kind: KIND[v.provider].label })
      }
    >
      <div className="surface-panel overflow-hidden rounded-control">
        {!report ? (
          <div className="flex items-center gap-2 px-4 py-3 text-[0.8125rem] text-muted-foreground">
            <Spinner className="size-4" /> {t("servers.setup.tools.checking")}
          </div>
        ) : cloud ? (
          <CloudTools s={s} />
        ) : v.provider === "vmware_esxi" ? (
          <EsxiTools s={s} />
        ) : (
          <TerraformRequirement s={s} detail={report.terraform.installed ? null : t("servers.setup.tools.terraformProxmox")} />
        )}
      </div>
      {pluginLog && (
        <div className="mt-3">
          <LogConsole lines={pluginLog} running={toolBusy} title={t("servers.setup.tools.install", { name: toolLabel })} />
        </div>
      )}
      <BackNav onBack={back}>
        <span className="flex gap-2">
          {!toolsOk && (
            <Button
              variant="outline"
              onClick={() => {
                onRefresh();
              }}
            >
              {t("servers.setup.nav.recheck")}
            </Button>
          )}
          <ContinueButton onClick={next} disabled={!toolsOk} />
        </span>
      </BackNav>
    </Step>
  );
}

/** A cloud account's tools: its CLI (for the clouds that sign in with one), then Terraform. */
function CloudTools({ s }: { s: HostSetup }) {
  const t = useT();
  const { cloudProvider, toolBusy, installTool, cloudDep, cloudHasCli, cloudCliTool, cloudCliOk, terraformOk } = s;
  const cli = CLOUD_META[cloudProvider].cli;
  return (
    <>
      {cloudHasCli && (
        <Requirement
          ok={cloudCliOk}
          title={t("servers.setup.tools.cliTitle", { cloud: CLOUD_META[cloudProvider].label })}
          detail={cloudCliOk ? (cloudCliTool?.version ?? t("servers.setup.tools.installed")) : t("servers.setup.tools.cliDetail", { cli })}
          action={
            <Button
              variant="outline"
              size="xs"
              disabled={toolBusy}
              onClick={() => {
                installTool(`${cli} CLI`, (log) => installDependency(cloudDep, log));
              }}
            >
              {t("servers.setup.tools.install", { name: cli })}
            </Button>
          }
        />
      )}
      <TerraformRequirement s={s} detail={terraformOk ? null : t("servers.setup.tools.terraformCloud")} />
    </>
  );
}

/** An ESXi host's tools: Vagrant, its ESXi plugin, and VMware's OVF Tool (a manual download). */
function EsxiTools({ s }: { s: HostSetup }) {
  const t = useT();
  const { report, kind, toolBusy, installTool, vagrantOk, esxiPluginOk, ovftoolOk } = s;
  return (
    <>
      <Requirement
        ok={vagrantOk}
        title="Vagrant"
        detail={vagrantOk ? (report?.vagrant.version ?? t("servers.setup.tools.installed")) : t("servers.setup.tools.vagrant")}
        action={
          <Button
            variant="outline"
            size="xs"
            disabled={toolBusy}
            onClick={() => {
              installTool("Vagrant", (log) => installDependency("vagrant", log));
            }}
          >
            {t("servers.setup.tools.install", { name: "Vagrant" })}
          </Button>
        }
      />
      <Requirement
        ok={esxiPluginOk}
        title={t("servers.setup.tools.esxiPlugin")}
        detail={vagrantOk || esxiPluginOk ? kind.plugin : t("servers.setup.tools.esxiPluginPending", { plugin: kind.plugin })}
        action={
          <Button
            variant="outline"
            size="xs"
            disabled={toolBusy || !vagrantOk}
            onClick={() => {
              installTool(kind.plugin, (log) => installVagrantPlugin(kind.plugin, log));
            }}
          >
            {t("servers.setup.tools.installPlugin")}
          </Button>
        }
      />
      <Requirement
        ok={ovftoolOk}
        title="VMware OVF Tool"
        detail={ovftoolOk ? (report?.ovftool.version ?? t("servers.setup.tools.installed")) : t("servers.setup.tools.ovftool")}
        action={
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              openExternal(OVFTOOL_URL);
            }}
          >
            <ExternalLink className="size-3" /> {t("servers.setup.tools.get")}
          </Button>
        }
      />
    </>
  );
}

/** Terraform, with its version once installed; `detail` says why it's needed while it isn't. */
function TerraformRequirement({ s, detail }: { s: HostSetup; detail: string | null }) {
  const t = useT();
  const { report, terraformOk, toolBusy, installTool } = s;
  return (
    <Requirement
      ok={terraformOk}
      title="Terraform"
      detail={detail === null ? (report?.terraform.version ?? t("servers.setup.tools.installed")) : detail}
      action={
        <Button
          variant="outline"
          size="xs"
          disabled={toolBusy}
          onClick={() => {
            installTool("Terraform", (log) => installDependency("terraform", log));
          }}
        >
          {t("servers.setup.tools.install", { name: "Terraform" })}
        </Button>
      }
    />
  );
}
