"use client";

import { useState } from "react";
import { CommandPalette } from "@/components/command-palette";
import { UpdateBanner } from "@/components/update-banner";
import { Toaster } from "@/components/ui/toaster";
import { Sidebar } from "@/components/shell/sidebar";
import { ShellHeader } from "@/components/shell/shell-header";
import { Screen } from "@/components/shell/screen";
import { SettingsWindowView } from "@/components/shell/settings-window";
import { paletteCommands } from "@/components/shell/palette-commands";
import { breadcrumbs, type Tab } from "@/components/shell/shell-model";
import { markOnboarded } from "@/components/shell/onboarded-flag";
import { openSettingsWindow, useShellListeners, useShellMount } from "@/components/shell/use-shell-setup";
import { useNavigation, type OpenLab } from "@/components/shell/use-navigation";
import { useAuthSession } from "@/components/shell/use-auth-session";
import { useSidebarLabs } from "@/components/shell/use-sidebar-labs";
import { useSystemCheck } from "@/components/shell/use-system-check";
import { QuitGuard } from "@/features/app/quit-guard";
import { LaunchConfirm } from "@/features/app/launch-confirm";
import { Onboarding } from "@/features/onboarding/onboarding";
import { PortModePrompt } from "@/features/labs/port-mode-prompt";
import { OfflineStartPrompt } from "@/features/labs/offline-start-prompt";
import { useAttackBoxAutoStart } from "@/features/labs/attack-box-auto";
import { useActiveOperations } from "@/lib/deploy-store";
import { useAppearanceSync } from "@/lib/appearance";
import { useT } from "@/lib/i18n";

/**
 * The main window: sidebar, header and the open screen, plus the app-wide listeners (deep links,
 * requests from the Settings window, session changes, keyboard shortcuts) and prompts (quit guard,
 * launch confirmation, port mode). The Settings window runs this shell too, as just its screen.
 */
export function AppShell() {
  useAppearanceSync();
  const nav = useNavigation();
  const { navigate, findALab } = nav;
  const [paletteOpen, setPaletteOpen] = useState(false);
  // The lab page open in Labs, for the breadcrumb (null on the list and other screens).
  const [labTitle, setLabTitle] = useState<string | null>(null);
  const system = useSystemCheck();
  const { check } = system;
  const session = useAuthSession();
  const mount = useShellMount(check, session.init);
  const { setOnboarded } = mount;
  const mainWindow = mount.ready && !mount.settingsWindow;

  useShellListeners({ navigate, findALab, setOnboarded, setPaletteOpen, mainWindow });

  // What each busy lab is doing and the step it is at, for a live line under its name. Main
  // window only: Settings runs this shell too, and its view uses none of this, so it must not
  // double every poll (docker and vagrant are run for the workloads one).
  const ops = useActiveOperations(mainWindow);
  // Main window only: two windows must not both start a box.
  useAttackBoxAutoStart(ops, mainWindow);
  const sidebarLabs = useSidebarLabs(session.auth?.loggedIn, ops, mainWindow);

  function completeOnboarding() {
    markOnboarded();
    setOnboarded(true);
    nav.setTab("labs");
    void check();
    // Pick up a sign-in done during onboarding: the shell's auth was read once at mount (before
    // onboarding), so re-read the persisted session, otherwise the app stays "offline" until a
    // restart even though the user just signed in.
    session.reread();
  }

  if (!mount.ready) return <div className="h-dvh bg-background" />;
  // Settings runs standalone in its own window: no sidebar, no onboarding, just the screen.
  if (mount.settingsWindow) return <SettingsWindowView auth={session.auth} onAuthChange={session.changed} />;
  // The launch confirmation is the same one, at the same place, during the onboarding and after
  // it: a cloud launch asked from the website while the onboarding is up must be shown (the
  // website takes the silence for a no), and still be there once the onboarding ends.
  // With its own Toaster: `tell()` shows errors as toasts while the window has focus.
  if (!mount.onboarded)
    return (
      <>
        <LaunchConfirm />
        <Onboarding onComplete={completeOnboarding} />
        <Toaster />
      </>
    );
  return (
    <>
      <LaunchConfirm />
      <MainWindow
        nav={nav}
        system={system}
        session={session}
        chrome={{ isMac: mount.isMac, version: mount.version }}
        palette={{ open: paletteOpen, setOpen: setPaletteOpen }}
        labTitle={labTitle}
        onLabChange={setLabTitle}
        sidebarLabs={sidebarLabs}
      />
    </>
  );
}

