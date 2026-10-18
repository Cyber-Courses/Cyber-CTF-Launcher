"use client";

import type { ReactNode } from "react";
import { Check, Copy } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { LogConsole } from "@/components/ui/log-console";
import { StatusDot } from "@/components/ui/status-pill";
import { StepCircle } from "@/features/machine/step-row";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { useCopy } from "@/components/ui/copy-value";
import { useT } from "@/lib/i18n";

export function Requirement({
  ok,
  title,
  detail,
  action,
  optional,
}: {
  ok: boolean;
  title: string;
  detail: string;
  action: React.ReactNode;
  optional?: boolean;
}) {
  return (
    <div className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2.5 first:border-t-0">
      {ok || optional ? (
        <StepCircle state={ok ? "done" : "pending"} />
      ) : (
        <span className="grid size-[1.1rem] shrink-0 place-items-center">
          <StatusDot tone="warn" />
        </span>
      )}
      <span className="min-w-0 flex-1 text-left">
        <span className="block text-[0.8125rem] text-foreground">{title}</span>
        <span className="block font-mono text-[0.6875rem] break-words text-faint">{detail}</span>
      </span>
      {!ok && <span className="shrink-0">{action}</span>}
    </div>
  );
}

// ---------- Small parts ----------

/** The install output of the current step, in a log well with a live timer. */
/** The install log, when it is about one of `of` (this step's installs): a failed VirtualBox
 *  install isn't shown under QEMU once the player picks that instead, or on the next step. */
export function Log({ setup, of }: { setup: MachineSetupState; of: (string | null | undefined)[] }) {
  const t = useT();
  if (!setup.logsFor || !of.includes(setup.logsFor)) return null;
  return <LogConsole lines={setup.logs} running={setup.installing !== null} title={t("machine.body.installLog")} />;
}

export function CmdRow({ cmd }: { cmd: string }) {
  const t = useT();
  const { copied, copy } = useCopy(cmd, { showFor: 1200, failure: t("machine.errors.copy") });
  return (
    <div className="surface-log flex items-center gap-2 rounded-control px-3 py-2">
      <code className="flex-1 overflow-x-auto text-left font-mono text-[0.75rem] text-foreground">{cmd}</code>
      <button onClick={copy} className="inline-flex shrink-0 items-center gap-1 text-[0.6875rem] text-muted-foreground transition-colors hover:text-foreground">
        {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />} {copied ? t("machine.body.copied") : t("machine.body.copy")}
      </button>
    </div>
  );
}

export function Num({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex gap-3 text-left">
      <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-glass-2 font-mono text-[0.625rem] text-muted-foreground shadow-[inset_0_0_0_1px_var(--input)]">
        {n}
      </span>
      <div className="min-w-0 text-[0.8125rem] leading-relaxed text-foreground">{children}</div>
    </li>
  );
}

export function Outcome({ title, ok, detail }: { title: string; ok: boolean; detail: string }) {
  return (
    <div className="flex min-h-[3.25rem] items-center gap-3 border-t border-border px-4 py-2.5 text-left first:border-t-0">
      <StepCircle state={ok ? "done" : "pending"} />
      <div className="min-w-0">
        <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
        <p className="text-[0.75rem] text-muted-foreground">{detail}</p>
      </div>
    </div>
  );
}

export function Skipped({ title, reason }: { title: string; reason: string }) {
  return (
    <div className="flex items-start gap-3 rounded-control bg-glass px-4 py-3 text-left shadow-[inset_0_0_0_1px_var(--border)]">
      <span className="mt-1">
        <StatusDot tone="muted" />
      </span>
      <div className="min-w-0">
        <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
        <p className="text-[0.75rem] text-muted-foreground">{reason}</p>
      </div>
    </div>
  );
}

/** A button's label, or a spinner and `label` while its action runs. */
export function BusyLabel({ busy, label, spinner = "size-3.5", children }: { busy: boolean; label: string; spinner?: string; children: ReactNode }) {
  return busy ? (
    <>
      <Spinner className={spinner} /> {label}
    </>
  ) : (
    children
  );
}
