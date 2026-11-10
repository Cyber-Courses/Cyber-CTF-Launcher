"use client";

import { useCallback, useEffect, useState } from "react";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { LogConsole } from "@/components/ui/log-console";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { installedTools, uninstallDependency, type Dependency, type InstalledTool } from "@/lib/tauri";
import { useRunLog } from "@/lib/use-run-log";
import { formatAgo } from "@/lib/format";
import { warn } from "@/lib/failure";
import { confirmDestructive } from "@/lib/confirm";
import { translate, useT } from "@/lib/i18n";
import { useToolInstallsOff } from "@/lib/use-managed";
import { InstallsOffNote } from "@/components/installs-off-note";

const NAMES: Record<Dependency, string> = {
  docker: "Docker",
  vagrant: "Vagrant",
  terraform: "Terraform",
  virtualbox: "VirtualBox",
  qemu: "QEMU",
  utm: "UTM",
  libvirt: "libvirt",
  awscli: "AWS CLI",
  azurecli: "Azure CLI",
  gcloud: "Google Cloud CLI",
  wsl: "WSL",
  "vmware-utility": "Vagrant VMware Utility",
};

/** What setup installed under that name: Docker Engine on Linux (Docker's apt repository), Docker Desktop elsewhere. */
function toolName(dep: Dependency): string {
  if (dep !== "docker") return NAMES[dep];
  return typeof navigator !== "undefined" && navigator.userAgent.includes("Linux") ? "Docker Engine" : "Docker Desktop";
}

/** The tools Cyber CTF installed, and their removal with its log (the list is read again after). */
function useInstalledTools() {
  const [tools, setTools] = useState<InstalledTool[]>([]);
  const [removing, setRemoving] = useState<Dependency | null>(null);
  const { lines, run } = useRunLog();

  const load = useCallback(() => {
    installedTools().then(setTools).catch(warn("reading the tools Cyber CTF installed"));
  }, []);
  useEffect(load, [load]);

  const uninstall = async (dep: Dependency) => {
    setRemoving(dep);
    await run(translate("settings.installedTools.logRemoving", { name: toolName(dep) }), (onLog) => uninstallDependency(dep, onLog));
    setRemoving(null);
    load();
  };
  const remove = (dep: Dependency) => {
    void uninstall(dep);
  };
  return { tools, removing, log: lines ?? [], remove };
}

/** One installed tool: when setup installed it, and Remove. */
function ToolRow({
  tool,
  now,
  removing,
  canRemove,
  onRemove,
}: {
  tool: InstalledTool;
  now: number;
  removing: Dependency | null;
  canRemove: boolean;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <li className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2 transition-colors first:border-t-0 hover:bg-glass">
      <div className="min-w-0 flex-1">
        <p className="text-[0.8125rem] font-medium text-foreground">{toolName(tool.dependency)}</p>
        <p className="font-mono text-[0.6875rem] text-faint">{t("settings.installedTools.installedAgo", { ago: formatAgo(tool.at * 1000, now) })}</p>
      </div>
      <Button variant="outline" size="xs" disabled={removing !== null || !canRemove} onClick={onRemove}>
        <Trash2 className="size-3" /> {removing === tool.dependency ? t("settings.installedTools.removing") : t("settings.installedTools.remove")}
      </Button>
    </li>
  );
}

/** The tools Cyber CTF installed during setup, each removable. Hidden when there are none; a tool
 *  the player had before is never listed, so nothing of theirs can be removed from here. */
export function InstalledToolsPanel() {
  const t = useT();
  const { tools, removing, log, remove } = useInstalledTools();
  // Removals go with installs under the policy (`DisableToolInstalls`).
  const installsOff = useToolInstallsOff();

  // "Installed … ago" counts from when the panel opened.
  const [now] = useState(Date.now);

  if (tools.length === 0 && log.length === 0) return null;
  return (
    <Panel>
      <PanelHeader title={t("settings.installedTools.title")} meta={tools.length ? t("settings.installedTools.count", { count: tools.length }) : undefined} />
      <p className="border-b border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">{t("settings.installedTools.intro")}</p>
      {installsOff && <InstallsOffNote className="border-b border-border px-4 py-2.5" />}
      <ul>
        {tools.map((tool) => (
          <ToolRow
            key={tool.dependency}
            tool={tool}
            now={now}
            removing={removing}
            canRemove={!installsOff}
            onRemove={() => {
              const name = toolName(tool.dependency);
              void confirmDestructive({
                title: t("settings.installedTools.confirmTitle", { name }),
                message: t("settings.installedTools.confirmBody", { name }),
                confirm: t("settings.installedTools.remove"),
              }).then((yes) => {
                if (yes) remove(tool.dependency);
              });
            }}
          />
        ))}
      </ul>
      {log.length > 0 && (
        <div className="border-t border-border p-4">
          <LogConsole lines={log} running={removing !== null} title={t("settings.installedTools.remove")} />
        </div>
      )}
    </Panel>
  );
}
