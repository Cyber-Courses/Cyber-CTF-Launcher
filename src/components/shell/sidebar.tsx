"use client";

import { memo } from "react";
import { Cog, type LucideIcon, Search } from "lucide-react";
import { Account } from "@/features/account/account";
import { CtfMark } from "@/components/brand/mark";
import { StatusDot } from "@/components/ui/status-pill";
import { operationLabel } from "@/lib/deploy-store";
import type { AuthStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { NAV, type ActiveLab, type Tab } from "@/components/shell/shell-model";

type SidebarProps = {
  tab: Tab;
  isMac: boolean;
  version: string | null;
  labCount: number;
  activeLabs: ActiveLab[];
  auth: AuthStatus | null;
  onFind: () => void;
  onNavigate: (tab: Tab, slug?: string) => void;
  onOpenSettings: () => void;
  onAuthChange: (status: AuthStatus) => void;
};

/** The main window's left column: brand, the find button, the screens, the labs running now,
 *  Settings and the account. */
export function Sidebar({ tab, isMac, version, labCount, activeLabs, auth, onFind, onNavigate, onOpenSettings, onAuthChange }: SidebarProps) {
  const t = useT();
  const title = (id: Tab) => t(`shell.tabs.${id}`);
  return (
    <aside className="flex w-[14.5rem] shrink-0 flex-col border-r border-border bg-card">
      <SidebarTop isMac={isMac} version={version} onFind={onFind} />
      <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2.5">
        {NAV.map((n) => (
          <div key={n.id}>
            {n.sep && <div className="mx-1.5 my-2 h-px bg-border" />}
            <NavItem
              id={n.id}
              icon={n.icon}
              active={tab === n.id}
              dim={n.soon && tab !== n.id}
              label={title(n.id)}
              meta={n.soon ? t("shell.sidebar.soon") : n.id === "labs" && labCount > 0 ? String(labCount) : undefined}
              metaSize={n.soon ? "text-[0.625rem]" : "text-[0.6875rem]"}
              onSelect={onNavigate}
            />
          </div>
        ))}

        {activeLabs.length > 0 && (
          <>
            <div className="section-label px-2 pt-4 pb-1.5">{t("shell.sidebar.running")}</div>
            {activeLabs.map((l) => (
              <ActiveLabItem key={l.id} lab={l} onNavigate={onNavigate} />
            ))}
          </>
        )}

        <div className="flex-1" />
        <NavItem
          id="settings"
          icon={Cog}
          active={false}
          label={title("settings")}
          meta={isMac ? "⌘," : "Ctrl ,"}
          metaSize="text-[0.625rem]"
          onSelect={onOpenSettings}
        />
      </nav>

      <div className="mt-1 border-t border-border px-2.5 py-2.5">
        <Account status={auth} onChange={onAuthChange} online={!!auth?.loggedIn} />
      </div>
    </aside>
  );
}

/** The column's top: the titlebar band, the brand with the version, and the find button. */
function SidebarTop({ isMac, version, onFind }: { isMac: boolean; version: string | null; onFind: () => void }) {
  const t = useT();
  return (
    <>
      {/* macOS titlebar band (traffic lights) inside the column, so the divider runs to the top.
          Linux and Windows keep their own title bar, so there it would only be a blank strip. */}
      {isMac && <div data-tauri-drag-region className="h-10 shrink-0" />}
      <div data-tauri-drag-region className={`flex items-center gap-2.5 px-3.5 pb-3 ${isMac ? "" : "pt-4"}`}>
        <CtfMark className="pointer-events-none size-[1.35rem]" />
        <span className="pointer-events-none text-[0.875rem] font-semibold tracking-tight">Cyber CTF</span>
        {version && <span className="pointer-events-none ml-auto font-mono text-[0.625rem] text-faint">v{version}</span>}
      </div>

      <button
        type="button"
        onClick={onFind}
        className="mx-2.5 mb-2.5 flex h-8 items-center justify-between rounded-sm bg-glass px-2.5 text-[0.75rem] text-faint shadow-[inset_0_0_0_1px_var(--border)] transition-colors hover:text-muted-foreground"
      >
        <span className="flex items-center gap-2">
          <Search className="size-3.5" /> {t("shell.sidebar.find")}
        </span>
        <kbd className="kbd">{isMac ? "⌘K" : "Ctrl K"}</kbd>
      </button>
    </>
  );
}

/** One sidebar entry: 2rem high, icon then label (and a note on the right); the current one sits
 *  on a glass fill. Memoized: the shell re-renders on every poll that changes anything. */
const NavItem = memo(function NavItem({
  id,
  icon: I,
  active,
  dim,
  label,
  meta,
  metaSize,
  onSelect,
}: {
  id: Tab;
  icon: LucideIcon;
  active: boolean;
  dim?: boolean;
  label: string;
  meta?: string;
  metaSize: string;
  onSelect: (id: Tab) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => {
        onSelect(id);
      }}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-8 w-full items-center gap-2.5 rounded-sm px-2.5 text-left text-[0.8125rem] transition-colors",
        active
          ? "bg-glass-2 text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
          : // Dimmed with the faint ink, not opacity: still readable (4.5:1) at this size.
            cn(dim ? "text-faint" : "text-muted-foreground", "hover:bg-glass hover:text-foreground"),
      )}
    >
      <I className="size-4 shrink-0 opacity-85" />
      {label}
      {meta && <span className={cn("ml-auto font-mono text-faint", metaSize)}>{meta}</span>}
    </button>
  );
});

/** The same entry: its lab and what it is doing (a new object on each list rebuild). */
const sameActiveLab = (a: { lab: ActiveLab; onNavigate: unknown }, b: { lab: ActiveLab; onNavigate: unknown }) =>
  a.onNavigate === b.onNavigate &&
  a.lab.id === b.lab.id &&
  a.lab.slug === b.lab.slug &&
  a.lab.title === b.lab.title &&
  a.lab.op?.op === b.lab.op?.op &&
  a.lab.op?.machine === b.lab.op?.machine &&
  a.lab.op?.step === b.lab.op?.step;

/** A lab running or busy: its name, then what it is doing and the step it is at. */
const ActiveLabItem = memo(function ActiveLabItem({ lab, onNavigate }: { lab: ActiveLab; onNavigate: (tab: Tab, slug?: string) => void }) {
  const t = useT();
  const { op } = lab;
  return (
    <button
      type="button"
      // No slug (a lab the catalogue, unreachable, doesn't list): the Labs screen, which says why.
      onClick={() => {
        onNavigate("labs", lab.slug || undefined);
      }}
      className="flex w-full items-start gap-2.5 rounded-sm px-2 py-1.5 text-left transition-colors hover:bg-glass"
    >
      <StatusDot tone={op ? "warn" : "ok"} pulse={!!op} className="mt-[0.4rem]" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[0.8125rem] text-foreground">{lab.title}</span>
        {op ? (
          <>
            <span className="block truncate font-mono text-[0.625rem] text-warning">{operationLabel(op)}</span>
            {/* The step the operation is at, re-entering as it changes. */}
            {op.step && (
              <span key={op.step} className="animate-fade-in block truncate font-mono text-[0.625rem] text-faint" title={op.step}>
                {op.step}
              </span>
            )}
          </>
        ) : (
          <span className="block truncate font-mono text-[0.625rem] text-faint">{t("shell.sidebar.labRunning")}</span>
        )}
      </span>
    </button>
  );
}, sameActiveLab);
