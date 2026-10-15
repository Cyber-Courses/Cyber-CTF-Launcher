"use client";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { isNavTab, type Tab } from "@/components/shell/shell-model";
import { NAVIGATE_EVENT, REPLAY_ONBOARDING_EVENT } from "@/lib/app-events";
import { ignore } from "@/lib/failure";
import { useTauriEvent } from "@/lib/use-tauri-event";

/**
 * What the Settings window asks the main window: switch screens, or show the onboarding again.
 * The main window comes to the front, since the request came from another window. Listens
 * while `enabled` (the main window, once ready).
 */
export function useWindowRequests({ navigate, replayOnboarding }: { navigate: (tab: Tab) => void; replayOnboarding: () => void }, enabled: boolean) {
  const toFront = () => void getCurrentWindow().setFocus().catch(ignore("the window manager keeps focus where it is"));
  useTauriEvent(
    NAVIGATE_EVENT,
    (screen) => {
      if (typeof screen === "string" && isNavTab(screen)) navigate(screen);
      toFront();
    },
    enabled,
  );
  useTauriEvent(
    REPLAY_ONBOARDING_EVENT,
    () => {
      replayOnboarding();
      toFront();
    },
    enabled,
  );
}
