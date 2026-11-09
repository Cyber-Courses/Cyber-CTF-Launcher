"use client";

import { useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Eraser, FlaskConical, Info, Palette, UserRound, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { agentInfo, systemCheck, type AgentInfo, type AuthStatus } from "@/lib/tauri";
import { AboutSection } from "@/features/settings/about-section";
import { AccountSection } from "@/features/settings/account-section";
import { AppearanceSection } from "@/features/settings/appearance-section";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { LabPortsRow } from "@/features/settings/lab-ports-row";
import { LaunchDefaultsRow } from "@/features/settings/launch-defaults-row";
import { CleanupSection } from "@/features/settings/cleanup-section";
import { HypervisorRow } from "@/features/settings/hypervisor-row";
import { ManagedSection } from "@/features/settings/managed-section";
import { useManagedSettings } from "@/lib/use-managed";
import { Section, useSavedFlash } from "@/features/settings/settings-layout";
import { warn } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { useStableCallback } from "@/lib/use-stable-callback";
import { useT, type MessageKey } from "@/lib/i18n";

/* ------------------------------------------------------------------ screen */

// One section at a time, picked from a rail: a setting is one click away instead of somewhere
// down a long scroll, and the window fits its content.
const TABS = [
  { id: "account", label: "settings.tabs.account", icon: UserRound },
  { id: "appearance", label: "settings.tabs.appearance", icon: Palette },
  { id: "labs", label: "settings.tabs.labs", icon: FlaskConical },
  { id: "maintenance", label: "settings.tabs.maintenance", icon: Eraser },
  { id: "about", label: "settings.tabs.about", icon: Info },
] as const satisfies readonly { id: string; label: MessageKey; icon: LucideIcon }[];
type SettingsTab = (typeof TABS)[number]["id"];

/** How often this machine's agent is read again while signed in: it registers, goes online, and
 *  registers again for another account, in the background. */
const AGENT_POLL_MS = 5_000;

/** A read the sections show: what it answered, or why it failed (`undefined`: no failure). Both
 *  unset while it runs. */
type Read<T> = { value: T | null; error?: unknown };

/** One read on mount, and again on demand (`reread`): a failure is kept (and logged) to be said
 *  where its answer would show, never left looking like a read still running. */
function useRead<T>(read: () => Promise<T>, what: string) {
  const [state, setState] = useState<Read<T>>({ value: null });
  const reread = useStableCallback(() => {
    read()
      .then((value) => {
        setState({ value });
      })
      .catch((error: unknown) => {
        warn(what)(error);
        setState((s) => ({ value: s.value, error }));
      });
  });
  useEffect(reread, [reread]);
  return [state, reread] as const;
}

/** What the sections show about this app and machine: its version, the system check (checked
 *  again on demand) and, while signed in, this machine's agent, read again as it registers. */
function useAboutThisMachine(loggedIn: boolean) {
  const [version] = useRead(getVersion, "reading the app's version");
  const [report, recheck] = useRead(systemCheck, "system check");
  const [agent, setAgent] = useState<Read<AgentInfo>>({ value: null });
  // A failing read is logged once, not on every poll that repeats it.
  const failing = useRef(false);
  usePoll(agentInfo, AGENT_POLL_MS, {
    enabled: loggedIn,
    onValue: (value) => {
      failing.current = false;
      setAgent({ value });
    },
    onError: (error) => {
      if (!failing.current) warn("reading this machine's agent")(error);
      failing.current = true;
      setAgent((a) => ({ value: a.value, error }));
    },
  });
  return { agent: loggedIn ? agent : { value: null }, report, version, recheck };
}

/** The rail of sections; under 640px (the window's narrowest), a tab bar, text only so the
 *  longer languages fit. */
function SettingsTabs({ tab, onPick }: { tab: SettingsTab; onPick: (tab: SettingsTab) => void }) {
  const t = useT();
  return (
    <nav
      aria-label={t("settings.tabs.label")}
      className="-mx-1 flex shrink-0 gap-1 overflow-x-auto border-b border-border px-1 pt-1 pb-2 sm:mx-0 sm:block sm:w-44 sm:space-y-0.5 sm:overflow-visible sm:border-b-0 sm:px-0 sm:pb-0"
    >
      {TABS.map((tb) => (
        <button
          key={tb.id}
          type="button"
          onClick={() => {
            onPick(tb.id);
          }}
          aria-current={tab === tb.id ? "page" : undefined}
          className={cn(
            "flex h-8 shrink-0 items-center gap-2 rounded-sm px-2.5 text-left text-[0.8125rem] whitespace-nowrap transition-colors sm:w-full sm:gap-2.5",
            tab === tb.id ? "bg-glass-2 text-foreground shadow-[inset_0_0_0_1px_var(--border)]" : "text-muted-foreground hover:bg-glass hover:text-foreground",
          )}
        >
          <tb.icon className="hidden size-4 shrink-0 opacity-85 sm:block" /> {t(tb.label)}
        </button>
      ))}
    </nav>
  );
}

export function SettingsScreen({
  auth,
  onAuthChange,
  onNavigate,
}: {
  auth: AuthStatus | null;
  onAuthChange: (status: AuthStatus) => void;
  onNavigate: (tab: "machine") => void;
}) {
  const t = useT();
  const { agent, report, version, recheck } = useAboutThisMachine(auth?.loggedIn ?? false);
  const [labsSaved, flashLabs] = useSavedFlash();
  const [tab, setTab] = useState<SettingsTab>("account");
  const managed = useManagedSettings();

  return (
    // A rail beside the section; a tab bar above it in a narrow window.
    <div className="mx-auto flex max-w-[47.5rem] flex-col gap-6 pb-10 sm:flex-row sm:gap-8">
      <SettingsTabs tab={tab} onPick={setTab} />
      <div className="min-w-0 flex-1 space-y-9">
        {tab === "account" && <AccountSection auth={auth} agent={agent.value} agentError={agent.error} onAuthChange={onAuthChange} />}

        {tab === "appearance" && <AppearanceSection />}

        {tab === "labs" && (
          <Section title={t("settings.labs.title")} description={t("settings.labs.description")} saved={labsSaved}>
            <LabPortsRow onSaved={flashLabs} />
            <LaunchDefaultsRow onSaved={flashLabs} />
            <AttackBoxRows onSaved={flashLabs} />
            <HypervisorRow report={report.value} reportError={report.error} onSaved={flashLabs} onNavigate={onNavigate} onRefresh={recheck} />
          </Section>
        )}

        {tab === "maintenance" && <CleanupSection />}

        {tab === "about" && (
          <>
            <ManagedSection policy={managed?.policy ?? []} />
            <AboutSection version={version.value} versionError={version.error} report={report.value} agent={agent.value} updates={managed?.updates ?? null} />
          </>
        )}
      </div>
    </div>
  );
}
