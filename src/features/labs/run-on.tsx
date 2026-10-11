"use client";

import { useEffect, useRef, useState } from "react";
import { serverTest, type Provider, type ServerHost, type ServerTest } from "@/lib/tauri";
import { RadioList, RadioRow } from "@/components/ui/radio-row";
import { StatusPill, type Tone } from "@/components/ui/status-pill";
import { TypeIcon } from "@/components/ui/type-icon";
import { KIND } from "@/features/servers/host-setup/constants";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { CLOUDS } from "@/features/labs/lab-runtime";
import { useFocusTrap } from "@/lib/use-focus-trap";
import type { PortMode } from "@/lib/settings";
import { useT, type T } from "@/lib/i18n";

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
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
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

type Status = { tone: Tone; label: string; title?: string };
type Option = { target: RunTarget; title: string; subtitle: string; ok: boolean; why?: string; logo: React.ReactNode; status?: Status };

/** Each host's connection test, run once when the picker opens (like "Test" on the Servers page). */
function useHostChecks(hosts: ServerHost[]) {
  const [checks, setChecks] = useState<Record<string, ServerTest | "testing">>({});
  const ids = hosts.map((h) => h.id).join(",");
  useEffect(() => {
    let live = true;
    const list = ids ? ids.split(",") : [];
    // Marks every host "testing" before the async checks settle (one batch, not a cascade).
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setChecks(Object.fromEntries(list.map((id) => [id, "testing" as const])));
    for (const id of list) {
      serverTest(id)
        .catch((e): ServerTest => ({ ok: false, reachable: false, authenticated: null, latencyMs: null, message: String(e), checks: [] }))
        .then((r) => live && setChecks((c) => ({ ...c, [id]: r })));
    }
    return () => {
      live = false;
    };
  }, [ids]);
  return checks;
}

function hostStatus(t: T, check: ServerTest | "testing" | undefined): Status {
  if (!check || check === "testing") return { tone: "muted", label: t("labs.runOn.checking") };
  if (check.ok) return { tone: "ok", label: t("labs.runOn.online"), title: check.message };
  return {
    tone: check.reachable ? "warn" : "fail",
    label: check.reachable ? t("labs.runOn.needsAttention") : t("labs.runOn.unreachable"),
    title: check.message,
  };
}

const CyberCtfLogo = () => (
  <TypeIcon>
    {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
    <img src="/logo-mark.svg" alt="Cyber CTF" className="size-4 object-contain" draggable={false} />
  </TypeIcon>
);

const DockerLogo = () => (
  <TypeIcon>
    {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
    <img src="/brands/docker.svg" alt="Docker" className="size-5 object-contain" draggable={false} />
  </TypeIcon>
);

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
  const hostOption = (h: ServerHost): Option => {
    const kind = KIND[h.provider]?.label ?? h.provider;
    const cloud = CLOUDS.has(h.provider);
    const ok = hostOk(h);
    return {
      target: { kind: "host", id: h.id },
      title: h.name,
      subtitle: t(cloud ? "labs.runOn.hostSubtitleCloud" : "labs.runOn.hostSubtitle", { kind, host: h.host }),
      ok,
      why: t("labs.runOn.unsupportedKind", { kind }),
      logo: <ProviderGlyph provider={h.provider} />,
      status: ok ? hostStatus(t, checks[h.id]) : undefined,
    };
  };
  const local: Option[] = [
    {
      target: { kind: "local" },
      title: t("labs.runOn.thisMachine"),
      subtitle: localNote,
      ok: true,
      logo: dockerRunning === null ? <HypervisorLogo provider={null} /> : <DockerLogo />,
      status:
        dockerRunning === null
          ? undefined
          : dockerRunning
            ? { tone: "ok", label: t("labs.runOn.running") }
            : { tone: "warn", label: t("labs.runOn.engineStopped"), title: t("labs.runOn.engineStoppedHint") },
    },
  ];
  if (localVm) {
    local.push({
      target: { kind: "local-vm", provider: localVm },
      title: t("labs.runOn.inVm"),
      subtitle: t("labs.runOn.inVmSubtitle", { provider: PROVIDER_LABELS[localVm] ?? localVm }),
      ok: true,
      logo: <HypervisorLogo provider={localVm} />,
      status: { tone: "ok", label: t("labs.runOn.ready"), title: t("labs.runOn.readyHint") },
    });
  }
  const groups: { id: string; label: string; options: Option[] }[] = [
    { id: "local", label: t("labs.runOn.thisMachine"), options: offerLocal ? local : [] },
    {
      id: "hosted",
      label: "Cyber CTF",
      options: hosted
        ? [
            {
              target: { kind: "hosted" },
              title: t("labs.runOn.hostedTitle"),
              subtitle: t("labs.runOn.hostedSubtitle"),
              ok: true,
              logo: <CyberCtfLogo />,
              status: { tone: "ok", label: t("labs.runOn.available") },
            } satisfies Option,
          ]
        : [],
    },
    { id: "servers", label: t("labs.runOn.servers"), options: hosts.filter((h) => !CLOUDS.has(h.provider)).map(hostOption) },
    { id: "cloud", label: t("labs.runOn.cloud"), options: hosts.filter((h) => CLOUDS.has(h.provider)).map(hostOption) },
  ].filter((g) => g.options.length > 0);

  return (
    <div className="space-y-4">
      <div>
        <p className="serif-title text-[1.5rem] text-foreground">{t("labs.runOn.heading")}</p>
        <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
      </div>
      {groups.flatMap((g) => [
        <div key={g.id} className="space-y-1.5">
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
        </div>,
        g.id === "local" && localExtra ? <div key="local-extra">{localExtra}</div> : null,
      ])}
    </div>
  );
}
