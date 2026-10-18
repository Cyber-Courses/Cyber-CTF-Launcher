"use client";

import { SettingsScreen } from "@/features/settings/settings-screen";
import { Toaster } from "@/components/ui/toaster";
import type { AuthStatus } from "@/lib/tauri";
import { showInMainWindow } from "@/lib/app-events";
import { useIsMac } from "@/lib/platform";
import { useT } from "@/lib/i18n";

/** Whether this webview is the dedicated Settings window (opened by `open_settings` with ?window=settings). */
export function isSettingsWindow() {
  try {
    return new URLSearchParams(window.location.search).get("window") === "settings";
  } catch {
    return false;
  }
}

// Settings rendered on its own in the dedicated `settings` window: just the titlebar band and
// the screen. The "set up a hypervisor" link shows the Machine screen in the main window.
export function SettingsWindowView({ auth, onAuthChange }: { auth: AuthStatus | null; onAuthChange: (status: AuthStatus) => void }) {
  const t = useT();
  // The titlebar band stands in for macOS's hidden title; elsewhere the window's own title bar
  // already says "Settings".
  const isMac = useIsMac();
  return (
    <main className="flex h-dvh flex-col overflow-clip bg-background text-foreground">
      <Toaster />
      {isMac && (
        <div data-tauri-drag-region className="flex h-12 shrink-0 items-center justify-center border-b border-border text-[0.8125rem] font-medium">
          <span className="pointer-events-none">{t("shell.tabs.settings")}</span>
        </div>
      )}
      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto w-full max-w-[52rem] px-7 pt-7 pb-10">
          <SettingsScreen auth={auth} onAuthChange={onAuthChange} onNavigate={showInMainWindow} />
        </div>
      </div>
    </main>
  );
}
