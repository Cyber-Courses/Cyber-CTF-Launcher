"use client";

import { useEffect, useState } from "react";
import { type Provider, type RunTier, type ServerHost, type ServerTest } from "@/lib/tauri";
import { badgeFor, candidatesFor, type RunBadge } from "@/features/labs/run-on-badges";
import { testHost } from "@/features/servers/use-server-list";
import { type Tone } from "@/components/ui/status-pill";
import { TypeIcon } from "@/components/ui/type-icon";
import { KIND } from "@/features/servers/host-setup/constants";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { Segmented } from "@/components/ui/segmented";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { CLOUDS } from "@/features/labs/lab-runtime";
import type { RunTarget } from "@/features/labs/run-on";
import { type T } from "@/lib/i18n";

export { useRunTiers } from "@/features/labs/run-on-badges";

// The places "Where should it run?" offers, by group: this machine, Cyber CTF, servers, clouds.

type Status = { tone: Tone; label: string; title?: string };
type Option = {
  target: RunTarget;
  title: string;
  subtitle: string;
  ok: boolean;
  why?: string;
  logo: React.ReactNode;
  status?: Status;
  badge?: RunBadge;
  extra?: React.ReactNode;
};
export type OptionGroup = { id: string; label: string; options: Option[] };
/** Each host's test result, by id; none yet while it runs. */
type HostChecks = Partial<Record<string, ServerTest>>;

/** Each host's connection test, run once when the picker opens (like "Test" on the Servers page).
 *  A host without an answer yet is being tested. The answers are kept with the host list they
 *  were asked for, so a new list starts over without a reset of its own. */
export function useHostChecks(hosts: ServerHost[]): HostChecks {
  const [answers, setAnswers] = useState<{ ids: string; checks: HostChecks }>({ ids: "", checks: {} });
  const ids = hosts.map((h) => h.id).join(",");
  useEffect(() => {
    let live = true;
    for (const id of ids ? ids.split(",") : []) {
      void testHost(id).then((r) => {
        if (live) setAnswers((a) => ({ ids, checks: { ...(a.ids === ids ? a.checks : {}), [id]: r } }));
      });
    }
    return () => {
      live = false;
    };
  }, [ids]);
  return answers.ids === ids ? answers.checks : {};
}

function hostStatus(t: T, check: ServerTest | undefined): Status {
  if (!check) return { tone: "muted", label: t("labs.runOn.checking") };
  if (check.ok) return { tone: "ok", label: t("labs.runOn.online"), title: check.message };
  return {
    tone: check.reachable ? "warn" : "fail",
    label: check.reachable ? t("labs.runOn.needsAttention") : t("labs.runOn.unreachable"),
    title: check.message,
  };
}

const DockerLogo = () => (
  <TypeIcon>
    {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
    <img src="/brands/docker.svg" alt="Docker" className="size-5 object-contain" draggable={false} />
  </TypeIcon>
);

/** The hypervisor switch inside a VM option, when several are ready for it. */
function hypervisorSwitch(t: T, value: Provider, options: Provider[], onChange: (p: Provider) => void) {
  return (
    options.length > 1 && (
      <Segmented
        label={t("labs.runOn.pickHypervisor")}
        className="mt-2"
        value={value}
        onChange={onChange}
        options={options.map((p) => ({ value: p, label: PROVIDER_LABELS[p] ?? p }))}
      />
    )
  );
}

/** This machine (with its Docker engine's state, for a container lab), a VM on `localVm`, with
 *  a switch to the other ready hypervisors (`localVms`) when there are several, and for a lab
 *  with VM recipes too, one VM per machine (`perMachine`). */
function localOptions(t: T, tiers: RunTier[], localNote: string, dockerRunning: boolean | null, vm: LocalVm): Option[] {
  const { localVm, localVms, onPickVm, perMachine } = vm;
  const local: Option[] = [
    {
      target: { kind: "local" },
      title: t("labs.runOn.thisMachine"),
      subtitle: localNote,
      ok: true,
      badge: badgeFor(tiers, candidatesFor({ kind: "local" })),
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
      badge: badgeFor(tiers, candidatesFor({ kind: "local-vm", provider: localVm })),
      logo: <HypervisorLogo provider={localVm} />,
      status: { tone: "ok", label: t("labs.runOn.ready"), title: t("labs.runOn.readyHint") },
      extra: hypervisorSwitch(t, localVm, localVms, onPickVm),
    });
  }
  if (perMachine?.provider) {
    const { provider, providers, onPick } = perMachine;
    local.push({
      target: { kind: "local-vms", provider },
      title: t("labs.runOn.perMachineVm"),
      subtitle: t("labs.runOn.perMachineVmSubtitle", { provider: PROVIDER_LABELS[provider] ?? provider }),
      ok: true,
      badge: badgeFor(tiers, candidatesFor({ kind: "local-vms", provider })),
      logo: <HypervisorLogo provider={provider} />,
      status: { tone: "ok", label: t("labs.runOn.ready"), title: t("labs.runOn.perMachineVmHint") },
      extra: hypervisorSwitch(t, provider, providers, onPick),
    });
  }
  return local;
}

