"use client";

import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { ExternalLink, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Requirement } from "@/features/machine/setup-steps/parts";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { ovftoolPlace, type OvftoolLocation, type OvftoolPlaced, type SystemReport } from "@/lib/tauri";
import { openExternal } from "@/lib/failure";
import { describeError } from "@/lib/errors";
import { useT, type T } from "@/lib/i18n";

/** Broadcom's free downloads of each desktop hypervisor, which ships the OVF Tool. */
const FUSION_URL = "https://support.broadcom.com/group/ecx/productdownloads?subfamily=VMware%20Fusion&freeDownloads=true";
const WORKSTATION_URL = "https://support.broadcom.com/group/ecx/productdownloads?subfamily=VMware%20Workstation%20Pro&freeDownloads=true";
/** The standalone OVF Tool's page at Broadcom. */
const OVFTOOL_URL = "https://developer.broadcom.com/tools/open-virtualization-format-ovf-tool/latest";

/** The desktop hypervisor that ships the OVF Tool on this machine, when one runs here: Fusion on
 *  a Mac, Workstation on an x64 PC (Windows or Linux; there is none for Arm). */
function ovftoolHypervisor(report: Pick<SystemReport, "os" | "arch"> | null): { name: string; url: string } | null {
  if (!report) return null;
  if (report.os === "macos") return { name: "VMware Fusion", url: FUSION_URL };
  const x64 = report.arch === "x86_64";
  if (x64 && (report.os === "windows" || report.os === "linux")) return { name: "VMware Workstation", url: WORKSTATION_URL };
  return null;
}

/** The row's detail: the version found and where; else why the one found doesn't run; else
 *  what the tool is for. */
function detail(t: T, ok: boolean, version: string | null | undefined, at: OvftoolLocation | null | undefined): string {
  if (ok && at)
    return t("servers.setup.tools.ovftoolFound", {
      version: version ?? t("servers.setup.tools.installed"),
      where: t(`servers.setup.tools.ovftoolWhere.${at.source}`),
      path: at.path,
    });
  if (ok) return version ?? t("servers.setup.tools.installed");
  if (at?.needsRosetta) return t("servers.setup.tools.ovftoolRosetta", { path: at.path });
  if (at) return t("servers.setup.tools.ovftoolNoRun", { path: at.path });
  return t("servers.setup.tools.ovftool");
}

/** What placing a picked folder came to, in the player's words. */
function said(t: T, placed: OvftoolPlaced): string {
  if (placed.outcome === "placed") return placed.needsRosetta ? t("servers.setup.tools.placedRosetta") : t("servers.setup.tools.placed", { path: placed.path });
  if (placed.reason === "notVmware" && !placed.detail) return t("servers.setup.tools.refused.notVmwareUnknown");
  return t(`servers.setup.tools.refused.${placed.reason}`, { detail: placed.detail ?? "", team: placed.detail ?? "" });
}

/** Picks the unpacked OVF Tool folder and has the core place it; what came of it, to say. */
function usePlaceOvftool(onRefresh: () => void) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ text: string; ok: boolean } | null>(null);
  const place = async () => {
    const folder = await open({ directory: true, multiple: false, title: t("servers.setup.tools.pickOvftool") });
    if (typeof folder !== "string") return;
    setBusy(true);
    setNote({ text: t("servers.setup.tools.placing"), ok: true });
    try {
      const placed = await ovftoolPlace(folder);
      setNote({ text: said(t, placed), ok: placed.outcome === "placed" });
      if (placed.outcome === "placed") onRefresh();
    } finally {
      setBusy(false);
    }
  };
  const choose = () => {
    place().catch((e: unknown) => {
      setNote({ text: describeError(e), ok: false });
    });
  };
  return { busy, note, choose };
}

/** VMware's OVF Tool: found (its version and where), or the ways to get it: the hypervisor that
 *  ships it, the standalone download, or a download already unpacked, placed by the app. */
export function OvftoolRequirement({ s }: { s: HostSetup }) {
  const t = useT();
  const { report, ovftoolOk, canInstallTools, onRefresh } = s;
  const { busy, note, choose } = usePlaceOvftool(onRefresh);
  const hypervisor = ovftoolHypervisor(report);
  return (
    <>
      <Requirement
        ok={ovftoolOk}
        title="VMware OVF Tool"
        detail={detail(t, ovftoolOk, report?.ovftool.version, report?.ovftoolLocation)}
        action={
          <span className="flex flex-col items-end gap-1">
            {hypervisor && (
              <Button
                variant="outline"
                size="xs"
                onClick={() => {
                  openExternal(hypervisor.url);
                }}
              >
                <ExternalLink className="size-3" /> {t("servers.setup.tools.getHypervisor", { name: hypervisor.name })}
              </Button>
            )}
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                openExternal(OVFTOOL_URL);
              }}
            >
              <ExternalLink className="size-3" /> {t("servers.setup.tools.getStandalone")}
            </Button>
            <Button variant="outline" size="xs" disabled={busy || !canInstallTools} onClick={choose}>
              <FolderOpen className="size-3" /> {t("servers.setup.tools.useDownloaded")}
            </Button>
          </span>
        }
      />
      {note && (
        <p role="status" className={`border-t border-border px-4 py-2 text-[0.75rem] break-words ${note.ok ? "text-muted-foreground" : "text-destructive"}`}>
          {note.text}
        </p>
      )}
    </>
  );
}
