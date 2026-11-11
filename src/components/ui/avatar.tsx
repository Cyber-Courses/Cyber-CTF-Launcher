"use client";

import { LogoTile } from "@/components/ui/logo-tile";
import { Tip } from "@/components/ui/tip";
import { useT } from "@/lib/i18n";

/** Something an avatar stands for: its name (the tooltip and the accessible name, never shown as
 *  text), its mark when there is one (`fill`: an app icon that fills the circle), else the
 *  `initials` it shows (by default, from its name). */
export type AvatarItem = { name: string; src?: string; fill?: boolean; initials?: string };

const CIRCLE = "relative flex size-8 shrink-0 overflow-hidden rounded-full ring-2 ring-background select-none";
const FALLBACK = "flex size-full items-center justify-center bg-muted text-[0.6875rem] font-medium text-muted-foreground";

/** A name's initials, for an avatar given none: "Hyper-V" → "HV", "Parallels" → "P". */
const initials = (name: string) =>
  name
    .split(/[\s-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w.charAt(0).toUpperCase())
    .join("");

/** One round avatar, after shadcn/ui's: the mark on white, or the initials on the muted
 *  background when there is none; named for screen readers, its name in a tooltip on hover. */
function Avatar({ item }: { item: AvatarItem }) {
  return (
    <Tip text={item.name} align="center" echo>
      <span role="img" aria-label={item.name} className={CIRCLE}>
        {item.src ? <LogoTile src={item.src} fill={item.fill} round /> : <span className={FALLBACK}>{item.initials ?? initials(item.name)}</span>}
      </span>
    </Tip>
  );
}

/** Avatars stacked after shadcn/ui's group: overlapping a little, each ringed in the background's
 *  colour so the overlap reads; past `max`, the rest as one "+N" avatar naming them. */
export function AvatarGroup({ items, max = 8, className }: { items: AvatarItem[]; max?: number; className?: string }) {
  const t = useT();
  const shown = items.slice(0, max);
  const rest = items.slice(shown.length);
  return (
    <div className={`flex -space-x-2 ${className ?? ""}`}>
      {shown.map((item) => (
        <Avatar key={item.name} item={item} />
      ))}
      {rest.length > 0 && (
        <Tip text={rest.map((i) => i.name).join(", ")} align="center" echo>
          <span role="img" aria-label={t("ui.avatars.more", { count: rest.length, names: rest.map((i) => i.name).join(", ") })} className={CIRCLE}>
            <span className={FALLBACK}>+{rest.length}</span>
          </span>
        </Tip>
      )}
    </div>
  );
}
