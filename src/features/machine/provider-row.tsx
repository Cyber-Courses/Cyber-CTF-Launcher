"use client";

import { type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { StatusDot, type Tone } from "@/components/ui/status-pill";
import { Tip } from "@/components/ui/tip";
import { MachineSelfTest, type LabKind } from "@/features/machine/machine-parts";
import type { SelfTestTarget } from "@/features/machine/self-test";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

// The Machine page's providers (every container engine and hypervisor installed here) as a list
// of rows on shared columns, each row saying where its provider stands in the same words.

/** Where a provider (a container engine, a hypervisor) stands, the same words for every row,
 *  the default or not (which one labs use is the Default mark beside it):
 *  - ready: labs can run on it now (an engine: its daemon answers and this account may use it; a
 *    hypervisor: Vagrant and its plugin are in place);
 *  - testFailed: ready, but its last self-test failed;
 *  - noPermission: an engine whose daemon runs but refuses this account;
 *  - stopped: an engine installed but not running;
 *  - needsSetup: something it needs isn't installed yet (Fix sets it up);
 *  - cantRunHere: a hypervisor this machine can't run, whatever is installed (the detail says why). */
export type RowStatus = "ready" | "testFailed" | "noPermission" | "stopped" | "needsSetup" | "cantRunHere";

const TONE: Record<RowStatus, Tone> = {
  ready: "ok",
  testFailed: "fail",
  noPermission: "warn",
  stopped: "warn",
  needsSetup: "warn",
  cantRunHere: "warn",
};

/** A status's tone: a stopped engine labs don't use is only idle, not a warning. */
export const statusTone = (status: RowStatus, isDefault: boolean): Tone => (status === "stopped" && !isDefault ? "muted" : TONE[status]);

const STATUS_TEXT: Record<Tone, string> = {
  ok: "text-foreground",
  warn: "text-warning",
  fail: "text-destructive",
  muted: "text-muted-foreground",
};

/** The providers' rows, on one grid: logo, name and detail, status, the default slot and the
 *  action, each a column every row shares (CSS subgrid), so a column sits at the same place in
 *  every row and is as wide as its widest cell, whatever each row shows in it. */
export function ProviderList({ children }: { children: ReactNode }) {
  const t = useT();
  return (
    <div role="list" aria-label={t("machine.screen.providers")} className="grid grid-cols-[auto_minmax(0,1fr)_auto_auto_auto]">
      {children}
    </div>
  );
}

/** The Providers panel around the list. Not clipped: a status's tooltip on the last row reaches
 *  past the panel's edge. */
export function ProvidersPanel({ children }: { children: ReactNode }) {
  const t = useT();
  return (
    <Panel className="overflow-visible!">
      <PanelHeader title={t("machine.screen.providers")} meta={t("machine.screen.providersMeta")} />
      <ProviderList>{children}</ProviderList>
    </Panel>
  );
}

/** The status column's cell: dot and word, with what the word means as a tooltip (on hover, and
 *  on keyboard focus). */
function StatusCell({ kind, status, tone }: { kind: LabKind; status: RowStatus; tone: Tone }) {
  const t = useT();
  const hint = status === "ready" ? t(`machine.screen.statusHint.ready.${kind}`) : t(`machine.screen.statusHint.${status}`);
  return (
    <Tip text={hint}>
      <span
        // Focusable so a keyboard reaches its tooltip too.
        tabIndex={0}
        data-slot="status"
        className={cn("inline-flex items-center gap-2 rounded-sm text-[0.75rem] whitespace-nowrap", STATUS_TEXT[tone])}
      >
        <StatusDot tone={tone} />
        {t(`machine.screen.status.${status}`)}
      </span>
    </Tip>
  );
}

/** The default slot: the Default mark on the provider labs use, "Use by default" on one that
 *  could be, nothing on the rest; the column keeps its width either way. */
function DefaultCell({ kind, isDefault, onUseDefault }: { kind: LabKind; isDefault: boolean; onUseDefault?: () => void }) {
  const t = useT();
  if (isDefault)
    return (
      <Tip text={t(`machine.screen.defaultHint.${kind}`)}>
        <span
          tabIndex={0}
          data-slot="mark"
          className="rounded-full px-2 py-px font-mono text-[0.625rem] whitespace-nowrap text-jewel-text shadow-[inset_0_0_0_1px_rgb(var(--jewel-rgb)/0.4)]"
        >
          {t("machine.screen.defaultMark")}
        </span>
      </Tip>
    );
  if (!onUseDefault) return null;
  return (
    <Button variant="ghost" size="xs" className="whitespace-nowrap" onClick={onUseDefault}>
      {t("machine.screen.useDefault")}
    </Button>
  );
}

type RowCells = {
  kind: LabKind;
  icon: ReactNode;
  title: string;
  status: RowStatus;
  /** The provider labs of this kind use: the Default mark. */
  isDefault?: boolean;
  /** Makes it the default: "Use by default" (on a ready provider that isn't). */
  onUseDefault?: () => void;
  detail: ReactNode;
  hint?: ReactNode;
  actions: ReactNode;
};

/** A row's five cells, on the list's columns: logo, name over detail (and hint), status, default
 *  slot, action. Every cell is there, filled or not. */
function RowLine({ kind, icon, title, status, isDefault = false, onUseDefault, detail, hint, actions }: RowCells) {
  return (
    <div className="col-span-full grid grid-cols-subgrid items-center gap-x-3.5 px-4 py-3">
      {icon}
      <div className="min-w-0">
        <p data-slot="name" className="text-[0.8125rem] font-medium break-words text-foreground">
          {title}
        </p>
        <p data-slot="detail" className="mt-0.5 font-mono text-[0.6875rem] text-faint">
          {detail}
        </p>
        {hint && (
          <div data-slot="hint" className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[0.75rem] text-warning">
            {hint}
          </div>
        )}
      </div>
      <div className="justify-self-start">
        <StatusCell kind={kind} status={status} tone={statusTone(status, isDefault)} />
      </div>
      <div data-slot="default" className="flex justify-center">
        <DefaultCell kind={kind} isDefault={isDefault} onUseDefault={onUseDefault} />
      </div>
      <div data-slot="actions" className="flex items-center justify-end gap-2">
        {actions}
      </div>
    </div>
  );
}

/** One provider (a container engine, a hypervisor) as a row of the providers' list: its cells,
 *  and its self-test inline below them when open. */
export function ProviderRow({
  testing,
  onTestDone,
  target,
  ...cells
}: RowCells & {
  testing: boolean;
  onTestDone: () => void;
  /** Which engine or hypervisor the inline test is of (this row's), and how its result is kept. */
  target?: SelfTestTarget;
}) {
  return (
    <div role="listitem" className="col-span-full grid grid-cols-subgrid border-t border-border first:border-t-0">
      <RowLine {...cells} />
      {testing && (
        <div className="col-span-full px-4 pb-4">
          <MachineSelfTest
            kind={cells.kind}
            target={target}
            onResult={(r) => {
              if (r === "ok" || r === "fail") onTestDone();
            }}
          />
        </div>
      )}
    </div>
  );
}
