"use client";

import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Eraser, FlaskConical, Info, Palette, UserRound } from "lucide-react";
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
import { useT } from "@/lib/i18n";

/* ------------------------------------------------------------------ screen */

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
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [report, setReport] = useState<SystemReport | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [labsSaved, flashLabs] = useSavedFlash();

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
    systemCheck()
      .then(setReport)
      .catch(() => setReport(null));
  }, []);
  const recheck = () => systemCheck().then(setReport).catch(warn("system check"));

  // The agent registers once signed in, so re-read it whenever auth changes.
  useEffect(() => {
    if (auth?.loggedIn)
      agentInfo()
        .then(setAgent)
        .catch(() => setAgent(null));
  }, [auth?.loggedIn]);

  // One section at a time, picked from a rail: a setting is one click away instead of somewhere
  // down a long scroll, and the window fits its content.
  const tabs = [
    { id: "account", label: t("settings.tabs.account"), icon: UserRound },
    { id: "appearance", label: t("settings.tabs.appearance"), icon: Palette },
    { id: "labs", label: t("settings.tabs.labs"), icon: FlaskConical },
    { id: "maintenance", label: t("settings.tabs.maintenance"), icon: Eraser },
    { id: "about", label: t("settings.tabs.about"), icon: Info },
  ] as const;
  type Tab = (typeof tabs)[number]["id"];
  const [tab, setTab] = useState<Tab>("account");

  return (
    // A rail beside the section; under 640px (the window's narrowest), a tab bar above it, text
    // only so the longer languages fit.
    <div className="mx-auto flex max-w-[47.5rem] flex-col gap-6 pb-10 sm:flex-row sm:gap-8">
      <nav
        aria-label={t("settings.tabs.label")}
        className="-mx-1 flex shrink-0 gap-1 overflow-x-auto border-b border-border px-1 pt-1 pb-2 sm:mx-0 sm:block sm:w-44 sm:space-y-0.5 sm:overflow-visible sm:border-b-0 sm:px-0 sm:pb-0"
      >
        {tabs.map((tb) => (
          <button
            key={tb.id}
            type="button"
            onClick={() => setTab(tb.id)}
            aria-current={tab === tb.id ? "page" : undefined}
            className={cn(
              "flex h-8 shrink-0 items-center gap-2 rounded-sm px-2.5 text-left text-[0.8125rem] whitespace-nowrap transition-colors sm:w-full sm:gap-2.5",
              tab === tb.id
                ? "bg-glass-2 text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
                : "text-muted-foreground hover:bg-glass hover:text-foreground",
            )}
          >
            <tb.icon className="hidden size-4 shrink-0 opacity-85 sm:block" /> {tb.label}
          </button>
        ))}
      </nav>

      <div className="min-w-0 flex-1 space-y-9">
        {tab === "account" && <AccountSection auth={auth} agent={auth?.loggedIn ? agent : null} onAuthChange={onAuthChange} />}

        {tab === "appearance" && <AppearanceSection />}

        {tab === "labs" && (
          <Section title={t("settings.labs.title")} description={t("settings.labs.description")} saved={labsSaved}>
            <LabPortsRow onSaved={flashLabs} />
            <AttackBoxRows onSaved={flashLabs} />
            <HypervisorRow report={report} onSaved={flashLabs} onNavigate={onNavigate} onRefresh={recheck} />
          </Section>
        )}

        {tab === "maintenance" && <CleanupSection />}

        {tab === "about" && <AboutSection version={version} report={report} agent={auth?.loggedIn ? agent : null} />}
      </div>
    </div>
  );
}
