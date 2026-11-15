"use client";

import { useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MachineSetup } from "@/features/machine/machine-setup";
import { Toaster } from "@/components/ui/toaster";
import { DialogPrompts } from "@/components/destructive-prompt";
import { DevMockGate } from "@/components/dev-mock-gate";
import { warn } from "@/lib/failure";
import { useMachineReport } from "@/features/machine/use-machine-report";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { machineSetupStep, providerId } from "@/lib/tauri/events";
import type { StartAt } from "@/features/machine/use-window-steps";

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
  // Deep link: the step (and the hypervisor it is for) in the URL on open, or a later "Fix"
  // while the window is open.
  const [startAt, setStartAt] = useState<StartAt>(() => {
    const query = typeof window === "undefined" ? null : new URLSearchParams(window.location.search);
    const step = query?.get("step");
    return step ? { step, provider: providerId(query?.get("provider")), nonce: 0 } : null;
  });
  useTauriEvent(machineSetupStep, (at) => {
    setStartAt({ ...at, nonce: Date.now() });
  });
  const close = () => {
    getCurrentWindow().close().catch(warn("closing the window"));
  };

  return (
    <>
      <MachineSetup report={report} checkError={checkError} onRefresh={refreshReport} onClose={close} startAt={startAt} />
      {/* This window has its own root (no app shell), so it mounts its own toasts and dialogs. */}
      <Toaster />
      <DialogPrompts />
    </>
  );
}
