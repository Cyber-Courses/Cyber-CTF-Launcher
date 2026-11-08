"use client";

import { useState } from "react";
import { Play, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PortsStep, RememberToggle } from "@/features/labs/lab-detail/ports-step";
import { RunOnDialog, RunOnPicker, type RunTarget } from "@/features/labs/run-on";
import type { RunTargetChoice } from "@/features/labs/lab-detail/use-run-target";
import type { Lab } from "@/features/labs/use-labs";
import { CloudLaunchNote } from "@/features/labs/cloud-panel";
import { cloudNames } from "@/features/labs/lab-runtime";
import { joinOr } from "@/features/labs/lab-readiness";
import { serverOpenSetup, type CloudPreview } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT, type T } from "@/lib/i18n";

type StartLabProps = {
  lab: Lab;
  run: RunTargetChoice;
  isDocker: boolean;
  /** A cloud lab: its cost and inputs (`preview`, once read). Null for the other labs. */
  cloud?: { preview: CloudPreview | null } | null;
  loggedIn: boolean;
  busy: boolean;
  dockerRunning: boolean | null;
  onStart: (target: RunTarget) => void;
  /** The last start failed: the same button, as Retry. */
  retry?: boolean;
  /** Open "Where should it run?" at once (the player pressed "Sign in to start" and is signed in
   *  now: the question is the next step of what they asked for, see `useStartAfterSignIn`). */
  chooseAtOnce?: boolean;
};

/** Why Start is greyed out (no runtime, signed out), if it is. */
function startHint(t: T, { runtime, loggedIn }: { runtime: boolean; loggedIn: boolean }): string | undefined {
  if (!runtime) return t("labs.detail.noRuntime");
  if (!loggedIn) return t("labs.detail.signInHint");
  return undefined;
}

/** Start lab: with somewhere else to run it (servers, a VM here), asks where first in
 *  "Where should it run?"; otherwise starts here right away. A cloud lab always asks (which
 *  account, and what it costs), and with no account of its cloud offers to connect one. */
export function StartLabButton({
  lab,
  run,
  isDocker,
  cloud = null,
  loggedIn,
  busy,
  dockerRunning,
  onStart,
  retry = false,
  chooseAtOnce = false,
}: StartLabProps) {
  const t = useT();
  const [choosing, setChoosing] = useState(chooseAtOnce);
  const rt = lab.runtime;
  if (cloud && run.cloudAccounts.length === 0) return <ConnectCloudButton providers={rt?.providers ?? []} />;
  return (
    <div className="relative">
      <Button
        variant="primary"
        onClick={() => {
          if (run.hasChoice) setChoosing(true);
          // Nothing to ask: run here at once, carrying a remembered engine (ports are asked/saved
          // in the launch path as before).
          else onStart(run.chosenEngine ? { kind: "local", engine: run.chosenEngine } : { kind: "local" });
        }}
        disabled={!loggedIn || !rt}
        title={startHint(t, { runtime: !!rt, loggedIn })}
        aria-haspopup={run.hasChoice ? "dialog" : undefined}
      >
        {retry ? <RotateCw className="size-3.5" /> : <Play className="size-3.5" />} {retry ? t("labs.detail.retry") : t("labs.detail.startLab")}
      </Button>
      {choosing && (
        <StartDialog
          lab={lab}
          run={run}
          isDocker={isDocker}
          cloud={cloud}
          busy={busy}
          dockerRunning={dockerRunning}
          onClose={() => {
            setChoosing(false);
            run.resetSteps();
          }}
          onStart={(target) => {
            setChoosing(false);
            run.resetSteps();
            onStart(target);
          }}
        />
      )}
    </div>
  );
}

/** "Where should it run?", with the host ports for a container lab here and a cloud lab's cost;
 *  Start goes with the target picked. */
function StartDialog({
  lab,
  run,
  isDocker,
  cloud,
  busy,
  dockerRunning,
  onClose,
  onStart,
}: Pick<StartLabProps, "lab" | "run" | "isDocker" | "busy" | "dockerRunning" | "onStart"> & { cloud: StartLabProps["cloud"]; onClose: () => void }) {
  const t = useT();
  const footer = <StartFooter run={run} cloudGated={!!cloud} onClose={onClose} onStart={onStart} />;
  return (
    <RunOnDialog onClose={onClose} footer={footer}>
      {run.step === "ports" ? (
        <PortsStep title={lab.title} run={run} />
      ) : (
        <>
          <RunOnPicker
            title={lab.title}
            hosts={run.hosts}
            hostOk={run.hostOk}
            localNote={isDocker ? t("labs.detail.localNoteContainers") : t("labs.detail.localNoteVm")}
            localVm={run.localVm}
            localVms={run.localVms}
            onPickVm={run.pickVm}
            perMachine={run.perMachine}
            dockerRunning={isDocker ? dockerRunning : null}
            engine={run.engine}
            value={run.runOn}
            onChange={run.setRunOn}
            disabled={busy}
            offerLocal={!cloud}
            offerContainers={isDocker}
            step={run.step}
            onPickPlatform={run.pickPlatform}
          />
          {run.step === "vmForms" && <RememberToggle run={run} />}
        </>
      )}
      {cloud && (
        <div className="mt-4">
          <CloudLaunchNote preview={cloud.preview} autoStopHours={run.pickedAccount?.autoStopHours ?? null} />
        </div>
      )}
    </RunOnDialog>
  );
}

/** The dialog's actions: Back (off the places step), Cancel, and Start. Start waits for a cloud
 *  account to be picked (`cloudGated`). */
function StartFooter({ run, cloudGated, onClose, onStart }: { run: RunTargetChoice; cloudGated: boolean; onClose: () => void; onStart: (t: RunTarget) => void }) {
  const t = useT();
  return (
    <>
      {run.step !== "platform" && (
        <Button variant="ghost" size="sm" onClick={run.back}>
          {t("labs.runOn.back")}
        </Button>
      )}
      <Button variant="ghost" size="sm" onClick={onClose}>
        {t("labs.detail.cancel")}
      </Button>
      <Button
        variant="primary"
        size="sm"
        disabled={cloudGated && !run.pickedAccount}
        onClick={() => {
          run.commitChoice();
          onStart(run.chosen());
        }}
      >
        <Play className="size-3.5" /> {t("labs.detail.start")}
      </Button>
    </>
  );
}

/** A cloud lab with no account of its cloud: connecting one is the next step. */
function ConnectCloudButton({ providers }: { providers: readonly string[] }) {
  const t = useT();
  return (
    <Button
      variant="primary"
      onClick={() => {
        serverOpenSetup(null, "cloud").catch(tell(t("labs.row.connectFailed")));
      }}
    >
      <Play className="size-3.5" /> {t("labs.detail.connectAccountOn", { clouds: joinOr(cloudNames(providers)) })}
    </Button>
  );
}
