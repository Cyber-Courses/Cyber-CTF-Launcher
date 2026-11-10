"use client";

import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { type SystemReport } from "@/lib/tauri";
import { type Engine } from "@/features/machine/setup-steps/engines";
import { BusyLabel, CmdRow } from "@/features/machine/setup-steps/parts";
import { engineActions } from "@/features/machine/setup-steps/plan";
import { type EngineActions } from "@/features/machine/setup-steps/use-engine-actions";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";

// What the engine step offers for the picked engine when it isn't the one in use, and the notes
// under it while Docker isn't ready.

type ActionProps = {
  report: SystemReport;
  setup: MachineSetupState;
  choice: Engine;
  engines: Engine[];
  actions: EngineActions;
};

/** What the core's plan says makes the picked engine Docker's: switch to it (it runs), start it
 *  (installed, stopped; a daemon refusing this account says so when started), install it (the
 *  recommended one, missing), or link out (its vendor's; its own install on a distribution the
 *  setup doesn't install on yet), then re-check. */
export function EngineAction(props: ActionProps) {
  const action = engineActions(props.report, props.choice.id).at(0);
  switch (action?.kind) {
    case "useEngine":
      return <SwitchEngine {...props} />;
    case "startEngine":
    case "relogin":
      return <StartEngine {...props} />;
    case "install":
      return <InstallEngine {...props} />;
    case "manual":
      return <GetEngine {...props} distribution={action.reason === "distribution"} />;
    default:
      return null;
  }
}

function SwitchEngine({ report, choice, engines, actions }: ActionProps) {
  const t = useT();
  return (
    <>
      <span className="text-[0.8125rem] text-muted-foreground">
        {t("machine.engine.otherRunning", {
          engine: choice.name,
          current: engines.find((e) => e.id === report.dockerEngine)?.name ?? t("machine.engine.anotherEngine"),
        })}
      </span>
      <Button
        variant="primary"
        size="sm"
        disabled={actions.switching}
        onClick={() => {
          actions.switchTo(choice.id);
        }}
      >
        <BusyLabel busy={actions.switching} label={t("machine.engine.switching")}>
          {t("machine.engine.use", { engine: choice.name })}
        </BusyLabel>
      </Button>
    </>
  );
}

function StartEngine({ choice, actions }: ActionProps) {
  const t = useT();
  const { starting } = actions;
  const startError = actions.startError(choice.id);
  return (
    <>
      <span className="text-[0.8125rem] text-muted-foreground">
        {startError ?? (starting ? t("machine.engine.starting", { engine: choice.name }) : t("machine.engine.notRunning", { engine: choice.name }))}
      </span>
      <span className="flex shrink-0 gap-2">
        {startError && <GetButton engine={choice} />}
        <Button
          variant="primary"
          size="sm"
          disabled={starting}
          onClick={() => {
            actions.start(choice.id);
          }}
        >
          <BusyLabel busy={starting} label={t("machine.engine.startingButton")}>
            {t("machine.engine.start", { engine: choice.name })}
          </BusyLabel>
        </Button>
      </span>
    </>
  );
}

function InstallEngine({ setup, choice }: ActionProps) {
  const t = useT();
  const { installing, canInstall, installerOpened, install } = setup.installs;
  return (
    <>
      <span className="text-[0.8125rem] text-muted-foreground">{t("machine.engine.canInstall", { name: choice.name })}</span>
      {!installerOpened ? (
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            install("docker", "docker", t("machine.engine.installLog"));
          }}
          disabled={!canInstall}
        >
          <BusyLabel busy={installing === "docker"} label={t("machine.engine.installing")}>
            {t("machine.engine.install", { name: choice.name })}
          </BusyLabel>
        </Button>
      ) : (
        <RecheckButton setup={setup} />
      )}
    </>
  );
}

function GetEngine({ setup, choice, distribution }: ActionProps & { distribution: boolean }) {
  const t = useT();
  return (
    <>
      <span className="text-[0.8125rem] text-muted-foreground">
        {distribution ? t("machine.body.distribution", { name: choice.name }) : t("machine.engine.installThenRecheck", { engine: choice.name })}
      </span>
      <span className="flex shrink-0 gap-2">
        <GetButton engine={choice} />
        <RecheckButton setup={setup} />
      </span>
    </>
  );
}

function GetButton({ engine }: { engine: Engine }) {
  const t = useT();
  return (
    <Button
      variant="outline"
      size="sm"
      onClick={() => {
        openExternal(engine.url);
      }}
    >
      <ExternalLink className="size-3.5" /> {t("machine.engine.get", { name: engine.name })}
    </Button>
  );
}

function RecheckButton({ setup }: { setup: MachineSetupState }) {
  const t = useT();
  return (
    <Button
      variant="outline"
      size="sm"
      onClick={() => {
        setup.onRefresh();
      }}
    >
      <RefreshCw className="size-3.5" /> {t("machine.engine.recheck")}
    </Button>
  );
}

/** While Docker isn't ready: Docker Desktop's licence before it is installed, finish the
 *  installer, reboot on Windows, the Linux post-install steps. */
export function EngineHints({ report, choice, installerOpened }: { report: SystemReport; choice: Engine; installerOpened: boolean }) {
  const t = useT();
  const isWin = report.os === "windows";
  const isMac = report.os === "macos";
  return (
    <>
      {choice.id === "docker-desktop" && !report.docker.installed && (
        <p className="text-[0.75rem] text-muted-foreground">{t("machine.engine.dockerDesktopLicence")}</p>
      )}
      {installerOpened && (
        <p className="text-[0.75rem] text-muted-foreground">
          {choice.id === "docker-desktop" ? t("machine.engine.finishDockerDesktop") : t("machine.engine.finishOther", { engine: choice.name })}
        </p>
      )}
      {isWin && <p className="text-[0.75rem] text-muted-foreground">{t("machine.engine.windowsReboot")}</p>}
      {!isMac && !isWin && choice.id === "docker-engine" && (
        <div className="space-y-2 rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)] p-3 text-left text-[0.75rem] text-muted-foreground">
          <p>{t("machine.engine.linuxPostInstall")}</p>
          <CmdRow cmd="sudo usermod -aG docker $USER" />
          <CmdRow cmd="sudo systemctl enable --now docker" />
          <p>{t("machine.engine.linuxRelog")}</p>
        </div>
      )}
    </>
  );
}
