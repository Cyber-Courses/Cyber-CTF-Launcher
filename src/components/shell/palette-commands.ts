import { Cog, FlaskConical, Search } from "lucide-react";
import type { Command } from "@/components/command-palette";
import type { T } from "@/lib/i18n";
import { NAV, type SidebarLab, type Tab } from "@/components/shell/shell-model";

/** The command palette's entries: the actions, every screen, then every lab. */
export function paletteCommands(
  t: T,
  labs: SidebarLab[],
  { findLab, openSettings, navigate }: { findLab: () => void; openSettings: () => void; navigate: (tab: Tab, slug?: string) => void },
): Command[] {
  const title = (tab: Tab) => t(`shell.tabs.${tab}`);
  return [
    {
      id: "find-lab",
      label: t("shell.palette.findLab"),
      hint: "/",
      icon: Search,
      keywords: t("shell.palette.findLabKeywords"),
      group: t("shell.palette.groups.actions"),
      run: findLab,
    },
    {
      id: "settings",
      label: t("shell.palette.openSettings"),
      icon: Cog,
      keywords: t("shell.palette.openSettingsKeywords"),
      group: t("shell.palette.groups.actions"),
      run: openSettings,
    },
    ...NAV.filter((n) => !n.soon).map((n) => ({
      id: `go-${n.id}`,
      label: t("shell.palette.goTo", { screen: title(n.id) }),
      icon: n.icon,
      keywords: title(n.id),
      group: t("shell.palette.groups.screens"),
      run: () => {
        navigate(n.id);
      },
    })),
    ...labs.map((l) => ({
      id: `lab-${l.slug}`,
      label: l.title,
      hint: l.category,
      icon: FlaskConical,
      keywords: t("shell.palette.labKeywords", { category: l.category }),
      group: t("shell.palette.groups.labs"),
      run: () => {
        navigate("labs", l.slug);
      },
    })),
  ];
}
