"use client";

import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { type DockerEngine, type SystemReport } from "@/lib/tauri";
import { type Engine } from "@/features/machine/setup-steps/engines";
import { BusyLabel, CmdRow } from "@/features/machine/setup-steps/parts";
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
  recommended: DockerEngine;
  actions: EngineActions;
  ready: boolean;
};

/** Running but not the one Docker uses: switch to it. Installed but stopped: start it. The
 *  recommended one, missing: install it. Anything else: link out, then re-check. */
export function EngineAction(props: ActionProps) {
  const { report, choice, recommended, ready } = props;
  if (report.dockerEnginesRunning.includes(choice.id)) return <SwitchEngine {...props} />;
  if (report.docker.installed && !ready) return <StartEngine {...props} />;
  if (choice.id === recommended && !report.docker.installed) return <InstallEngine {...props} />;
  return <GetEngine {...props} />;
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
  const { installing, installerOpened, install } = setup;
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
          disabled={installing !== null}
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

function GetEngine({ setup, choice }: ActionProps) {
  const t = useT();
  return (
    <>
      <span className="text-[0.8125rem] text-muted-foreground">{t("machine.engine.installThenRecheck", { engine: choice.name })}</span>
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

/** A button's label, or a spinner and `label` while it runs. */
/** While Docker isn't ready: finish the installer, reboot on Windows, the Linux post-install steps. */
export function EngineHints({ report, choice, installerOpened }: { report: SystemReport; choice: Engine; installerOpened: boolean }) {
  const t = useT();
  const isWin = report.os === "windows";
  const isMac = report.os === "macos";
  return (
    <>
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