/** The hypervisor a (container) lab's VM runs on here, the others ready for it, and the switch;
 *  and, for a lab with VM recipes too, its per-machine VM form (`perMachine`). */
export type LocalVm = { localVm: Provider | null; localVms: Provider[]; onPickVm: (p: Provider) => void; perMachine?: PerMachineVm | null };

/** The per-machine VM form of a lab with VM recipes too (Isoloom's vagrant target, as a VM lab
 *  runs): the hypervisor it runs on here (null: none ready that can), the others that can, and
 *  the switch's pick. */
export type PerMachineVm = { provider: Provider | null; providers: Provider[]; onPick: (p: Provider) => void };

/** A saved server or cloud account; one the lab can't run on says why, without a status. */
function hostOption(t: T, tiers: RunTier[], h: ServerHost, ok: boolean, check: ServerTest | undefined): Option {
  const kind = KIND[h.provider].label;
  const cloud = CLOUDS.has(h.provider);
  return {
    target: { kind: "host", id: h.id },
    title: h.name,
    subtitle: t(cloud ? "labs.runOn.hostSubtitleCloud" : "labs.runOn.hostSubtitle", { kind, host: h.host }),
    ok,
    why: t("labs.runOn.unsupportedKind", { kind }),
    logo: <ProviderGlyph provider={h.provider} />,
    status: ok ? hostStatus(t, check) : undefined,
    badge: badgeFor(tiers, candidatesFor({ kind: "host", id: h.id }, h.provider)),
  };
}

/** The groups with something to offer, in order: this machine, Cyber CTF, servers, clouds. */
export function runOnGroups(
  t: T,
  {
    hosts,
    hostOk,
    checks,
    localNote,
    dockerRunning,
    vm,
    offerLocal,
    tiers,
  }: {
    hosts: ServerHost[];
    hostOk: (h: ServerHost) => boolean;
    checks: HostChecks;
    localNote: string;
    dockerRunning: boolean | null;
    vm: LocalVm;
    offerLocal: boolean;
    tiers: RunTier[];
  },
): OptionGroup[] {
  const forHost = (h: ServerHost) => hostOption(t, tiers, h, hostOk(h), checks[h.id]);
  const groups: OptionGroup[] = [
    { id: "local", label: t("labs.runOn.thisMachine"), options: offerLocal ? localOptions(t, tiers, localNote, dockerRunning, vm) : [] },
    { id: "servers", label: t("labs.runOn.servers"), options: hosts.filter((h) => !CLOUDS.has(h.provider)).map(forHost) },
    { id: "cloud", label: t("labs.runOn.cloud"), options: hosts.filter((h) => CLOUDS.has(h.provider)).map(forHost) },
  ];
  return groups.filter((g) => g.options.length > 0);
}
