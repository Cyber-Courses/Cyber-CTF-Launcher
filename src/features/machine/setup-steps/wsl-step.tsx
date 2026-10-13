"use client";

import { useState } from "react";
import { ExternalLink, RefreshCw, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { StatusDot } from "@/components/ui/status-pill";
import { CmdRow, Log, Num, Requirement } from "@/features/machine/setup-steps/parts";
import { wslState } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { type SystemReport } from "@/lib/tauri";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

const bold = (s: string) => <b>{s}</b>;

/** The Windows "Enable virtualization (WSL 2)" step: turns WSL 2 on, then restarts Windows to
 *  finish. The machine check says where it stands, so a machine already set up just passes. */
export function WslStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const state = wslState(report, setup);
  const [asking, setAsking] = useState(false);

  if (state === "ready") {
    return (
      <div className="overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
        <Requirement ok title={t("machine.body.wsl.readyTitle")} detail={t("machine.body.wsl.readyDetail")} action={null} />
      </div>
    );
  }

  const installing = setup.installing === "wsl";
  function restart() {
    setAsking(false);
    void setup.restart(t("machine.body.wsl.restartReason"));
  }

  return (
    <>
      {state === "needsReboot" ? (
        <div
          role="status"
          className="flex items-center gap-3 rounded-control bg-glass px-4 py-3 text-[0.8125rem] text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
        >
          <StatusDot tone="warn" />
          <span className="min-w-0 flex-1">{setup.restarting ? t("machine.body.wsl.restarting") : t("machine.body.wsl.restart")}</span>
          <Button variant="primary" size="sm" className="shrink-0" disabled={setup.busy || setup.restarting} onClick={() => setAsking(true)}>
            <RotateCcw className="size-3.5" /> {t("machine.body.wsl.restartNow")}
          </Button>
        </div>
      ) : (
        <>
          <p className="text-[0.8125rem] text-muted-foreground">{t("machine.body.wsl.intro")}</p>
          {state === "unknown" &&
            (report.wsl?.virtualizationOff ? (
              <div
                role="status"
                className="mt-3 flex items-start gap-3 rounded-control bg-glass px-4 py-3 text-[0.8125rem] text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
              >
                <span className="mt-1">
                  <StatusDot tone="warn" />
                </span>
                <span className="min-w-0">{t("machine.body.wsl.firmware")}</span>
              </div>
            ) : (
              <p className="mt-2 text-[0.8125rem] text-muted-foreground">{t("machine.body.wsl.unknown")}</p>
            ))}
          <div className="mt-4 flex flex-wrap items-center gap-2">
            <Button variant="primary" size="sm" disabled={setup.busy} onClick={() => setup.install("wsl", "wsl", t("machine.body.wsl.log"))}>
              {installing ? t("machine.body.wsl.turningOn") : t("machine.body.wsl.turnOn")}
            </Button>
            <Button variant="ghost" size="sm" disabled={setup.busy} onClick={setup.onRefresh}>
              <RefreshCw className="size-3.5" /> {t("machine.body.wsl.recheck")}
            </Button>
          </div>
        </>
      )}
      <div className="mt-3">
        <Log setup={setup} of={["wsl"]} />
      </div>
      <details className="mt-4 text-[0.8125rem] text-muted-foreground">
        <summary className="cursor-pointer hover:text-foreground">{t("machine.body.wsl.byHand")}</summary>
        <ol className="mt-3 space-y-3">
          <Num n={1}>{t.rich("machine.body.wsl.step1", { b: bold })}</Num>
          <Num n={2}>
            {t("machine.body.wsl.step2")}
            <div className="mt-1.5">
              <CmdRow cmd="wsl --install" />
            </div>
          </Num>
          <Num n={3}>{t.rich("machine.body.wsl.step3", { b: bold })}</Num>
        </ol>
        <button
          onClick={() => openExternal("https://learn.microsoft.com/windows/wsl/install")}
          className="mt-4 inline-flex items-center gap-1.5 text-[0.75rem] text-jewel-text hover:underline"
        >
          <ExternalLink className="size-3.5" /> {t("machine.body.wsl.guide")}
        </button>
      </details>
      {asking && (
        <ConfirmDialog
          title={t("machine.body.wsl.confirmTitle")}
          confirmLabel={t("machine.body.wsl.restartNow")}
          onConfirm={restart}
          onCancel={() => setAsking(false)}
        >
          {t("machine.body.wsl.confirmBody")} {report.wsl?.reopensAfterRestart ? t("machine.body.wsl.reopens") : t("machine.body.wsl.reopenByHand")}
        </ConfirmDialog>
      )}
    </>
  );
}
