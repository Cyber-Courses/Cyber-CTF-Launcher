"use client";

import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MachineSetup } from "@/features/machine/machine-setup";
import { Toaster } from "@/components/ui/toaster";
import { systemCheck, type SystemReport } from "@/lib/tauri";
import { warn } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { useTauriEvent } from "@/lib/use-tauri-event";

import { installDevMock } from "@/lib/dev-mock";

// Development only: ?mock in a plain browser answers the Tauri commands with sample data.
installDevMock();

/** The guided machine-setup window (opened by `machine_open_setup`, optionally `?step=`). */
export default function MachineSetupWindow() {
  const [report, setReport] = useState<SystemReport | null>(null);
  // The check failed before any answered: the window says so, with Try again, instead of a
  // spinner over silent retries. Once a report is in, a later failure keeps it (polled again).
  const [checkError, setCheckError] = useState<string | null>(null);
  // Deep link: the step in the URL on open, or a later "Fix" while the window is open.
  const [startAt, setStartAt] = useState<{ step: string; nonce: number } | null>(() => {
    const step = typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("step");
    return step ? { step, nonce: 0 } : null;
  });
  // Poll so an install (incl. one the user finishes in a native installer) is detected as
  // done and the steps update, without a manual re-check.
  const stuck = checkError !== null && report === null;
  const check = usePoll(systemCheck, 5000, {
    serial: true,
    enabled: !stuck,
    onValue: (r) => {
      setReport(r);
      setCheckError(null);
    },
    onError: (e) => {
      warn("system check")(e);
      setCheckError(String(e));
    },
  });
  // Try again: one check now; it starts the poll again once it answers.
  const retry = () =>
    systemCheck().then(
      (r) => {
        setReport(r);
        setCheckError(null);
      },
      (e) => setCheckError(String(e)),
    );
  useTauriEvent<string>("machine-setup-step", (step) => setStartAt({ step, nonce: Date.now() }));
  const close = () => getCurrentWindow().close().catch(warn("closing the window"));

  return (
    <>
      <MachineSetup report={report} checkError={checkError} onRetryCheck={retry} onRefresh={check} onClose={close} startAt={startAt} />
      {/* This window has its own root (no app shell), so it mounts its own toasts. */}
      <Toaster />
    </>
  );
}
