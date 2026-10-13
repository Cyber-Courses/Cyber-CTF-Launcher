"use client";

import { useEffect, useRef, useState } from "react";
import { MoreHorizontal, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

export type MenuItem = { label: string; icon: LucideIcon; onClick: () => void; danger?: boolean };
/** Overflow menu. The dropdown is positioned `fixed` from the trigger's rect so it escapes
 *  the Panel's `overflow-hidden` (an `absolute` child would be clipped). It closes on scroll
 *  or resize, since a fixed position would otherwise drift from the button. */
export function HostMenu({ items }: { items: MenuItem[] }) {
  const t = useT();
  const btnRef = useRef<HTMLButtonElement>(null);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);

  const open = () => {
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setPos({ top: r.bottom + 4, right: Math.max(8, window.innerWidth - r.right) });
  };

  useEffect(() => {
    if (!pos) return;
    const close = () => setPos(null);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [pos]);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        aria-label={t("servers.host.moreActions")}
        onClick={() => (pos ? setPos(null) : open())}
        className="grid size-7 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-glass-2 hover:text-foreground"
      >
        <MoreHorizontal className="size-4" />
      </button>
      {pos && (
        <>
          <button type="button" aria-hidden tabIndex={-1} className="fixed inset-0 z-40 cursor-default" onClick={() => setPos(null)} />
          <div style={{ position: "fixed", top: pos.top, right: pos.right }} className="surface-glass z-50 w-36 overflow-hidden rounded-control p-1">
            {items.map((it) => (
              <button
                key={it.label}
                type="button"
                onClick={() => {
                  setPos(null);
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
        </>
      )}
    </>
  );
}
