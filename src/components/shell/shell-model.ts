import { CalendarDays, Cloud, FlaskConical, LayoutDashboard, type LucideIcon, MonitorCog, Server } from "lucide-react";
import type { ActiveOperation, LabStatus, SystemReport } from "@/lib/tauri";
import { engineName } from "@/features/machine/setup-steps/engines";
import { PROVIDER_LABELS } from "@/features/machine/hypervisors";
import { prettyLabId } from "@/lib/format";

// The app shell's pure logic: the screens and their order, the breadcrumb, the header's engine
// line, which labs the sidebar lists as running and the keyboard shortcuts.

export type Tab = "home" | "labs" | "machine" | "setup" | "server" | "cloud" | "events" | "settings";

export type NavEntry = { id: Tab; icon: LucideIcon; soon?: boolean; sep?: boolean };

// Grouped: the lab area (what you run), then the setup area (the compute it runs on), then
// Settings. `sep` draws a divider before the item, at each group boundary. Labels are the
// screens' titles (shell.tabs).
export const NAV: NavEntry[] = [
  { id: "home", icon: LayoutDashboard },
  { id: "labs", icon: FlaskConical },
  { id: "events", icon: CalendarDays, soon: true },
  { id: "machine", icon: MonitorCog, sep: true },
  { id: "server", icon: Server },
  { id: "cloud", icon: Cloud },
];

/** Whether `id` names a screen of the sidebar (what another window may ask to show). */
export const isNavTab = (id: string): id is Tab => NAV.some((n) => n.id === id);

/** Where you are: a lab page reads "Labs / <lab>", Settings "Settings / Settings", any other
 *  screen "This machine / <screen>". */
export function breadcrumbs(tab: Tab, labTitle: string | null, title: (tab: Tab) => string, thisMachine: string): string[] {
  if (tab === "labs" && labTitle) return [title("labs"), labTitle];
  return [tab === "settings" ? title("settings") : thisMachine, title(tab)];
}

/** The header's engine line: "OrbStack 29.5.3" while Docker runs (null otherwise), and the first
 *  hypervisor ready on this machine. */
export function engineSummary(report: SystemReport) {
  // `docker --version` reads "Docker version 29.5.3, build d1c06ef": the number is enough next to the engine's name.
  const version = report.docker.version?.match(/\d+\.\d+(\.\d+)?/)?.[0] ?? null;
  const engine = report.dockerRunning && report.dockerEngine ? `${engineName(report.dockerEngine)}${version ? ` ${version}` : ""}` : null;
  const local = report.vmProviders.find((p) => !p.remote && p.available && p.hypervisor !== false);
  const hypervisor = local ? (PROVIDER_LABELS[local.provider] ?? local.provider) : null;
  return { engine, hypervisor };
}

export type SidebarLab = { id: string; slug: string; title: string; category: string };
export type ActiveLab = SidebarLab & { op: ActiveOperation | null };

/**
 * The labs the sidebar lists as running or busy: deploying (backend), with an operation in flight,
 * or seen running by the workload scan. A lab's own status, once known, has the last word over
 * the scan: the scan counts any VM a lab left behind (a half-started VM lab), so the sidebar said
 * "Running" while the Overview, from the same statuses, counted one lab fewer. A lab `labs`
 * doesn't list (the catalogue couldn't be loaded) is still listed, by its id made readable, with
 * no slug to open: a running lab must not vanish with the catalogue.
 */
export function activeLabs(
  labs: SidebarLab[],
  deploying: Set<string>,
  ops: Map<string, ActiveOperation>,
  runningIds: Set<string>,
  statuses: Record<string, LabStatus>,
): ActiveLab[] {
  const scanSaysRunning = (id: string) => runningIds.has(id) && (statuses[id] ? statuses[id].running : true);
  const active = (id: string) => deploying.has(id) || ops.has(id) || scanSaysRunning(id);
  const listed = new Set(labs.map((l) => l.id));
  const unlisted = [...new Set([...deploying, ...ops.keys(), ...runningIds])]
    .filter((id) => !listed.has(id) && active(id))
    .sort()
    .map((id): SidebarLab => ({ id, slug: "", title: prettyLabId(id), category: "" }));
  return [...labs.filter((l) => active(l.id)), ...unlisted].map((l) => ({
    ...l,
    op: ops.get(l.id) ?? (deploying.has(l.id) ? { labId: l.id, op: "launch" as const, machine: null, step: null } : null),
  }));
}

export type Shortcut = "palette" | "settings" | "findLab";
type Key = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">;

/**
 * The shell's keyboard shortcuts: ⌘K / Ctrl+K toggles the command palette; Ctrl+, opens Settings
 * on Linux and Windows, where the menu bar (and its shortcut) is gone (macOS keeps Cmd+, in the
 * app menu); "/" jumps to Labs search, except while typing in a field.
 */
export function shortcutFor(e: Key, mac: boolean, typing: boolean): Shortcut | null {
  // Cmd+K on macOS, Ctrl+K elsewhere: on macOS Ctrl+K is the system's "delete to end of line"
  // in every text field (and the shell's kill-line in the attack box terminal).
  if ((e.key === "k" || e.key === "K") && (mac ? e.metaKey : e.ctrlKey)) return "palette";
  if ((e.ctrlKey || e.metaKey) && e.key === "," && !mac) return "settings";
  if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey || typing) return null;
  return "findLab";
}

/** Whether the focus is in a text field, where "/" is typed rather than a shortcut. */
export function isTyping(el: Element | null): boolean {
  const h = el as HTMLElement | null;
  return !!h && (h.tagName === "INPUT" || h.tagName === "TEXTAREA" || h.isContentEditable);
}