type MainWindowProps = {
  nav: ReturnType<typeof useNavigation>;
  system: ReturnType<typeof useSystemCheck>;
  session: ReturnType<typeof useAuthSession>;
  /** The platform (for the titlebar band and shortcuts) and the app version. */
  chrome: { isMac: boolean; version: string | null };
  palette: { open: boolean; setOpen: (open: boolean) => void };
  labTitle: string | null;
  onLabChange: (title: string | null) => void;
  sidebarLabs: ReturnType<typeof useSidebarLabs>;
};

/** The onboarded main window: the palette, the app-wide prompts, the sidebar and the open screen.
 *  The launch confirmation stays with the shell, at the place it has during the onboarding, so a
 *  question asked then is still there once it ends. */
function MainWindow({ nav, system, session, chrome, palette, labTitle, onLabChange, sidebarLabs }: MainWindowProps) {
  const t = useT();
  const { tab, openLab, navigate, findALab } = nav;
  const commands = paletteCommands(t, sidebarLabs.labs, { findLab: findALab, openSettings: openSettingsWindow, navigate });

  // `overflow-clip`, not `hidden`: a hidden box can still be scrolled (focus, scrollIntoView, a
  // wheel over the sidebar), which shifted the whole window up under the titlebar.
  return (
    <div className="flex h-dvh overflow-clip bg-background text-foreground">
      <CommandPalette
        key={palette.open ? "open" : "closed"}
        open={palette.open}
        onClose={() => {
          palette.setOpen(false);
        }}
        commands={commands}
      />
      <QuitGuard />
      <PortModePrompt />
      <OfflineStartPrompt />
      <Toaster />
      <Sidebar
        tab={tab}
        isMac={chrome.isMac}
        version={chrome.version}
        labCount={sidebarLabs.labs.length}
        activeLabs={sidebarLabs.active}
        auth={session.auth}
        onFind={() => {
          palette.setOpen(true);
        }}
        onNavigate={navigate}
        onOpenSettings={openSettingsWindow}
        onAuthChange={session.changed}
      />

      <MainColumn tab={tab} openLab={openLab} system={system} session={session} labTitle={labTitle} onLabChange={onLabChange} onNavigate={navigate} />
    </div>
  );
}

/** The main column: the header with its breadcrumb, the update banner and the open screen. */
function MainColumn({
  tab,
  openLab,
  system,
  session,
  labTitle,
  onLabChange,
  onNavigate,
}: Pick<MainWindowProps, "system" | "session" | "labTitle" | "onLabChange"> & {
  tab: Tab;
  openLab: OpenLab;
  onNavigate: (tab: Tab, slug?: string) => void;
}) {
  const t = useT();
  const { report, checkError, check } = system;
  const title = (id: Tab) => t(`shell.tabs.${id}`);
  return (
    <main className="flex min-w-0 flex-1 flex-col overflow-clip">
      <ShellHeader crumbs={breadcrumbs(tab, labTitle, title, t("shell.header.thisMachine"))} report={report} checkError={checkError} />
      <div className="flex-1 overflow-y-auto">
        <UpdateBanner />
        <div className="mx-auto w-full max-w-[160rem] px-7 pt-7 pb-10 2xl:px-10">
          <Screen
            tab={tab}
            report={report}
            checkError={checkError}
            auth={session.auth}
            openLab={openLab}
            onRefresh={check}
            onNavigate={onNavigate}
            onAuthChange={session.setAuth}
            onLabChange={onLabChange}
          />
        </div>
      </div>
    </main>
  );
}
