"use client";

import { useState } from "react";
import { Check, Copy, Download, Mail } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { saveDiagnostics, type AgentInfo, type SystemReport } from "@/lib/tauri";
import { getAttackImage, getVmProvider } from "@/lib/settings";
import { Row } from "@/features/settings/settings-layout";
import { bundleFileName, bundleSettings, diagnosticsText, supportMail } from "@/features/settings/diagnostics";
import { pickSaveFile } from "@/lib/pickers";
import { ignore, tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { useFlash } from "@/lib/use-flash";

/** Copies the diagnostics for a bug report (in English: they go to support), or saves the whole
 *  bundle as one file to attach, and can open a mail to support with it. Once saved, a status line
 *  (announced to screen readers) says where the file is and to attach it. */
export function DiagnosticsRow({ version, report, agent }: { version: string | null; report: SystemReport | null; agent: AgentInfo | null }) {
  const t = useT();
  const [copied, flashCopied] = useFlash(1600);
  const bundle = useBundle(agent);
  const copy = () => {
    const text = diagnosticsText(version, report, agent, { attackImage: getAttackImage(), vmProvider: getVmProvider() });
    // Inside the chain, so a webview without the clipboard API fails here too, quietly.
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(text))
      .then(flashCopied, ignore("the clipboard is unavailable"));
  };
  const mail = (path: string) => {
    const subject = t("settings.about.mailSubject", { version: version ?? "?" });
    const body = t("settings.about.mailBody", { path });
    return openUrl(supportMail(subject, body));
  };
  return (
    <Row
      title={t("settings.about.diagnostics")}
      description={
        <>
          {t("settings.about.diagnosticsDescription")} {t("settings.about.saveHint")}
          <span role="status" aria-live="polite" className="mt-1 block">
            {bundle.savedTo && t("settings.about.savedTo", { path: bundle.savedTo })}
          </span>
        </>
      }
      control={
        <div className="flex flex-wrap items-center justify-end gap-2" role="group" aria-label={t("settings.about.diagnostics")}>
          <Button variant="outline" size="xs" onClick={copy}>
            {copied ? <Check aria-hidden className="size-3.5 text-success" /> : <Copy aria-hidden className="size-3.5" />}{" "}
            {copied ? t("settings.about.copied") : t("settings.about.copy")}
          </Button>
          <Button
            variant="outline"
            size="xs"
            disabled={bundle.busy !== false}
            aria-busy={bundle.busy === "save"}
            onClick={() => {
              bundle.save().catch(tell(t("settings.about.saveFailed")));
            }}
          >
            {bundle.busy === "save" ? <Spinner className="size-3.5" /> : <Download aria-hidden className="size-3.5" />} {t("settings.about.saveDiagnostics")}
          </Button>
          <Button
            variant="outline"
            size="xs"
            disabled={bundle.busy !== false}
            aria-busy={bundle.busy === "mail"}
            onClick={() => {
              bundle.save("mail", mail).catch(tell(t("settings.about.sendFailed")));
            }}
          >
            {bundle.busy === "mail" ? <Spinner className="size-3.5" /> : <Mail aria-hidden className="size-3.5" />} {t("settings.about.sendToSupport")}
          </Button>
        </div>
      }
    />
  );
}

/** What a bundle save is doing: nothing, saving for Save, or saving for Send to support. */
type Busy = false | "save" | "mail";

/** Saving the diagnostics bundle where the player picks: `save(why, then)` asks where, saves, then
 *  runs `then` with the file's path (opening the mail); `savedTo`, the last file saved. */
function useBundle(agent: AgentInfo | null) {
  const t = useT();
  const [busy, setBusy] = useState<Busy>(false);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const save = async (why: Exclude<Busy, false> = "save", then?: (path: string) => Promise<void>) => {
    const path = await pickSaveFile({
      title: t("settings.about.saveDiagnostics"),
      defaultPath: bundleFileName(new Date()),
      filters: [{ name: t("settings.about.bundleFilter"), extensions: ["gz"] }],
    });
    if (path === null) return;
    setBusy(why);
    try {
      await saveDiagnostics(path, bundleSettings(agent));
      setSavedTo(path);
      await then?.(path);
    } finally {
      setBusy(false);
    }
  };
  return { busy, savedTo, save };
}
