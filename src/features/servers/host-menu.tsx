"use client";

import { useEffect, useRef, useState, type RefObject } from "react";
import { MoreHorizontal, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useEscape } from "@/lib/use-escape";
import { useT } from "@/lib/i18n";

export type MenuItem = { label: string; icon: LucideIcon; onClick: () => void; danger?: boolean };

type Position = { top: number; right: number };

/** Where the open menu sits under its button (null while closed), and the toggle. Escape closes
 *  it and gives the focus back to its button, as a menu does; a scroll or a resize closes it,
 *  since a fixed position would otherwise drift from the button. */
function useMenuPosition(button: RefObject<HTMLButtonElement | null>) {
  const [pos, setPos] = useState<Position | null>(null);
  const close = () => {
    setPos(null);
  };
  useEscape(() => {
    setPos(null);
    button.current?.focus();
  }, !!pos);

  useEffect(() => {
    if (!pos) return;
    const closeNow = () => {
      setPos(null);
    };
    // Open: the focus goes to its first item, as a menu's does.
    document.querySelector<HTMLElement>('[role="menu"] [role="menuitem"]')?.focus();
    window.addEventListener("scroll", closeNow, true);
    window.addEventListener("resize", closeNow);
    return () => {
      window.removeEventListener("scroll", closeNow, true);
      window.removeEventListener("resize", closeNow);
    };
  }, [pos]);

  const toggle = () => {
    const r = button.current?.getBoundingClientRect();
    if (pos) close();
    else if (r) setPos({ top: r.bottom + 4, right: Math.max(8, window.innerWidth - r.right) });
  };
  return { pos, close, toggle };
}

/** Overflow menu. The dropdown is positioned `fixed` from the trigger's rect so it escapes
 *  the Panel's `overflow-hidden` (an `absolute` child would be clipped). */
export function HostMenu({ items }: { items: MenuItem[] }) {
  const t = useT();
  const btnRef = useRef<HTMLButtonElement>(null);
  const { pos, close, toggle } = useMenuPosition(btnRef);
  return (
    <>
      <button
        ref={btnRef}
        type="button"
        aria-label={t("servers.host.moreActions")}
        aria-haspopup="menu"
        aria-expanded={!!pos}
        onClick={toggle}
        className="grid size-7 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-glass-2 hover:text-foreground"
      >
        <MoreHorizontal className="size-4" />
      </button>
      {pos && (
        <>
          <button type="button" aria-hidden tabIndex={-1} className="fixed inset-0 z-40 cursor-default" onClick={close} />
          <MenuList items={items} pos={pos} onPick={close} />
        </>
      )}
    </>
  );
}

/** The open menu's items; picking one closes it first. */
function MenuList({ items, pos, onPick }: { items: MenuItem[]; pos: Position; onPick: () => void }) {
  const t = useT();
  return (
    <div
      role="menu"
      aria-label={t("servers.host.moreActions")}
      style={{ position: "fixed", top: pos.top, right: pos.right }}
      className="surface-glass z-50 w-36 overflow-hidden rounded-control p-1"
    >
      {items.map((it) => (
        <button
          key={it.label}
          type="button"
          role="menuitem"
          onClick={() => {
            onPick();
            it.onClick();
          }}
          className={cn(
            "flex w-full items-center gap-2 rounded-sm px-2.5 py-1.5 text-left text-[0.8125rem] transition-colors hover:bg-glass-2",
            it.danger ? "text-destructive" : "text-foreground",
          )}
        >
          <it.icon className="size-3.5" /> {it.label}
        </button>
      ))}
    </div>
  );
}
