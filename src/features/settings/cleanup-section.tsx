"use client";

import { DownloadsPanel } from "@/features/machine/downloads-panel";
import { RunningNowPanel } from "@/features/machine/running-now-panel";
import { SectionTitle } from "@/features/settings/settings-layout";
import { InstalledToolsPanel } from "@/features/settings/installed-tools-panel";
import { useT } from "@/lib/i18n";

/** Cleanup: stop anything still running on this machine (a stuck or leftover lab), reclaim disk
 *  from downloaded images and VM boxes, and remove tools Cyber CTF installed. Reuses the Machine
 *  page's panels. */
export function CleanupSection() {
  const t = useT();
  return (
    <section className="space-y-4">
      <SectionTitle title={t("settings.cleanup.title")} description={t("settings.cleanup.description")} />
      <RunningNowPanel refreshKey={null} />
      <DownloadsPanel />
      <InstalledToolsPanel />
    </section>
  );
}
