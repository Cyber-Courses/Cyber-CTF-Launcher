"use client";

import { useState } from "react";
import { Check, Copy, RotateCcw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { KeyValue } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { restartApp, type AgentInfo, type SystemReport } from "@/lib/tauri";
import { useDeployingLabs } from "@/lib/deploy-store";
import { getAttackBox, getAttackImage, getVmProvider } from "@/lib/settings";
import { Row, Section } from "@/features/settings/settings-layout";
import { ONBOARDED_KEY } from "@/features/settings/settings-screen";
import { diagnosticsText } from "@/features/settings/diagnostics";
import { useUpdater, type UpdateState } from "@/features/settings/use-updater";
import { tell } from "@/lib/failure";
import { emit } from "@tauri-apps/api/event";
import { closeIfSettings, REPLAY_ONBOARDING_EVENT } from "@/lib/app-events";
import { useFormat, useT } from "@/lib/i18n";

/* ------------------------------------------------------------------ about */

export function AboutSection({ version, report, agent }: { version: string | null; report: SystemReport | null; agent: AgentInfo | null }) {
  const t = useT();
  const [copied, setCopied] = useState(false);

  // Diagnostics stay in English: they go to support with a bug report.
  async function copyDiagnostics() {
    const text = diagnosticsText(version, report, agent, { attackImage: getAttackImage(), attackBox: getAttackBox(), vmProvider: getVmProvider() });
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      /* clipboard unavailable */
    }
  }

  // The onboarding shows in the main window: it is asked to show it again (a reload here only
  // reloaded the Settings window, which never shows it).
  function replayOnboarding() {
    try {
      localStorage.removeItem(ONBOARDED_KEY);
    } catch {
      /* ignore */
    }
    emit(REPLAY_ONBOARDING_EVENT).catch(tell(t("settings.about.replayFailed")));
    closeIfSettings();
  }

  return (
    <Section title={t("settings.about.title")} description={t("settings.about.description")}>
      <KeyValue k={t("settings.about.version")}>{version ? `v${version}` : "…"}</KeyValue>
      {report && (
        <KeyValue k={t("settings.about.system")}>
          {report.os} {report.arch}
        </KeyValue>
      )}
      {agent && <KeyValue k={t("settings.about.installId")}>{agent.installId}</KeyValue>}
      <UpdatesRow />
      <Row
        title={t("settings.about.diagnostics")}
        description={t("settings.about.diagnosticsDescription")}
        control={
          <Button variant="outline" size="xs" onClick={copyDiagnostics}>
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

/** Updates: where the last check left things, and the next step (check, install, restart). */
function UpdatesRow() {
  const t = useT();
  const { upd, checkedAt, checkUpdates, install } = useUpdater();
  // Installing quits every Cyber CTF process, the deploy workers included (the installer ends
  // them by name on Windows), so an update waits for running deploys to finish.
  const deploying = useDeployingLabs().size > 0;
  return (
    <Row
      title={
        <span className="flex items-center gap-2">
          {t("settings.about.updates")}
          {upd.phase === "available" && <Badge variant="accent">{t("settings.about.updateAvailable")}</Badge>}
        </span>
      }
      description={<UpdateStatus upd={upd} checkedAt={checkedAt} deploying={deploying} />}
      control={
        upd.phase === "available" || upd.phase === "installing" ? (
          <Button size="xs" onClick={install} disabled={upd.phase === "installing" || deploying}>
            {upd.phase === "installing" ? (
              <>
                <Spinner className="size-3.5" /> {t("settings.about.installing")}
              </>
            ) : (
              t("settings.about.installUpdate")
            )}
          </Button>
        ) : upd.phase === "installed" ? (
          <Button size="xs" onClick={() => restartApp().catch(tell(t("settings.about.restartFailed")))}>
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
function UpdateStatus({ upd, checkedAt, deploying }: { upd: UpdateState; checkedAt: number | null; deploying: boolean }) {
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
      return deploying ? t("shell.update.waitForDeploys") : t("settings.about.status.available", { version: upd.update.version });
    case "installing":
      return t("settings.about.status.installing", { version: upd.update.version });
    case "installed":
      return t("settings.about.status.installed", { version: upd.update.version });
    default:
      return t("settings.about.status.idle");
  }
}
