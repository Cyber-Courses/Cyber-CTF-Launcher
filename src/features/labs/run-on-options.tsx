"use client";

import { useEffect, useState } from "react";
import { type Provider, type ServerHost, type ServerTest } from "@/lib/tauri";
import { testHost } from "@/features/servers/use-server-list";
import { type Tone } from "@/components/ui/status-pill";
import { TypeIcon } from "@/components/ui/type-icon";
import { KIND } from "@/features/servers/host-setup/constants";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { CLOUDS } from "@/features/labs/lab-runtime";
import type { RunTarget } from "@/features/labs/run-on";
import { type T } from "@/lib/i18n";

// The places "Where should it run?" offers, by group: this machine, Cyber CTF, servers, clouds.

type Status = { tone: Tone; label: string; title?: string };
type Option = { target: RunTarget; title: string; subtitle: string; ok: boolean; why?: string; logo: React.ReactNode; status?: Status };
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

/** This machine (with its Docker engine's state, for a container lab), and a VM on `localVm`. */
function localOptions(t: T, localNote: string, dockerRunning: boolean | null, localVm: Provider | null): Option[] {
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
  return local;
}

const hostedOption = (t: T): Option => ({
  target: { kind: "hosted" },
  title: t("labs.runOn.hostedTitle"),
  subtitle: t("labs.runOn.hostedSubtitle"),
  ok: true,
  logo: <CyberCtfLogo />,
  status: { tone: "ok", label: t("labs.runOn.available") },
});

/** A saved server or cloud account; one the lab can't run on says why, without a status. */
function hostOption(t: T, h: ServerHost, ok: boolean, check: ServerTest | undefined): Option {
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
    localVm,
    hosted,
    offerLocal,
  }: {
    hosts: ServerHost[];
    hostOk: (h: ServerHost) => boolean;
    checks: HostChecks;
    localNote: string;
    dockerRunning: boolean | null;
    localVm: Provider | null;
    hosted: boolean;
    offerLocal: boolean;
  },
): OptionGroup[] {
  const forHost = (h: ServerHost) => hostOption(t, h, hostOk(h), checks[h.id]);
  const groups: OptionGroup[] = [
    { id: "local", label: t("labs.runOn.thisMachine"), options: offerLocal ? localOptions(t, localNote, dockerRunning, localVm) : [] },
    { id: "hosted", label: "Cyber CTF", options: hosted ? [hostedOption(t)] : [] },
    { id: "servers", label: t("labs.runOn.servers"), options: hosts.filter((h) => !CLOUDS.has(h.provider)).map(forHost) },
    { id: "cloud", label: t("labs.runOn.cloud"), options: hosts.filter((h) => CLOUDS.has(h.provider)).map(forHost) },
  ];
  return groups.filter((g) => g.options.length > 0);
}
