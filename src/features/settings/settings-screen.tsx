"use client";

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Eraser, FlaskConical, Info, Palette, UserRound, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { agentInfo, systemCheck, type AgentInfo, type AuthStatus, type SystemReport } from "@/lib/tauri";
import { AboutSection } from "@/features/settings/about-section";
import { AccountSection } from "@/features/settings/account-section";
import { AppearanceSection } from "@/features/settings/appearance-section";
import { AttackBoxRows } from "@/features/settings/attack-box-rows";
import { LabPortsRow } from "@/features/settings/lab-ports-row";
import { CleanupSection } from "@/features/settings/cleanup-section";
import { HypervisorRow } from "@/features/settings/hypervisor-row";
import { Section, useSavedFlash } from "@/features/settings/settings-layout";
import { warn } from "@/lib/failure";
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

/** What the sections show about this app and machine: its version, the system check (checked
 *  again on demand) and, once signed in, this machine's agent. */
function useAboutThisMachine(loggedIn: boolean) {
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [report, setReport] = useState<SystemReport | null>(null);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {
        setVersion(null);
      });
    systemCheck()
      .then(setReport)
      .catch(() => {
        setReport(null);
      });
  }, []);
  const recheck = () => {
    systemCheck().then(setReport).catch(warn("system check"));
  };

  // The agent registers once signed in, so re-read it whenever auth changes.
  useEffect(() => {
    if (loggedIn)
      agentInfo()
        .then(setAgent)
        .catch(() => {
          setAgent(null);
        });
  }, [loggedIn]);
  return { agent: loggedIn ? agent : null, report, version, recheck };
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

  return (
    // A rail beside the section; a tab bar above it in a narrow window.
    <div className="mx-auto flex max-w-[47.5rem] flex-col gap-6 pb-10 sm:flex-row sm:gap-8">
      <SettingsTabs tab={tab} onPick={setTab} />
      <div className="min-w-0 flex-1 space-y-9">
        {tab === "account" && <AccountSection auth={auth} agent={agent} onAuthChange={onAuthChange} />}

        {tab === "appearance" && <AppearanceSection />}

        {tab === "labs" && (
          <Section title={t("settings.labs.title")} description={t("settings.labs.description")} saved={labsSaved}>
            <LabPortsRow onSaved={flashLabs} />
            <AttackBoxRows onSaved={flashLabs} />
            <HypervisorRow report={report} onSaved={flashLabs} onNavigate={onNavigate} onRefresh={recheck} />
          </Section>
        )}

        {tab === "maintenance" && <CleanupSection />}

        {tab === "about" && <AboutSection version={version} report={report} agent={agent} />}
      </div>
    </div>
  );
}
