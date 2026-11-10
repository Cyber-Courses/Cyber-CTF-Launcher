"use client";

import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MachineSetup } from "@/features/machine/machine-setup";
import { Toaster } from "@/components/ui/toaster";
import { DevMockGate } from "@/components/dev-mock-gate";
import { warn } from "@/lib/failure";
import { useMachineReport } from "@/features/machine/use-machine-report";
import { useTauriEvent } from "@/lib/use-tauri-event";

/** The guided machine-setup window (opened by `machine_open_setup`, optionally `?step=`). */
export default function MachineSetupPage() {
  return (
    <DevMockGate>
      <MachineSetupWindow />
    </DevMockGate>
  );
}

function MachineSetupWindow() {
  const { report, checkError, refreshReport } = useMachineReport();
  // Deep link: the step in the URL on open, or a later "Fix" while the window is open.
  const [startAt, setStartAt] = useState<{ step: string; nonce: number } | null>(() => {
    const step = typeof window === "undefined" ? null : new URLSearchParams(window.location.search).get("step");
    return step ? { step, nonce: 0 } : null;
  });
  useTauriEvent("machine-setup-step", (step) => {
    if (typeof step === "string") setStartAt({ step, nonce: Date.now() });
  });
  const close = () => {
    getCurrentWindow().close().catch(warn("closing the window"));
  };

  return (
    <>
      <MachineSetup report={report} checkError={checkError} onRetryCheck={refreshReport} onRefresh={refreshReport} onClose={close} startAt={startAt} />
      {/* This window has its own root (no app shell), so it mounts its own toasts. */}
      <Toaster />
    </>
  );
}
