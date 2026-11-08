"use client";

import { useRef } from "react";
import { type Provider, type ServerHost } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { StatusPill } from "@/components/ui/status-pill";
import { platformGroups, vmFormOptions, useHostChecks, useRunTiers, type LocalRows, type OptionGroup, type PerMachineVm } from "@/features/labs/run-on-options";
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

/** The two steps of the picker: first the platform ("Where should it run?"), then, only when This
 *  machine's In a VM row has two VM forms, how it runs there ("How should it run?"). */
export type PickerStep = "platform" | "form";

/** A row a lab can be launched from in step 1: run here as containers, run here in a VM, or on one
 *  saved host. (A container lab's local form is `containers`; a VM lab's is `vm`.) */
export type Platform = { kind: "containers" } | { kind: "vm" } | { kind: "host"; id: string };

/** The platform a step-1 row's target stands for. A bare `local` is the Containers row for a
 *  container lab, the In a VM row (its own VMs) for a VM lab; `local-vm`/`local-vms` are In a VM. */
const platformOfRow = (t: RunTarget, offerContainers: boolean): Platform => {
  if (t.kind === "host") return { kind: "host", id: t.id };
  if (t.kind === "local") return offerContainers ? { kind: "containers" } : { kind: "vm" };
  return { kind: "vm" };
};

/** A target collapsed to its step-1 row's representative, so the right row highlights whichever
 *  local form (`local`, `local-vm`, `local-vms`) is the current pick. A container lab's VM forms
 *  all map to the In a VM row (`local-vm` on the chosen hypervisor). */
const asRowTarget = (t: RunTarget, localVm: Provider | null): RunTarget => {
  if (t.kind === "host" || t.kind === "local") return t;
  return localVm ? { kind: "local-vm", provider: localVm } : t;
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
 * The launch picker, in two steps. Step 1 ("Where should it run?") lists the places: This machine
 * as up to two rows (Containers, In a VM), then the player's servers, then their cloud accounts,
 * each with its logo and live status; a place the lab can't run on stays visible, disabled, saying
 * why. Containers and a lone VM form go straight to Start; an In a VM row with two VM forms opens
 * step 2 ("How should it run?"). `step`/`onPickPlatform` are driven by the caller (the hook).
 */
type RunOnPickerProps = {
  title: string;
  hosts: ServerHost[];
  hostOk: (h: ServerHost) => boolean;
  localNote: string;
  /** Container labs: whether a Docker engine answers here (null = unknown / not relevant). */
  dockerRunning: boolean | null;
  /** Whether the lab has a container form that runs here, so This machine shows a Containers row
   *  (a VM lab shows only In a VM). */
  offerContainers?: boolean;
  /** The local hypervisor this (container) lab can run a VM on: Settings' choice, else the first ready. */
  localVm: Provider | null;
  /** Every ready hypervisor it can run a VM on: with several, the VM form carries a switch. */
  localVms?: Provider[];
  /** The switch's pick (saved as Settings' choice by the caller). */
  onPickVm?: (p: Provider) => void;
  /** A lab with VM recipes too: the per-machine VM form here (the hypervisor it runs on, the
   *  others ready for it, the switch); null when the lab or this machine can't. */
  perMachine?: PerMachineVm | null;
  value: RunTarget;
  /** Picks a VM form in step 2 (the switches use `onPickVm`). */
  onChange: (t: RunTarget) => void;
  disabled: boolean;
  /** The host-port choice for a container lab here: shown under the Containers row. */
  localExtra?: React.ReactNode;
  /** This machine is a place it can run (false for a cloud lab: cloud accounts only). */
  offerLocal?: boolean;
  /** Which step to show. */
  step?: PickerStep;
  /** Picks a place in step 1. */
  onPickPlatform?: (p: Platform) => void;
};

/** A step's title ("Where should it run?" / "How should it run?") over the lab's name. */
function PickerHeading({ heading, title }: { heading: string; title: string }) {
  return (
    <div>
      <p className="serif-title text-[1.5rem] text-foreground">{heading}</p>
      <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
    </div>
  );
}

export function RunOnPicker(props: RunOnPickerProps) {
  const { title, hosts, hostOk, localNote, dockerRunning, value, disabled, localExtra, offerLocal = true, offerContainers = true, step = "platform" } = props;
  const { localVm, localVms = [], onPickVm = () => {}, perMachine = null, onChange, onPickPlatform = () => {} } = props;
  const t = useT();
  const checks = useHostChecks(hosts);
  const tiers = useRunTiers();

  if (step === "form") {
    const forms = vmFormOptions(t, tiers, { localVm, localVms, onPickVm, perMachine });
    return (
      <div className="space-y-4">
        <PickerHeading heading={t("labs.runOn.formHeading")} title={title} />
        <OptionList group={{ id: "local", label: t("labs.runOn.thisMachine"), options: forms }} value={value} onChange={onChange} disabled={disabled} />
      </div>
    );
  }

  const local: LocalRows = { offerContainers, localNote, dockerRunning, localVm, localVms, onPickVm, perMachine };
  const groups = platformGroups(t, { hosts, hostOk, checks, local, offerLocal, tiers });
  const pick = (target: RunTarget) => {
    onPickPlatform(platformOfRow(target, offerContainers));
  };
  return (
    <div className="space-y-4">
      <PickerHeading heading={t("labs.runOn.heading")} title={title} />
      {groups.flatMap((g) => [
        <OptionList key={g.id} group={g} value={asRowTarget(value, localVm)} onChange={pick} disabled={disabled} />,
        // The Containers row is a leaf: its host ports sit with This machine here.
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
