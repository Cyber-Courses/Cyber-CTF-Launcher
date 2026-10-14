"use client";

import type { ReactNode } from "react";
import { Labs, LabsLoading } from "@/features/labs/labs-screen";
import { HomeScreen } from "@/features/home/home-screen";
import { MachineScreen } from "@/features/machine/machine-screen";
import { ServerScreen } from "@/features/servers/servers-screen";
import { CloudScreen } from "@/features/cloud/cloud-screen";
import { SettingsScreen } from "@/features/settings/settings-screen";
import { WaitingForCheck } from "@/features/machine/waiting-for-check";
import { EmptyState } from "@/components/ui/empty-state";
import { LIST_SCREEN } from "@/components/ui/screen-width";
import { authLogin, type AuthStatus, type SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import type { Tab } from "@/components/shell/shell-model";

/** The screen for `tab`, in the main column. The labs tab sizes itself (its list stops at
 *  LIST_SCREEN, a lab's page uses the shell's full width); every other screen stops at LIST_SCREEN. */
export function Screen(props: ScreenProps): ReactNode {
  if (props.tab === "labs") return <TabScreen {...props} />;
  return (
    <div className={LIST_SCREEN}>
      <TabScreen {...props} />
    </div>
  );
}

type ScreenProps = {
  tab: Tab;
  report: SystemReport | null;
  checkError: string | null;
  auth: AuthStatus | null;
  openLab: { slug: string | null; tick: number };
  onRefresh: () => void | Promise<void>;
  onNavigate: (t: Tab, slug?: string) => void;
  onAuthChange: (status: AuthStatus) => void;
  onLabChange: (title: string | null) => void;
};

function TabScreen({ tab, report, checkError, auth, openLab, onRefresh, onNavigate, onAuthChange, onLabChange }: ScreenProps): ReactNode {
  const t = useT();
  const waiting = (what: string) => <WaitingForCheck what={what} checkError={checkError} onRetry={onRefresh} />;
  const login = async () => onAuthChange(await authLogin());
  if (tab === "home")
    return <HomeScreen report={report} checkError={checkError} onRetryCheck={onRefresh} auth={auth} onNavigate={onNavigate} onLogin={login} />;
  if (tab === "settings") return <SettingsScreen auth={auth} onAuthChange={onAuthChange} onNavigate={onNavigate} />;
  if (tab === "machine") return report ? <MachineScreen report={report} onRefresh={onRefresh} onNavigate={onNavigate} /> : waiting(t("shell.screen.checking"));
  if (tab === "server") return <ServerScreen onNavigate={onNavigate} />;
  if (tab === "cloud") return <CloudScreen />;
  if (tab === "events")
    return (
      <EmptyState icon="sparkles" title={t("shell.screen.comingSoon", { title: t("shell.tabs.events") })} description={t("shell.screen.eventsDescription")} />
    );
  return report ? (
    <Labs
      loggedIn={auth?.loggedIn ?? false}
      authReady={auth !== null}
      onLogin={login}
      hostArch={report.arch}
      report={report}
      openLab={openLab}
      onDetailChange={onLabChange}
    />
  ) : checkError ? (
    waiting(t("shell.screen.loading"))
  ) : (
    // Placeholder rows, not a bare spinner: the list's shape while the machine check answers.
    <LabsLoading />
  );
}
