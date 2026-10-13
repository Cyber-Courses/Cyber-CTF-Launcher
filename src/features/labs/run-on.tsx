"use client";

import { useEffect, useRef } from "react";
import { type Provider, type ServerHost } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { StatusPill } from "@/components/ui/status-pill";
import { runOnGroups, useHostChecks, type OptionGroup } from "@/features/labs/run-on-options";
import { useFocusTrap } from "@/lib/use-focus-trap";
import type { PortMode } from "@/lib/settings";
import { useT } from "@/lib/i18n";

/** Where a lab starts: this machine (Docker, or a VM on a local hypervisor), a saved host, or
 *  hosted by Cyber CTF. `ports` on this machine: chosen in the dialog (container labs), else
 *  asked at the start. */
export type RunTarget = { kind: "local"; ports?: PortMode } | { kind: "local-vm"; provider: Provider } | { kind: "host"; id: string } | { kind: "hosted" };

const targetKey = (t: RunTarget) => (t.kind === "local" || t.kind === "hosted" ? t.kind : t.kind === "local-vm" ? `vm:${t.provider}` : `host:${t.id}`);

/**
 * A centered dialog over the page (Escape or a click on the backdrop closes it). The body
 * scrolls; `footer` (the actions) stays pinned at the bottom.
 */
export function RunOnDialog({ onClose, children, footer }: { onClose: () => void; children: React.ReactNode; footer: React.ReactNode }) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    // Escape is the dialog's alone: stopped here, it doesn't also reach the lab page's own
    // Escape (which goes back to the list) once the dialog has closed and focus left it.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);
  // Keep keyboard focus inside the dialog and restore it on close.
  useFocusTrap(ref);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[0.125rem]"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
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
  hosted,
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
  /** Cyber CTF can run this lab for the player (no install, a public URL). */
  hosted: boolean;
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
  const groups = runOnGroups(t, { hosts, hostOk, checks, localNote, dockerRunning, localVm, hosted, offerLocal });
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
            onSelect={() => onChange(o.target)}
            disabled={disabled || !o.ok}
            hint={o.ok ? o.status?.title : o.why}
            title={o.title}
            subtitle={o.ok ? o.subtitle : t("labs.runOn.notSupported")}
            leading={o.logo}
            trailing={
              o.status && (
                <StatusPill tone={o.status.tone}>
                  <span className="whitespace-nowrap">{o.status.label}</span>
                </StatusPill>
              )
            }
          />
        ))}
      </RadioList>
    </div>
  );
}
