"use client";

import { useRef } from "react";
import { type Provider, type ServerHost } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { StatusPill } from "@/components/ui/status-pill";
import { runOnGroups, useHostChecks, useRunTiers, type OptionGroup, type PerMachineVm } from "@/features/labs/run-on-options";
import { useFocusTrap } from "@/lib/use-focus-trap";
import type { PortMode } from "@/lib/settings";
import { useT } from "@/lib/i18n";

/** Where a lab starts: this machine (Docker; a container lab in one VM on a local hypervisor;
 *  or, for a lab with VM recipes too, one VM per machine there), or a saved host.
 *  `ports` on this machine: chosen in the dialog (container labs), else asked at the start. */
export type RunTarget =
  { kind: "local"; ports?: PortMode } | { kind: "local-vm"; provider: Provider } | { kind: "local-vms"; provider: Provider } | { kind: "host"; id: string };

const targetKey = (t: RunTarget) => {
  switch (t.kind) {
    case "local":
      return t.kind;
    case "local-vm":
      return `vm:${t.provider}`;
    case "local-vms":
      return `vms:${t.provider}`;
    case "host":
      return `host:${t.id}`;
  }
};

/**
 * A centered dialog over the page (Escape or a click on the backdrop closes it). The body
 * scrolls; `footer` (the actions) stays pinned at the bottom.
 */
export function RunOnDialog({ onClose, children, footer }: { onClose: () => void; children: React.ReactNode; footer: React.ReactNode }) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  // Keep keyboard focus inside the dialog and restore it on close; Escape closes it.
  useFocusTrap(ref, true, onClose);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[0.125rem]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("labs.runOn.dialogLabel")}
        className="surface-glass flex max-h-[85vh] w-full max-w-[28rem] flex-col rounded-panel outline-none"
      >
        <div className="min-h-0 flex-1 overflow-y-auto p-5">{children}</div>
        <div className="flex justify-end gap-2 border-t border-border px-5 py-3.5">{footer}</div>
      </div>
    </div>
  );
}

/**
 * "Where should it run?": this machine (Docker, or a VM on the hypervisor chosen in Settings),
 * then the player's servers, then their cloud accounts, each with its logo and live status.
 * Targets the lab can't run on stay visible, disabled, saying why.
 */
export function RunOnPicker({
  title,
  hosts,
  hostOk,
  localNote,
  dockerRunning,
  localVm,
  localVms = [],
  onPickVm = () => {},
  perMachine = null,
  value,
  onChange,
  disabled,
  localExtra,
  offerLocal = true,
}: {
  title: string;
  hosts: ServerHost[];
  hostOk: (h: ServerHost) => boolean;
  localNote: string;
  /** Container labs: whether a Docker engine answers here (null = unknown / not relevant). */
  dockerRunning: boolean | null;
  /** The local hypervisor this (container) lab can run a VM on: Settings' choice, else the first ready. */
  localVm: Provider | null;
  /** Every ready hypervisor it can run a VM on: with several, the VM option carries a switch. */
  localVms?: Provider[];
  /** The switch's pick (saved as Settings' choice by the caller). */
  onPickVm?: (p: Provider) => void;
  /** A lab with VM recipes too: the per-machine VM form here (the hypervisor it runs on, the
   *  others ready for it, the switch); null when the lab or this machine can't. */
  perMachine?: PerMachineVm | null;
  value: RunTarget;
  onChange: (t: RunTarget) => void;
  disabled: boolean;
  /** Shown right under the "This machine" group (e.g. the host-port choice). */
  localExtra?: React.ReactNode;
  /** This machine is a place it can run (false for a cloud lab: cloud accounts only). */
  offerLocal?: boolean;
}) {
  const t = useT();
  const checks = useHostChecks(hosts);
  const tiers = useRunTiers();
  const groups = runOnGroups(t, { hosts, hostOk, checks, localNote, dockerRunning, vm: { localVm, localVms, onPickVm, perMachine }, offerLocal, tiers });
  return (
    <div className="space-y-4">
      <div>
        <p className="serif-title text-[1.5rem] text-foreground">{t("labs.runOn.heading")}</p>
        <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
      </div>
      {groups.flatMap((g) => [
        <OptionList key={g.id} group={g} value={value} onChange={onChange} disabled={disabled} />,
        g.id === "local" && localExtra ? <div key="local-extra">{localExtra}</div> : null,
      ])}
    </div>
  );
}

/** One group of places, as radio rows with their logo and live status. */
function OptionList({ group: g, value, onChange, disabled }: { group: OptionGroup; value: RunTarget; onChange: (t: RunTarget) => void; disabled: boolean }) {
  const t = useT();
  return (
    <div className="space-y-1.5">
      <p className="section-label">{g.label}</p>
      <RadioList label={g.label}>
        {g.options.map((o) => (
          <RadioRow
            key={targetKey(o.target)}
            selected={targetKey(value) === targetKey(o.target)}
            onSelect={() => {
              onChange(o.target);
            }}
            disabled={disabled || !o.ok}
            hint={o.ok ? o.status?.title : o.why}
            title={o.title}
            subtitle={o.ok ? o.subtitle : t("labs.runOn.notSupported")}
            leading={o.logo}
            trailing={
              (o.badge || o.status) && (
                <span className="flex items-center gap-1.5">
                  {o.badge && (
                    <span title={o.badge.title}>
                      <StatusPill tone={o.badge.kind === "experimental" ? "warn" : "muted"}>
                        <span className="whitespace-nowrap">{t(o.badge.kind === "experimental" ? "labs.runOn.experimental" : "labs.runOn.beta")}</span>
                      </StatusPill>
                    </span>
                  )}
                  {o.status && (
                    <StatusPill tone={o.status.tone}>
                      <span className="whitespace-nowrap">{o.status.label}</span>
                    </StatusPill>
                  )}
                </span>
              )
            }
          >
            {/* A switch inside the row picks within the option, never another option. */}
            {o.extra && (
              <div
                onClick={(e) => {
                  e.stopPropagation();
                }}
              >
                {o.extra}
              </div>
            )}
          </RadioRow>
        ))}
      </RadioList>
    </div>
  );
}
