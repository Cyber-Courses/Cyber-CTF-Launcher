"use client";

import { SettingsScreen } from "@/features/settings/settings-screen";
import { Toaster } from "@/components/ui/toaster";
import type { AuthStatus } from "@/lib/tauri";
import { showInMainWindow } from "@/lib/app-events";
import { useT } from "@/lib/i18n";
import { WindowTitleBar } from "@/components/window-title-bar";

/** Whether this webview is the dedicated Settings window (opened by `open_settings` with ?window=settings). */
export function isSettingsWindow() {
  try {
    return new URLSearchParams(window.location.search).get("window") === "settings";
  } catch {
    return false;
  }
}

// Settings rendered on its own in the dedicated `settings` window: just the title bar and the
// screen. The "set up a hypervisor" link shows the Machine screen in the main window.
export function SettingsWindowView({ auth, onAuthChange }: { auth: AuthStatus | null; onAuthChange: (status: AuthStatus) => void }) {
  const t = useT();
  return (
    <main className="flex h-dvh flex-col overflow-clip bg-background text-foreground">
      <Toaster />
      <WindowTitleBar title={t("shell.tabs.settings")} className="border-b border-border" />
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-[52rem] px-7 pt-7 pb-10">
          <SettingsScreen auth={auth} onAuthChange={onAuthChange} onNavigate={showInMainWindow} />
        </div>
      </div>
    </main>
  );
}
