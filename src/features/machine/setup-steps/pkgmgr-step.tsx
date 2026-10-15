"use client";

import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { CmdRow, Requirement } from "@/features/machine/setup-steps/parts";
import { type SystemReport } from "@/lib/tauri";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

const BREW_INSTALL = '/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"';

/** The package manager the installs go through: found, or how to get it on this OS. */
export function PackageManagerStep({ report }: { report: SystemReport }) {
  const t = useT();
  // Homebrew builds from source on Intel Macs (no bottles since macOS 26): say so before the
  // installs that go through it, so a long "Installing…" isn't taken for a hang.
  const intelMac = report.os === "macos" && report.arch === "x86_64" && (
    <p className="mt-3 text-[0.75rem] text-muted-foreground">{t("machine.body.pkgmgr.intelMac")}</p>
  );
  if (report.pkgManager.installed) {
    return (
      <>
        <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
          <Requirement ok title={report.pkgManager.name} detail={report.pkgManager.version ?? t("machine.body.installed")} action={null} />
        </div>
        {intelMac}
      </>
    );
  }
  return (
    <>
      {intelMac}
      <GetPackageManager report={report} />
    </>
  );
}

/** Homebrew's install line on macOS, winget from the Store on Windows, the distro's own on Linux. */
function GetPackageManager({ report }: { report: SystemReport }) {
  const t = useT();
  if (report.os === "macos")
    return (
      <div className="space-y-2">
        <p className="text-[0.8125rem] text-muted-foreground">{t("machine.body.pkgmgr.macRun")}</p>
        <CmdRow cmd={BREW_INSTALL} />
        <button
          onClick={() => {
            openExternal("https://brew.sh");
          }}
          className="inline-flex items-center gap-1.5 text-[0.75rem] text-jewel-text hover:underline"
        >
          <ExternalLink className="size-3.5" /> brew.sh
        </button>
      </div>
    );
  if (report.os === "windows")
    return (
      <div className="flex items-center justify-between gap-3 rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)] px-4 py-3">
        <div>
          <p className="text-[0.8125rem] font-medium text-foreground">{t("machine.body.pkgmgr.winget")}</p>
          <p className="text-[0.75rem] text-muted-foreground">{t("machine.body.pkgmgr.wingetHint")}</p>
        </div>
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            openExternal("https://apps.microsoft.com/detail/9nblggh4nns1");
          }}
        >
          <ExternalLink className="size-3.5" /> {t("machine.body.pkgmgr.get")}
        </Button>
      </div>
    );
  return (
    <p className="text-[0.8125rem] text-muted-foreground">
      {report.pkgManager.name ? t("machine.body.pkgmgr.linuxNamed", { name: report.pkgManager.name }) : t("machine.body.pkgmgr.linux")}
    </p>
  );
}
