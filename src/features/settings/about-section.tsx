"use client";

import { useState } from "react";
import { Check, Copy, FolderOpen, RotateCcw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { KeyValue } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { openLogFolder, type AgentInfo, type SystemReport, type UpdateChannel } from "@/lib/tauri";
import { useLabsBusy } from "@/features/labs/runs/operations";
import { getAttackImage, getVmProvider } from "@/lib/settings";
import { Row, Section } from "@/features/settings/settings-layout";
import { clearOnboarded } from "@/lib/onboarded-flag";
import { diagnosticsText } from "@/features/settings/diagnostics";
import { useUpdater, type UpdateState } from "@/features/settings/use-updater";
import { restartOnUpdate } from "@/lib/install-update";
import { ignore, tell } from "@/lib/failure";
import { emit } from "@tauri-apps/api/event";
import { closeIfSettings, REPLAY_ONBOARDING_EVENT } from "@/lib/app-events";
import { useFormat, useT } from "@/lib/i18n";

/* ------------------------------------------------------------------ about */

/** Copies the diagnostics for a bug report; they stay in English: they go to support. */
function DiagnosticsRow({ version, report, agent }: { version: string | null; report: SystemReport | null; agent: AgentInfo | null }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const copy = () => {
    const text = diagnosticsText(version, report, agent, { attackImage: getAttackImage(), vmProvider: getVmProvider() });
    // Inside the chain, so a webview without the clipboard API fails here too, quietly.
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(text))
      .then(() => {
        setCopied(true);
        setTimeout(() => {
          setCopied(false);
        }, 1600);
      }, ignore("the clipboard is unavailable"));
  };
  return (
    <Row
      title={t("settings.about.diagnostics")}
      description={t("settings.about.diagnosticsDescription")}
      control={
        <Button variant="outline" size="xs" onClick={copy}>
          {copied ? (
            <>
              <Check className="size-3.5 text-success" /> {t("settings.about.copied")}
            </>
          ) : (
            <>
              <Copy className="size-3.5" /> {t("settings.about.copy")}
            </>
          )}
        </Button>
      }
    />
  );
}

export function AboutSection({
  version,
  versionError,
  report,
  agent,
  updates,
}: {
  /** The app's version; null until it is read. */
  version: string | null;
  /** Why reading the version failed, if it did. */
  versionError?: unknown;
  report: SystemReport | null;
  agent: AgentInfo | null;
  /** Where this copy's updates come from (null until known). */
  updates: UpdateChannel | null;
}) {
  const t = useT();

  // The onboarding shows in the main window: it is asked to show it again (a reload here only
  // reloaded the Settings window, which never shows it).
  function replayOnboarding() {
    clearOnboarded();
    emit(REPLAY_ONBOARDING_EVENT).catch(tell(t("settings.about.replayFailed")));
    closeIfSettings();
  }

  return (
    <Section title={t("settings.about.title")} description={t("settings.about.description")}>
      <KeyValue k={t("settings.about.version")}>{version ? `v${version}` : versionError === undefined ? "…" : t("settings.about.versionUnknown")}</KeyValue>
      {report && (
        <KeyValue k={t("settings.about.system")}>
          {report.os} {report.arch}
        </KeyValue>
      )}
      {agent && <KeyValue k={t("settings.about.installId")}>{agent.installId}</KeyValue>}
      {updates === null || updates === "inApp" ? <UpdatesRow /> : <ManagedUpdatesRow channel={updates} />}
      <DiagnosticsRow version={version} report={report} agent={agent} />
      <Row
        title={t("settings.about.logs")}
        description={t("settings.about.logsDescription")}
        control={
          <Button
            variant="outline"
            size="xs"
            onClick={() => {
              openLogFolder().catch(tell(t("settings.about.logsFailed")));
            }}
          >
            <FolderOpen className="size-3.5" /> {t("settings.about.openLogs")}
          </Button>
        }
      />
      <Row
        title={t("settings.about.firstRun")}
        description={t("settings.about.firstRunDescription")}
        control={
          <Button variant="ghost" size="xs" onClick={replayOnboarding}>
            <RotateCcw className="size-3.5" /> {t("settings.about.replay")}
          </Button>
        }
      />
    </Section>
  );
}

/** Updates that come from elsewhere (the organization, or the package manager that installed the
 *  app): which, and no button, since the in-app updater is off. */
function ManagedUpdatesRow({ channel }: { channel: Exclude<UpdateChannel, "inApp"> }) {
  const t = useT();
  return <Row title={t("settings.about.updates")} description={t(`settings.about.channel.${channel}`)} />;
}

/** Updates: where the last check left things, and the next step (check, install, restart). */
function UpdatesRow() {
  const t = useT();
  const { upd, checkedAt, checkUpdates, install } = useUpdater();
  // Installing quits every Cyber CTF process, the deploy workers included (the installer ends
  // them by name on Windows), so an update waits for every lab operation to finish.
  const busy = useLabsBusy();
  return (
    <Row
      title={
        <span className="flex items-center gap-2">
          {t("settings.about.updates")}
          {upd.phase === "available" && <Badge variant="accent">{t("settings.about.updateAvailable")}</Badge>}
        </span>
      }
      description={<UpdateStatus upd={upd} checkedAt={checkedAt} busy={busy} />}
      control={
        upd.phase === "available" || upd.phase === "installing" ? (
          <Button size="xs" onClick={install} disabled={upd.phase === "installing" || busy}>
            {upd.phase === "installing" ? (
              <>
                <Spinner className="size-3.5" /> {t("settings.about.installing")}
              </>
            ) : (
              t("settings.about.installUpdate")
            )}
          </Button>
        ) : upd.phase === "installed" ? (
          <Button
            size="xs"
            onClick={() => {
              void restartOnUpdate();
            }}
          >
            {t("settings.about.restartNow")}
          </Button>
        ) : (
          <Button variant="outline" size="xs" onClick={checkUpdates} disabled={upd.phase === "checking"}>
            {upd.phase === "checking" ? (
              <>
                <Spinner className="size-3.5" /> {t("settings.about.checking")}
              </>
            ) : (
              t("settings.about.checkUpdates")
            )}
          </Button>
        )
      }
    />
  );
}

/** The update check's status line. */
function UpdateStatus({ upd, checkedAt, busy }: { upd: UpdateState; checkedAt: number | null; busy: boolean }) {
  const t = useT();
  const format = useFormat();
  switch (upd.phase) {
    case "checking":
      return t("settings.about.status.checking");
    case "none":
      return checkedAt
        ? t("settings.about.status.latestAt", { time: format.date(checkedAt, { hour: "2-digit", minute: "2-digit" }) })
        : t("settings.about.status.latest");
    case "error":
      return t("settings.about.status.error");
    case "available":
      return busy ? t("shell.update.waitForLabs") : t("settings.about.status.available", { version: upd.update.version });
    case "installing":
      return t("settings.about.status.installing", { version: upd.update.version });
    case "installed":
      return t("settings.about.status.installed", { version: upd.update.version });
    default:
      return t("settings.about.status.idle");
  }
}
