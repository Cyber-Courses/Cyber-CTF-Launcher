"use client";

import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { isSettingsWindow } from "@/components/shell/settings-window";
import type { Tab } from "@/components/shell/shell-model";
import { readOnboarded } from "@/components/shell/onboarded-flag";
import { useAppShortcuts } from "@/components/shell/use-app-shortcuts";
import { useWindowRequests } from "@/components/shell/use-window-requests";
import { openSettings } from "@/lib/tauri";
import { ignore, tell } from "@/lib/failure";
import { useLabLinks } from "@/lib/deep-link";
import { isMac as onMac } from "@/lib/platform";
import { translate } from "@/lib/i18n";

/** Opens the Settings window; a failure is told to the player. */
export const openSettingsWindow = () => void openSettings().catch(tell(translate("shell.failures.openSettings")));

/** What the shell reads once mounted, from state only the browser has: whether this is the
 *  Settings window, the app version, the platform, the onboarding flag. `ready` once read. */
export function useShellMount(check: () => Promise<void>, readSession: () => void) {
  const [onboarded, setOnboarded] = useState(true);
  const [ready, setReady] = useState(false);
  const [settingsWindow, setSettingsWindow] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  const [isMac, setIsMac] = useState(true);
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- read once on mount: the window marker and localStorage don't exist during the static render
    setSettingsWindow(isSettingsWindow());
    void check();
    getVersion().then(setVersion).catch(ignore("no version outside the app"));
    setIsMac(onMac());
    readSession();
    setOnboarded(readOnboarded());
    setReady(true);
  }, [check, readSession]);
  return { onboarded, setOnboarded, ready, settingsWindow, version, isMac };
}

/** The app-wide listeners: requests from the Settings window, deep links and the keyboard
 *  shortcuts. The first two only in the main window. */
export function useShellListeners({
  navigate,
  findALab,
  setOnboarded,
  setPaletteOpen,
  mainWindow,
}: {
  navigate: (tab: Tab, slug?: string) => void;
  findALab: () => void;
  setOnboarded: (onboarded: boolean) => void;
  setPaletteOpen: Dispatch<SetStateAction<boolean>>;
  mainWindow: boolean;
}) {
  // Asked from the Settings window: switch screens, or show the onboarding again.
  useWindowRequests(
    {
      navigate,
      replayOnboarding: () => {
        setOnboarded(false);
      },
    },
    mainWindow,
  );

  // A cyberctf://labs/<slug> link (the one that opened the app, or one opened since) shows that
  // lab, whatever screen is open. Not in the Settings window: the main window takes it.
  useLabLinks((slug) => {
    navigate("labs", slug);
  }, mainWindow);

  useAppShortcuts({
    togglePalette: () => {
      setPaletteOpen((o) => !o);
    },
    openSettings: openSettingsWindow,
    findLab: findALab,
  });
}
