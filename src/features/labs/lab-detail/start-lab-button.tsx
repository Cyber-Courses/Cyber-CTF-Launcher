"use client";

import { useState } from "react";
import { Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PortChoice } from "@/features/labs/port-mode-prompt";
import { RunOnDialog, RunOnPicker, type RunTarget } from "@/features/labs/run-on";
import type { RunTargetChoice } from "@/features/labs/lab-detail/use-run-target";
import type { Lab } from "@/features/labs/use-labs";
import { CloudLaunchNote } from "@/features/labs/cloud-panel";
import { cloudNames } from "@/features/labs/lab-runtime";
import { joinOr } from "@/features/labs/lab-readiness";
import { serverOpenSetup, type CloudPreview } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** Start lab: with somewhere else to run it (servers, a VM here, hosted), asks where first in
 *  "Where should it run?"; otherwise starts here right away. A cloud lab always asks (which
 *  account, and what it costs), and with no account of its cloud offers to connect one. */
export function StartLabButton({
  lab,
  run,
  isDocker,
  cloud = null,
  loggedIn,
  hostedLive,
  busy,
  dockerRunning,
  onStart,
}: {
  lab: Lab;
  run: RunTargetChoice;
  isDocker: boolean;
  /** A cloud lab: its cost and inputs (`preview`, once read). Null for the other labs. */
  cloud?: { preview: CloudPreview | null } | null;
  loggedIn: boolean;
  /** A hosted session of this lab is up: one run at a time. */
  hostedLive: boolean;
  busy: boolean;
  dockerRunning: boolean | null;
  onStart: (target: RunTarget) => void;
}) {
  const t = useT();
  const [choosing, setChoosing] = useState(false);
  const rt = lab.runtime;
  if (cloud && run.cloudAccounts.length === 0) return <ConnectCloudButton providers={rt?.providers ?? []} />;
  return (
    <div className="relative">
      <Button
        variant="primary"
        onClick={() => (run.hasChoice ? setChoosing(true) : onStart({ kind: "local" }))}
        disabled={!loggedIn || !rt || hostedLive}
        title={!rt ? t("labs.detail.noRuntime") : !loggedIn ? t("labs.detail.signInHint") : hostedLive ? t("labs.detail.hostedLive") : undefined}
        aria-haspopup={run.hasChoice ? "dialog" : undefined}
      >
        <Play className="size-3.5" /> {t("labs.detail.startLab")}
      </Button>
      {choosing && (
        <StartDialog
          lab={lab}
          run={run}
          isDocker={isDocker}
          cloud={cloud}
          busy={busy}
          dockerRunning={dockerRunning}
          onClose={() => setChoosing(false)}
          onStart={(target) => {
            setChoosing(false);
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
}: {
  lab: Lab;
  run: RunTargetChoice;
  isDocker: boolean;
  cloud: { preview: CloudPreview | null } | null;
  busy: boolean;
  dockerRunning: boolean | null;
  onClose: () => void;
  onStart: (target: RunTarget) => void;
}) {
  const t = useT();
  const rt = lab.runtime;
  return (
    <RunOnDialog
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            {t("labs.detail.cancel")}
          </Button>
          <Button variant="primary" size="sm" disabled={!!cloud && !run.pickedAccount} onClick={() => onStart(run.chosen())}>
            <Play className="size-3.5" /> {t("labs.detail.start")}
          </Button>
        </>
      }
    >
      <RunOnPicker
        title={lab.title}
        hosts={run.hosts}
        hostOk={run.hostOk}
        localNote={isDocker ? t("labs.detail.localNoteContainers") : t("labs.detail.localNoteVm")}
        localVm={run.localVm}
        hosted={!!rt?.hosted}
        dockerRunning={isDocker ? dockerRunning : null}
        value={run.runOn}
        onChange={run.setRunOn}
        disabled={busy}
        offerLocal={!cloud}
        localExtra={
          run.runOn.kind === "local" &&
          isDocker && (
            <div className="space-y-1.5">
              <p className="section-label">{t("labs.detail.portsHere")}</p>
              <PortChoice value={run.ports} onChange={run.setPorts} />
            </div>
          )
        }
      />
      {cloud && (
        <div className="mt-4">
          <CloudLaunchNote preview={cloud.preview} autoStopHours={run.pickedAccount?.autoStopHours ?? null} />
        </div>
      )}
    </RunOnDialog>
  );
}

/** A cloud lab with no account of its cloud: connecting one is the next step. */
function ConnectCloudButton({ providers }: { providers: readonly string[] }) {
  const t = useT();
  return (
    <Button variant="primary" onClick={() => void serverOpenSetup(null, "cloud").catch(tell(t("labs.row.connectFailed")))}>
      <Play className="size-3.5" /> {t("labs.detail.connectAccount", { clouds: joinOr(cloudNames(providers)) })}
    </Button>
  );
}
