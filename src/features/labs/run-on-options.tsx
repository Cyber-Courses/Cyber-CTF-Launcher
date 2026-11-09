"use client";

import { useEffect, useState } from "react";
import { Container } from "lucide-react";
import { PROVIDER_NAMES, type DockerEngine, type LabRunOptions, type Provider, type ServerHost, type ServerTest } from "@/lib/tauri";
import { badgeOf, pathOf, refusalOn, type RunBadge } from "@/features/labs/run-on-badges";
import { testHost } from "@/features/servers/use-server-list";
import { StatusPill, type Tone } from "@/components/ui/status-pill";
import { TypeIcon } from "@/components/ui/type-icon";
import { ProviderGlyph } from "@/features/servers/provider-glyph";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { ENGINES, engineName } from "@/features/machine/setup-steps/engines";
import { vmFormTarget } from "@/features/labs/lab-runtime";
import { isCloudProvider } from "@/lib/tauri";
import type { RunTarget } from "@/features/labs/run-on";
import { type T } from "@/lib/i18n";

// The places "Where should it run?" offers, by group: this machine, Cyber CTF, servers, clouds.

type Status = { tone: Tone; label: string; title?: string };
type Option = {
  target: RunTarget;
  title: string;
  subtitle?: string;
  ok: boolean;
  why?: string;
  logo: React.ReactNode;
  status?: Status;
  badge?: RunBadge;
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

/** A tier badge or a live status, as one aligned pill (same line-height, dot and gap). */
export function RowPill({ tone, title, children }: { tone: Tone; title?: string; children: React.ReactNode }) {
  return (
    <span title={title} className="inline-flex items-center">
      <StatusPill tone={tone}>
        <span className="whitespace-nowrap">{children}</span>
      </StatusPill>
    </span>
  );
}

/** An engine's brand logo for the engine step's rows (the neutral container icon when it has none). */
export function EngineLogo({ id }: { id: DockerEngine }) {
  const logo = ENGINES.find((e) => e.id === id)?.logo;
  return (
    <TypeIcon>
      {logo ? (
        // eslint-disable-next-line @next/next/no-img-element -- static export, plain asset
        <img src={logo} alt="" className="size-5 object-contain" draggable={false} />
      ) : (
        <Container className="size-5 text-foreground" strokeWidth={1.5} />
      )}
    </TypeIcon>
  );
}

/** This machine's container engine, for a container lab: running, stopped, or no status at all
 *  (a VM lab, or the engine's state not yet known). Shared by the platform row and the
 *  Containers form. */
function engineStatus(t: T, dockerRunning: boolean | null): Status | undefined {
  if (dockerRunning === null) return undefined;
  return dockerRunning
    ? { tone: "ok", label: t("labs.runOn.running") }
    : { tone: "warn", label: t("labs.runOn.engineStopped"), title: t("labs.runOn.engineStoppedHint") };
}

/** The "How should it run?" step, shown only when there are two forms: This machine's VM forms
 *  (helper VM, one VM per machine), or — when `account` is a cloud account — its one VM
 *  (cloud-docker) and one VM per machine (cloud-vm). The hypervisor is a later step for a local
 *  form, so it isn't named by one; a cloud form runs on the account, so Start finishes it. */
export function vmFormOptions(t: T, options: LabRunOptions | null, forms: RunTarget[], account?: ServerHost | null): Option[] {
  return forms.map((form) => {
    // A cloud account's forms: the account's own logo and the path's tier badge.
    if (form.kind === "host") {
      const perMachine = !!form.perMachine;
      return {
        target: form,
        title: t(perMachine ? "labs.runOn.cloudFormPerMachine" : "labs.runOn.cloudFormOneVm"),
        subtitle: t(perMachine ? "labs.runOn.cloudPerMachineHint" : "labs.runOn.cloudOneVmHint"),
        ok: true,
        badge: account ? badgeOf(pathOf(options, form, account.provider)) : undefined,
        logo: account ? <ProviderGlyph provider={account.provider} /> : <HypervisorLogo provider={null} />,
        status: { tone: "ok", label: t("labs.runOn.ready") },
      };
    }
    const provider = form.kind === "local-vm" || form.kind === "local-vms" ? form.provider : null;
    const perMachine = form.kind === "local-vms";
    return {
      target: form,
      title: t(perMachine ? "labs.runOn.formPerMachine" : "labs.runOn.formInVm"),
      subtitle: t(perMachine ? "labs.runOn.perMachineVmHint" : "labs.runOn.inVmFormNote"),
      ok: true,
      badge: badgeOf(pathOf(options, form)),
      logo: <HypervisorLogo provider={provider} />,
      status: { tone: "ok", label: t("labs.runOn.ready") },
    };
  });
}

/** The "Which hypervisor?" step: the ready hypervisors that can run the chosen `form`, as full rows.
 *  The default (Settings' choice, else the first) is marked and preselected by the caller. */
export function hypervisorOptions(t: T, form: "local-vm" | "local-vms", hypervisors: Provider[], fallback: Provider | null): Option[] {
  return hypervisors.map((p) => ({
    target: vmFormTarget(form, p),
    title: PROVIDER_NAMES[p],
    ok: true,
    logo: <HypervisorLogo provider={p} />,
    status: { tone: "ok", label: t(p === fallback ? "labs.runOn.defaultChoice" : "labs.runOn.ready") },
  }));
}

/** Everything the step-1 "This machine" rows need: whether a container form runs here
 *  (`offerContainers`), the note/engine state and the engine it will use for the Container row, and
 *  the VM forms available here (`vmForms`). */
export type LocalRows = { offerContainers: boolean; localNote: string; dockerRunning: boolean | null; engine?: DockerEngine | null; vmForms: RunTarget[] };

/** Step 1's rows for "This machine": up to two, each its own kind of run here. Container (the
 *  engine's state) when the lab has a container form the run options give it (once read; refused
 *  there, the row says Isoloom's reason), and VM (the hypervisor's logo) when a VM form is ready
 *  here. No tier badges here: a place is a place, not a form being compared (step 2). */
function localPlatformRows(t: T, local: LocalRows, options: LabRunOptions | null): Option[] {
  const { offerContainers, localNote, dockerRunning, engine } = local;
  const rows: Option[] = [];
  const containers = !options || options.docker !== null;
  const refused = containers ? undefined : refusalOn(t, options, null);
  if (offerContainers && (containers || refused)) {
    rows.push({
      target: { kind: "local" },
      title: t("labs.runOn.formContainers"),
      // The engine it will run on (Docker Desktop, OrbStack…), not "Docker": a neutral container
      // icon, the engine named, its state. The engine is chosen in its own step when several answer.
      subtitle: engine ? engineName(engine) : localNote,
      ok: containers,
      why: refused,
      logo: (
        <TypeIcon>
          <Container className="size-5 text-foreground" strokeWidth={1.5} />
        </TypeIcon>
      ),
      status: containers ? engineStatus(t, dockerRunning) : undefined,
    });
  }
  const vm = localVmRow(t, local);
  if (vm) rows.push(vm);
  return rows;
}

/** The step-1 "VM" row, or null when no VM form is ready here. A VM lab runs its own VMs as
 *  `{ kind: "local" }`; a container lab runs a helper VM (`local-vm`) and, when dual-form, one VM
 *  per machine too. Picking it leads to the form step (two forms), the hypervisor step (one form,
 *  several hypervisors), or straight to Start — no inline switch. */
function localVmRow(t: T, local: LocalRows): Option | null {
  const { offerContainers, localNote, vmForms } = local;
  if (!offerContainers) {
    return { target: { kind: "local" }, title: t("labs.runOn.inAVm"), subtitle: localNote, ok: true, logo: <HypervisorLogo provider={null} /> };
  }
  const first = vmForms.at(0);
  if (!first || (first.kind !== "local-vm" && first.kind !== "local-vms")) return null;
  const twoForms = vmForms.length >= 2;
  return {
    target: first,
    title: t("labs.runOn.inAVm"),
    subtitle: twoForms ? t("labs.runOn.vmForms") : t("labs.runOn.inVmFormNote"),
    ok: true,
    logo: <HypervisorLogo provider={first.provider} />,
    status: { tone: "ok", label: t("labs.runOn.ready") },
  };
}

/** A saved server or cloud account; one the lab can't run on says why, without a status. */
function hostOption(t: T, options: LabRunOptions | null, h: ServerHost, ok: boolean, check: ServerTest | undefined): Option {
  const kind = PROVIDER_NAMES[h.provider];
  const cloud = isCloudProvider(h.provider);
  return {
    target: { kind: "host", id: h.id },
    title: h.name,
    subtitle: t(cloud ? "labs.runOn.hostSubtitleCloud" : "labs.runOn.hostSubtitle", { kind, host: h.host }),
    ok,
    why: refusalOn(t, options, h.provider) ?? t("labs.runOn.unsupportedKind", { kind }),
    logo: <ProviderGlyph provider={h.provider} />,
    status: ok ? hostStatus(t, check) : undefined,
    badge: badgeOf(pathOf(options, { kind: "host", id: h.id }, h.provider)),
  };
}

/** Step 1's platforms with something to offer, in order: this machine (up to two rows, Containers
 *  and In a VM), then servers, then clouds. A host runs one way, derived downstream. */
export function platformGroups(
  t: T,
  {
    hosts,
    hostOk,
    checks,
    local,
    offerLocal,
    options,
  }: {
    hosts: ServerHost[];
    hostOk: (h: ServerHost) => boolean;
    checks: HostChecks;
    local: LocalRows;
    offerLocal: boolean;
    options: LabRunOptions | null;
  },
): OptionGroup[] {
  const forHost = (h: ServerHost) => hostOption(t, options, h, hostOk(h), checks[h.id]);
  const groups: OptionGroup[] = [
    { id: "local", label: t("labs.runOn.thisMachine"), options: offerLocal ? localPlatformRows(t, local, options) : [] },
    { id: "servers", label: t("labs.runOn.servers"), options: hosts.filter((h) => !isCloudProvider(h.provider)).map(forHost) },
    { id: "cloud", label: t("labs.runOn.cloud"), options: hosts.filter((h) => isCloudProvider(h.provider)).map(forHost) },
  ];
  return groups.filter((g) => g.options.length > 0);
}
