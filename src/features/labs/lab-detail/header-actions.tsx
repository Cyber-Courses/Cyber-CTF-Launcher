"use client";

import { LogIn, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { StartTimer } from "@/features/labs/lab-timers";
import type { HeaderMode, LabAction, LabState } from "@/features/labs/lab-detail/lab-state";
import { ParkedActions, RunningActions } from "@/features/labs/lab-detail/up-actions";
import { tell } from "@/lib/failure";
import { operationStatus } from "@/features/labs/runs/operations";
import { useT } from "@/lib/i18n";

type HeaderActionsProps = {
  mode: Exclude<HeaderMode, "start" | "failed">;
  state: LabState;
  url: string | null | undefined;
  busy: boolean;
  /** An operation the backend reports in flight on the lab (one started before a window reload,
   *  so not this session's `busy`): every action waits for it. */
  deploying?: boolean;
  /** An attack box is being added or removed: Pause and Shut down wait for it. */
  boxBusy?: boolean;
  acting: LabAction | null;
  resetting: boolean;
  onAct: (what: LabAction) => void;
  /** Asks to confirm, then stops and removes the lab. */
  onRemove: () => void;
  onStop: () => Promise<unknown>;
  onLogin?: () => Promise<boolean>;
  /** When the start in flight began, for its timer. */
  startedAt?: number;
};

/**
 * The header's actions for a lab that is up, parked, starting, stopping or interrupted, or
 * logged out ("start" mode is StartLabButton): the one thing to do next, and the others.
 */
export function HeaderActions({
  mode,
  state: s,
  url,
  busy,
  deploying = false,
  boxBusy = false,
  acting,
  resetting,
  onAct,
  onRemove,
  onStop,
  onLogin,
  startedAt,
}: HeaderActionsProps) {
  const locked = busy || deploying;
  switch (mode) {
    case "running":
      return (
        <RunningActions
          state={s}
          url={url}
          locked={locked}
          parkLocked={locked || boxBusy}
          acting={acting}
          resetting={resetting}
          onAct={onAct}
          onRemove={onRemove}
        />
      );
    case "parked":
      return <ParkedActions locked={locked} busy={busy} onAct={onAct} onRemove={onRemove} />;
    case "stopping":
      return <StoppingButton />;
    case "starting":
      return <StartingButton operation={s.operation} startedAt={startedAt} />;
    case "interrupted":
      return <CleanUpButton locked={locked} busy={busy} onStop={onStop} />;
    case "signIn":
      return <SignInButton onLogin={onLogin} />;
  }
}

/** A stop in flight. */
function StoppingButton() {
  const t = useT();
  return (
    <Button variant="destructive" disabled>
      <Spinner className="size-3.5" /> {t("labs.detail.stopping")}
    </Button>
  );
}

/** Machines exist but nothing is deploying and the lab isn't fully up: a previous run was
 *  interrupted (e.g. the app restarted mid-start). Clean it up before a fresh start. */
function CleanUpButton({ locked, busy, onStop }: { locked: boolean; busy: boolean; onStop: HeaderActionsProps["onStop"] }) {
  const t = useT();
  return (
    <Button
      variant="destructive"
      onClick={() => {
        void onStop();
      }}
      disabled={locked}
      title={t("labs.detail.interruptedHint")}
    >
      {busy ? (
        t("labs.detail.cleaningUp")
      ) : (
        <>
          <Square className="size-3.5" /> {t("labs.detail.stopCleanUp")}
        </>
      )}
    </Button>
  );
}

/** A start in flight: what it is doing (starting, resuming…) and for how long. */
function StartingButton({ operation, startedAt }: { operation: LabState["operation"]; startedAt?: number }) {
  const t = useT();
  return (
    <Button variant="primary" disabled>
      <Spinner className="size-3.5" /> {`${operationStatus(operation ?? "launch", t)}…`} <StartTimer from={startedAt} />
    </Button>
  );
}

/** Logged out: say so on the button and log in from it, rather than a greyed-out Start. */
function SignInButton({ onLogin }: { onLogin?: () => Promise<boolean> }) {
  const t = useT();
  return (
    <Button
      variant="primary"
      onClick={() => {
        onLogin?.().catch(tell(t("labs.detail.signInFailed")));
      }}
    >
      <LogIn className="size-3.5" /> {t("labs.detail.signInToStart")}
    </Button>
  );
}
