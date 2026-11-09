"use client";

import { type DockerEngine, type LabRunOptions, type Provider, type ServerHost } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { RowPill, hypervisorOptions, platformGroups, vmFormOptions, useHostChecks, type LocalRows, type OptionGroup } from "@/features/labs/run-on-options";
import type { PortMode } from "@/lib/settings";
import { useT } from "@/lib/i18n";
import { EngineStep, PickerHeading } from "@/features/labs/run-on-steps";

/** Where a lab starts: this machine (Docker; a container lab in one VM on a local hypervisor;
 *  or, for a lab with VM recipes too, one VM per machine there), or a saved host.
 *  `ports` on this machine: chosen in the dialog (container labs), else asked at the start. */
export type RunTarget =
  | { kind: "local"; ports?: PortMode; engine?: DockerEngine }
  | { kind: "local-vm"; provider: Provider }
  | { kind: "local-vms"; provider: Provider }
  | { kind: "host"; id: string; perMachine?: boolean };

const targetKey = (t: RunTarget) => {
  switch (t.kind) {
    case "local":
      return t.kind;
    case "local-vm":
      return `vm:${t.provider}`;
    case "local-vms":
      return `vms:${t.provider}`;
    // `perMachine` (a cloud account's one-VM-per-machine form, cloud-vm) is its own row on the
    // form step; on the places step both forms collapse to the one account row (`asRowTarget`).
    case "host":
      return `host:${t.id}${t.perMachine ? ":vms" : ""}`;
  }
};

/** The picker's steps: the places ("Where should it run?"), then — only when there is a choice —
 *  the VM form, its hypervisor, or the container engine and host ports. */
export type PickerStep = "platform" | "form" | "hypervisor" | "engine" | "ports";

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
 *  all map to the one VM row (its first form). */
const asRowTarget = (t: RunTarget, firstVmForm: RunTarget | undefined): RunTarget => {
  // A cloud form (`perMachine`) still highlights its account's one row on the places step.
  if (t.kind === "host") return { kind: "host", id: t.id };
  if (t.kind === "local") return t;
  return firstVmForm ?? t;
};

export { RunOnDialog } from "@/features/labs/run-on-dialog";

/**
 * The launch picker, step by step. The places ("Where should it run?": This machine as up to two
 * rows — Container, VM — then servers, then cloud accounts, each with its logo and live status);
 * then, only when there is a choice, the VM form ("How should it run?"), its hypervisor ("Which
 * hypervisor?"), or the container engine ("Which engine?"). The host-ports step is rendered by the
 * caller. Each row is a full row — no inline switches. `step` and the `onPick…` handlers are driven
 * by the hook.
 */
type RunOnPickerProps = {
  title: string;
  step?: PickerStep;
  value: RunTarget;
  disabled: boolean;
  // The places (step 1).
  hosts: ServerHost[];
  hostOk: (h: ServerHost) => boolean;
  /** This machine is a place it can run (false for a cloud lab: cloud accounts only). */
  offerLocal?: boolean;
  /** Whether the lab has a container form that runs here (a VM lab shows only the VM row). */
  offerContainers?: boolean;
  localNote: string;
  /** Container labs: whether a Docker engine answers here (null = unknown / not relevant). */
  dockerRunning: boolean | null;
  /** The container engine the lab will use here, named on the Container row and preselected in the
   *  engine step (null: none / a VM lab). */
  engine?: DockerEngine | null;
  /** The VM forms This machine offers, for the "In a VM" row on the places step. */
  vmForms?: RunTarget[];
  onPickPlatform?: (p: Platform) => void;
  // The form step ("How should it run?"): this machine's VM forms, or a cloud account's forms
  // (one VM / one VM per machine) when `formAccount` is set.
  /** The forms to choose between on the form step. */
  formTargets?: RunTarget[];
  /** The cloud account the form step's rows belong to (their logo and badge), when they are a
   *  cloud account's forms rather than this machine's. */
  formAccount?: ServerHost | null;
  onPickForm?: (t: RunTarget) => void;
  // The hypervisor step.
  hypervisors?: Provider[];
  defaultHypervisor?: Provider | null;
  onPickHypervisor?: (p: Provider) => void;
  /** The lab's run options (each place's launch path and its badge); null while they load. */
  runOptions?: LabRunOptions | null;
  // The engine step.
  engines?: DockerEngine[];
  defaultEngine?: DockerEngine | null;
  onPickEngine?: (e: DockerEngine) => void;
};

export function RunOnPicker(props: RunOnPickerProps) {
  const { title, step = "platform", value, disabled } = props;
  const { hosts, hostOk, offerLocal = true, offerContainers = true, localNote, dockerRunning, engine = null, vmForms = [], onPickPlatform = () => {} } = props;
  const { formTargets = vmForms, formAccount = null, onPickForm = () => {}, hypervisors = [], defaultHypervisor = null, onPickHypervisor = () => {} } = props;
  const { engines = [], defaultEngine = null, onPickEngine = () => {} } = props;
  const t = useT();
  const checks = useHostChecks(hosts);
  const options = props.runOptions ?? null;

  if (step === "form") {
    // A cloud account's forms sit under its name; this machine's under "This machine".
    const label = formAccount ? formAccount.name : t("labs.runOn.thisMachine");
    return (
      <div className="space-y-4">
        <PickerHeading heading={t("labs.runOn.formHeading")} title={title} />
        <OptionList
          group={{ id: "form", label, options: vmFormOptions(t, options, formTargets, formAccount) }}
          value={value}
          onChange={onPickForm}
          disabled={disabled}
        />
      </div>
    );
  }
  if (step === "hypervisor") {
    const form = value.kind === "local-vms" ? "local-vms" : "local-vm";
    const pickHypervisor = (target: RunTarget) => {
      if (target.kind === "local-vm" || target.kind === "local-vms") onPickHypervisor(target.provider);
    };
    return (
      <div className="space-y-4">
        <PickerHeading heading={t("labs.runOn.pickHypervisor")} title={title} />
        <OptionList
          group={{ id: "hyp", label: t("labs.runOn.thisMachine"), options: hypervisorOptions(t, form, hypervisors, defaultHypervisor) }}
          value={value}
          onChange={pickHypervisor}
          disabled={disabled}
        />
      </div>
    );
  }
  if (step === "engine") {
    return <EngineStep title={title} engines={engines} selected={engine} fallback={defaultEngine} onPick={onPickEngine} disabled={disabled} />;
  }

  const local: LocalRows = { offerContainers, localNote, dockerRunning, engine, vmForms };
  const groups = platformGroups(t, { hosts, hostOk, checks, local, offerLocal, options });
  const pick = (target: RunTarget) => {
    onPickPlatform(platformOfRow(target, offerContainers));
  };
  return (
    <div className="space-y-4">
      <PickerHeading heading={t("labs.runOn.heading")} title={title} />
      {groups.map((g) => (
        <OptionList key={g.id} group={g} value={asRowTarget(value, vmForms.at(0))} onChange={pick} disabled={disabled} />
      ))}
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
                    <RowPill tone={o.badge.kind === "experimental" ? "warn" : "muted"} title={o.badge.title}>
                      {t(o.badge.kind === "experimental" ? "labs.runOn.experimental" : "labs.runOn.beta")}
                    </RowPill>
                  )}
                  {o.status && (
                    <RowPill tone={o.status.tone} title={o.status.title}>
                      {o.status.label}
                    </RowPill>
                  )}
                </span>
              )
            }
          />
        ))}
      </RadioList>
    </div>
  );
}
